#!/usr/bin/env python
"""Release script for the Herald monorepo.

Bumps every version site in this repository, refreshes lockfiles, validates,
then creates the release commit and v-prefixed tag and pushes them.

Version sites (keep in sync with the repository layout):
  - backend/Cargo.toml        [workspace.package] version (inherited by all backend crates)
  - sdk/rust/Cargo.toml       [package] version (standalone crate, DEC-js-sdk-017)
  - frontend/package.json
  - demo/package.json
  - sdk/web/package.json      npm herald-auth-web
  - sdk/node/package.json     npm herald-sdk

Lockfile refresh:
  - backend/Cargo.lock + sdk/rust/Cargo.lock  via `cargo check` (validation side effect)
  - */package-lock.json                        via `npm install --package-lock-only`

Usage:
  python scripts/release.py            # recommend next version, exit (or --yes to accept)
  python scripts/release.py 0.6.0      # release 0.6.0
  python scripts/release.py --dry-run  # preflight + resolved version only
  python scripts/release.py --no-push  # commit + tag locally, do not push

SDK publishes to npm/crates.io are separate manual workflows
(publish-web-sdk.yml / publish-node-sdk.yml / publish-sdk.yml).
"""

import argparse
import json
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

SEMVER_RE = re.compile(r"^v?(\d+)\.(\d+)\.(\d+)$")

CARGO_WORKSPACE_MANIFEST = REPO_ROOT / "backend" / "Cargo.toml"
CARGO_SDK_MANIFEST = REPO_ROOT / "sdk" / "rust" / "Cargo.toml"
CARGO_BACKEND_DIR = REPO_ROOT / "backend"
CARGO_SDK_DIR = REPO_ROOT / "sdk" / "rust"

# frontend uses --legacy-peer-deps everywhere (CI, Docker); keep its lock in
# sync with the same flag so the resolution matches.
NPM_PACKAGES = {
    "frontend": ["--legacy-peer-deps"],
    "demo": [],
    "sdk/web": [],
    "sdk/node": [],
}

RELEASE_PATHS = [
    "backend/Cargo.toml",
    "backend/Cargo.lock",
    "sdk/rust/Cargo.toml",
    "sdk/rust/Cargo.lock",
    *[f"{name}/package.json" for name in NPM_PACKAGES],
    *[f"{name}/package-lock.json" for name in NPM_PACKAGES],
]


@dataclass(frozen=True)
class Semver:
    major: int
    minor: int
    patch: int

    @classmethod
    def parse(cls, text: str) -> "Semver":
        match = SEMVER_RE.fullmatch(text.strip())
        if not match:
            raise ValueError(f"Invalid version '{text}'. Use X.Y.Z or vX.Y.Z.")
        return cls(*(int(part) for part in match.groups()))

    def bump_patch(self) -> "Semver":
        return Semver(self.major, self.minor, self.patch + 1)

    def __str__(self) -> str:
        return f"{self.major}.{self.minor}.{self.patch}"


@dataclass(frozen=True)
class FileChange:
    path: Path
    before: str | None
    after: str


def require_executable(name: str) -> str:
    resolved = shutil.which(name)
    if not resolved:
        raise RuntimeError(f"Required executable '{name}' not found on PATH.")
    return resolved


def run_cmd(command: list[str], cwd: Path, capture: bool = False) -> subprocess.CompletedProcess:
    return subprocess.run(
        command,
        cwd=cwd,
        capture_output=capture,
        text=True,
        encoding="utf-8",
        errors="replace",
    )


def git(*args: str, capture: bool = False) -> subprocess.CompletedProcess:
    return run_cmd([require_executable("git"), *args], REPO_ROOT, capture=capture)


def ensure_success(result: subprocess.CompletedProcess, message: str) -> None:
    if result.returncode != 0:
        raise RuntimeError(message)


def ensure_on_main() -> None:
    result = git("branch", "--show-current", capture=True)
    ensure_success(result, "Unable to determine current git branch.")
    branch = result.stdout.strip()
    if branch != "main":
        raise RuntimeError(f"Release must run on main, current branch is '{branch}'.")


def ensure_clean_worktree() -> None:
    result = git("status", "--porcelain", capture=True)
    ensure_success(result, "Unable to inspect git status.")
    if result.stdout.strip():
        raise RuntimeError("Working tree is not clean. Commit or stash changes before release.")


def ensure_remote_access() -> None:
    result = git("ls-remote", "--exit-code", "origin", capture=True)
    ensure_success(result, "Remote 'origin' is not accessible.")


def list_tags() -> list[str]:
    result = git("tag", "--list", capture=True)
    ensure_success(result, "Unable to list git tags.")
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def latest_semver_tag(tags: list[str]) -> Semver | None:
    versions: list[Semver] = []
    for tag in tags:
        try:
            versions.append(Semver.parse(tag))
        except ValueError:
            continue
    if not versions:
        return None
    return max(versions, key=lambda item: (item.major, item.minor, item.patch))


def ensure_tag_available(version: str, tags: list[str]) -> None:
    conflicts = [tag for tag in (version, f"v{version}") if tag in tags]
    if conflicts:
        raise RuntimeError(f"Tag conflict: {', '.join(conflicts)} already exists.")

    remote = git("ls-remote", "--tags", "origin", version, f"v{version}", capture=True)
    ensure_success(remote, "Unable to inspect remote tags.")
    remote_conflicts = []
    for line in remote.stdout.splitlines():
        if "refs/tags/" in line:
            remote_conflicts.append(line.rsplit("refs/tags/", 1)[1].removesuffix("^{}"))
    if remote_conflicts:
        unique = sorted(set(remote_conflicts))
        raise RuntimeError(f"Remote tag conflict: {', '.join(unique)} already exists.")


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def update_cargo_version(path: Path, version: str, section: str) -> FileChange:
    """Replace the first `version = "..."` inside the given TOML section."""
    if not path.is_file():
        raise RuntimeError(f"Expected version file is missing: {display_path(path)}")

    # newline="" preserves the file's exact line endings (CRLF or LF); the
    # default universal-newline read would silently rewrite CRLF files to LF.
    with open(path, encoding="utf-8", newline="") as handle:
        lines = handle.readlines()
    in_section = False
    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith("["):
            in_section = stripped == section
            continue
        if not in_section:
            continue
        body = line.rstrip("\r\n")
        match = re.match(r'(\s*version\s*=\s*")([^"]+)(".*)', body)
        if match:
            ending = line[len(body):]
            lines[index] = f"{match.group(1)}{version}{match.group(3)}{ending}"
            with open(path, "w", encoding="utf-8", newline="") as handle:
                handle.writelines(lines)
            return FileChange(path, match.group(2), version)
    raise RuntimeError(f"No version key found in [{section}] of {display_path(path)}.")


def update_package_json(path: Path, version: str) -> FileChange:
    if not path.is_file():
        raise RuntimeError(f"Expected version file is missing: {display_path(path)}")

    data = json.loads(path.read_text(encoding="utf-8"))
    before = data.get("version")
    data["version"] = version
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return FileChange(path, before, version)


def update_version_files(version: str) -> list[FileChange]:
    changes = [
        update_cargo_version(CARGO_WORKSPACE_MANIFEST, version, "[workspace.package]"),
        update_cargo_version(CARGO_SDK_MANIFEST, version, "[package]"),
        update_package_json(REPO_ROOT / "frontend" / "package.json", version),
        update_package_json(REPO_ROOT / "demo" / "package.json", version),
        update_package_json(REPO_ROOT / "sdk" / "web" / "package.json", version),
        update_package_json(REPO_ROOT / "sdk" / "node" / "package.json", version),
    ]
    return changes


def refresh_npm_locks() -> None:
    npm = require_executable("npm")
    for name, extra_flags in NPM_PACKAGES.items():
        package_dir = REPO_ROOT / name
        command = [npm, "install", "--package-lock-only", "--no-audit", "--no-fund", *extra_flags]
        print(f"Refreshing lockfile: {' '.join(command[1:])} (cwd: {name})", flush=True)
        result = run_cmd(command, package_dir)
        if result.returncode != 0:
            raise RuntimeError(f"Lockfile refresh failed for {name}.")


def validate() -> None:
    cargo = require_executable("cargo")
    npm = require_executable("npm")

    checks: list[tuple[str, list[str], Path]] = [
        ("backend cargo check", [cargo, "check"], CARGO_BACKEND_DIR),
        ("rust sdk cargo check", [cargo, "check"], CARGO_SDK_DIR),
        *[
            (f"{name} type-check", [npm, "run", "type-check"], REPO_ROOT / name)
            for name in NPM_PACKAGES
        ],
    ]
    for label, command, cwd in checks:
        print(f"Validating: {label} ({' '.join(command[1:])}, cwd: {cwd.relative_to(REPO_ROOT)})", flush=True)
        result = run_cmd(command, cwd)
        if result.returncode != 0:
            raise RuntimeError(f"Validation failed: {label}.")


def commit_tag_and_push(version: str, push: bool) -> str:
    release_tag = f"v{version}"
    existing_paths = [path for path in RELEASE_PATHS if (REPO_ROOT / path).is_file()]
    add = git("add", *existing_paths)
    ensure_success(add, "Unable to stage release files.")

    staged = git("diff", "--cached", "--quiet")
    if staged.returncode == 0:
        raise RuntimeError(f"No version file changes detected for {version}.")

    commit = git("commit", "-m", f"chore: bump version to {version}")
    ensure_success(commit, "Unable to create release commit.")

    tag = git("tag", release_tag)
    ensure_success(tag, f"Unable to create tag {release_tag}.")

    rev = git("rev-parse", "--short", "HEAD", capture=True)
    ensure_success(rev, "Unable to resolve release commit hash.")
    commit_hash = rev.stdout.strip()

    if push:
        push_commit = git("push")
        if push_commit.returncode != 0:
            raise RuntimeError(f"Push failed. Commit {commit_hash} and tag {release_tag} remain local.")

        push_tag = git("push", "origin", release_tag)
        if push_tag.returncode != 0:
            raise RuntimeError(f"Tag push failed. Commit {commit_hash} and tag {release_tag} remain local.")

    return commit_hash


def resolve_target_version(raw_version: str | None, assume_yes: bool, tags: list[str]) -> str:
    if raw_version:
        version = str(Semver.parse(raw_version))
        if raw_version.startswith("v"):
            print(f"Normalized input version {raw_version} -> {version}; release tag will be v{version}.")
        return version

    latest = latest_semver_tag(tags)
    recommendation = str(latest.bump_patch() if latest else Semver(0, 1, 0))
    if not assume_yes:
        latest_text = str(latest) if latest else "none"
        raise RuntimeError(
            f"Recommended version is {recommendation} based on latest semver tag {latest_text}. "
            "Re-run with this version or pass --yes to accept the recommendation."
        )
    return recommendation


def main() -> int:
    parser = argparse.ArgumentParser(description="Release project version with v-prefixed git tags.")
    parser.add_argument("version", nargs="?", help="Target version, X.Y.Z or vX.Y.Z. Final tag is always vX.Y.Z.")
    parser.add_argument("--yes", action="store_true", help="Accept the auto-recommended version when version is omitted.")
    parser.add_argument("--no-push", action="store_true", help="Create the release commit and tag locally without pushing.")
    parser.add_argument("--dry-run", action="store_true", help="Run preflight checks and print the resolved version without editing files.")
    args = parser.parse_args()

    try:
        ensure_on_main()
        ensure_clean_worktree()
        ensure_remote_access()
        tags = list_tags()
        version = resolve_target_version(args.version, args.yes, tags)
        ensure_tag_available(version, tags)

        print(f"Release version: {version}")
        release_tag = f"v{version}"
        print(f"Release tag: {release_tag}")
        if args.dry_run:
            return 0

        changes = update_version_files(version)
        for change in changes:
            rel_path = change.path.relative_to(REPO_ROOT)
            before = change.before if change.before is not None else "<missing>"
            print(f"Updated {rel_path}: {before} -> {change.after}")

        refresh_npm_locks()
        validate()
        commit_hash = commit_tag_and_push(version, push=not args.no_push)
        push_text = "pushed" if not args.no_push else "created locally"
        print(f"Release {version} {push_text}: commit {commit_hash}, tag {release_tag}")
        print("Reminder: SDK publishes are manual workflows (publish-web-sdk / publish-node-sdk / publish-sdk).")
        return 0
    except Exception as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
