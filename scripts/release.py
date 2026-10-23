#!/usr/bin/env python
"""Release script for the Herald monorepo.

Bumps the version sites in this repository, refreshes lockfiles, validates,
then creates the release commit and v-prefixed tag and pushes them.

Version sites (keep in sync with the repository layout):
  - backend/Cargo.toml        [workspace.package] version (inherited by all backend crates)
  - frontend/package.json
  - demo/package.json
  - sdk/rust/Cargo.toml       [package] version (standalone crate, DEC-js-sdk-017)
  - sdk/web/package.json      npm herald-auth-web
  - sdk/node/package.json     npm herald-sdk

The three SDK sites bump conditionally: an SDK's version advances only when
that SDK has something to publish. sdk/rust and sdk/node publish when their
sources changed since the previous release tag; sdk/web also publishes when
the backend OpenAPI spec changed, because its typed client is regenerated
from that spec at publish time. The last-published spec is tracked in
sdk/web/.spec-hash; a missing hash file counts as a change so bootstrapping
errs toward one extra publish, never a missed one.

Lockfile refresh:
  - backend/Cargo.lock + sdk/rust/Cargo.lock  via `cargo check` (validation side effect)
  - */package-lock.json                        via `npm install --package-lock-only`

Usage:
  python scripts/release.py            # recommend next version, exit (or --yes to accept)
  python scripts/release.py 0.6.0      # release 0.6.0
  python scripts/release.py --dry-run  # preflight, resolved version and SDK publish decisions
  python scripts/release.py --no-push  # commit + tag locally, do not push

SDK publishes to npm/crates.io run automatically: pushing the tag triggers
the CD pipeline (cd.yml), which compares each SDK's version with the previous
release tag and skips the publish job when the version did not change — so an
SDK without changes emits no empty release on npm/crates.io and its version
stays behind the project version until it changes again. The publish
workflows (publish-sdk.yml / publish-node-sdk.yml / publish-web-sdk.yml)
remain available manually via workflow_dispatch for re-runs.
"""

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
import tempfile
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

# Hash of the OpenAPI spec the last web SDK publish was generated from; the
# release script updates it whenever it bumps the web SDK version.
WEB_SPEC_HASH_FILE = REPO_ROOT / "sdk" / "web" / ".spec-hash"

RELEASE_PATHS = [
    "backend/Cargo.toml",
    "backend/Cargo.lock",
    "sdk/rust/Cargo.toml",
    "sdk/rust/Cargo.lock",
    "sdk/web/.spec-hash",
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


def previous_release_tag(tags: list[str], current_version: str) -> str | None:
    """Highest semver tag strictly below the releasing version."""
    current = Semver.parse(current_version)
    current_key = (current.major, current.minor, current.patch)
    best_tag: str | None = None
    best_key: tuple[int, int, int] | None = None
    for tag in tags:
        try:
            version = Semver.parse(tag)
        except ValueError:
            continue
        key = (version.major, version.minor, version.patch)
        if key >= current_key:
            continue
        if best_key is None or key > best_key:
            best_tag, best_key = tag, key
    return best_tag


def sources_changed_since(previous_tag: str | None, paths: list[str]) -> bool:
    """True when `paths` have committed changes after `previous_tag`.

    Runs before this release's bump is committed, so the previous release's
    own version bump sits inside the previous tag's tree and never counts as
    a change.
    """
    if previous_tag is None:
        return True
    result = git("diff", "--quiet", previous_tag, "HEAD", "--", *paths)
    return result.returncode != 0


def current_openapi_surface_hash() -> str:
    """Hash of the API surface the web SDK's typed client is generated from.

    Exports the spec exactly the way publish-web-sdk.yml regenerates the
    client at publish time. The spec's info.version embeds the backend crate
    version and therefore changes with every project release; it is dropped
    before hashing so a version bump alone never looks like an API change.
    Sorting keys on top makes the hash independent of serialization order.
    """
    cargo = require_executable("cargo")
    with tempfile.TemporaryDirectory(prefix="herald-release-") as tmp:
        spec_path = Path(tmp) / "openapi.json"
        result = run_cmd(
            [
                cargo,
                "run",
                "--manifest-path",
                str(REPO_ROOT / "backend" / "app" / "Cargo.toml"),
                "--bin",
                "herald-app",
                "--",
                "--export-openapi",
                str(spec_path),
            ],
            REPO_ROOT,
        )
        ensure_success(result, "OpenAPI spec export failed; cannot decide the web SDK bump.")
        data = json.loads(spec_path.read_text(encoding="utf-8"))
    data.setdefault("info", {}).pop("version", None)
    normalized = json.dumps(data, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(normalized.encode("utf-8")).hexdigest()


def decide_sdk_bumps(previous_tag: str | None) -> tuple[dict[str, tuple[bool, str]], str]:
    """Decide per SDK whether this release publishes it (and bumps its version).

    rust/node: their own sources must have changed since the previous release.
    web: sources changed, OR the backend OpenAPI spec changed since the last
    web publish (sdk/web/.spec-hash); the spec export needs a backend build.
    """
    decisions: dict[str, tuple[bool, str]] = {}
    for label in ("sdk/rust", "sdk/node"):
        if sources_changed_since(previous_tag, [label]):
            decisions[label] = (True, f"sources changed since {previous_tag or 'first release'}")
        else:
            decisions[label] = (False, f"no source changes since {previous_tag}")

    spec_hash = current_openapi_surface_hash()
    published_hash = (
        WEB_SPEC_HASH_FILE.read_text(encoding="utf-8").strip()
        if WEB_SPEC_HASH_FILE.is_file()
        else ""
    )
    source_changed = sources_changed_since(previous_tag, ["sdk/web"])
    spec_changed = published_hash != spec_hash  # missing file => hash mismatch => publish
    if source_changed:
        decisions["sdk/web"] = (True, f"sources changed since {previous_tag or 'first release'}")
    elif spec_changed:
        if published_hash:
            decisions["sdk/web"] = (True, "backend API spec changed since the last web SDK publish")
        else:
            decisions["sdk/web"] = (True, f"first run of spec tracking ({display_path(WEB_SPEC_HASH_FILE)} missing)")
    else:
        decisions["sdk/web"] = (False, f"sources and API spec unchanged since {previous_tag}")
    return decisions, spec_hash


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


def update_version_files(version: str, sdk_decisions: dict[str, tuple[bool, str]]) -> list[FileChange]:
    changes = [
        update_cargo_version(CARGO_WORKSPACE_MANIFEST, version, "[workspace.package]"),
        update_package_json(REPO_ROOT / "frontend" / "package.json", version),
        update_package_json(REPO_ROOT / "demo" / "package.json", version),
    ]
    if sdk_decisions["sdk/rust"][0]:
        changes.append(update_cargo_version(CARGO_SDK_MANIFEST, version, "[package]"))
    if sdk_decisions["sdk/node"][0]:
        changes.append(update_package_json(REPO_ROOT / "sdk" / "node" / "package.json", version))
    if sdk_decisions["sdk/web"][0]:
        changes.append(update_package_json(REPO_ROOT / "sdk" / "web" / "package.json", version))
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
    parser.add_argument("--dry-run", action="store_true", help="Preflight, resolved version and SDK publish decisions, without editing files (still exports the OpenAPI spec, which builds the backend once).")
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

        previous_tag = previous_release_tag(tags, version)
        print(f"Previous release tag: {previous_tag or '<none, first release>'}")
        sdk_decisions, spec_hash = decide_sdk_bumps(previous_tag)
        for label, (bump, reason) in sdk_decisions.items():
            action = "publish (bump)" if bump else "skip (hold version)"
            print(f"{label}: {action} — {reason}")

        if args.dry_run:
            return 0

        changes = update_version_files(version, sdk_decisions)
        for change in changes:
            rel_path = change.path.relative_to(REPO_ROOT)
            before = change.before if change.before is not None else "<missing>"
            print(f"Updated {rel_path}: {before} -> {change.after}")
        if sdk_decisions["sdk/web"][0]:
            WEB_SPEC_HASH_FILE.write_text(f"{spec_hash}\n", encoding="utf-8")
            print(f"Recorded published API spec hash in {display_path(WEB_SPEC_HASH_FILE)}")

        refresh_npm_locks()
        validate()
        commit_hash = commit_tag_and_push(version, push=not args.no_push)
        push_text = "pushed" if not args.no_push else "created locally"
        print(f"Release {version} {push_text}: commit {commit_hash}, tag {release_tag}")
        if not args.no_push:
            print(f"Tag push triggers the CD pipeline (docker images, npm/crates.io SDK publishes, GitHub Release).")
        return 0
    except Exception as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
