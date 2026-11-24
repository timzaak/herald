from __future__ import annotations

import importlib.util
import io
import json
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch


REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location("push_under_test", REPO_ROOT / "scripts" / "push.py")
assert SPEC and SPEC.loader
push = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = push
SPEC.loader.exec_module(push)


class PushRecoveryTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory(prefix="t-tools-push-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name) / "work"
        self.remote = Path(temporary.name) / "remote.git"
        self.root.mkdir()
        self.command("git", "init", "--bare", str(self.remote))
        self.command("git", "init", "-b", "main")
        self.command("git", "config", "user.name", "Push Test")
        self.command("git", "config", "user.email", "push-test@example.invalid")
        self.command("git", "config", "core.hooksPath", str(Path(temporary.name) / "no-hooks"))
        self.source = self.root / "sample.txt"
        self.source.write_text("base\n", encoding="utf-8")
        self.command("git", "add", ".")
        self.command("git", "commit", "-m", "base")
        self.command("git", "remote", "add", "origin", str(self.remote))
        self.command("git", "push", "-u", "origin", "main")
        self.base_head = self.command("git", "rev-parse", "HEAD").stdout.strip()
        self.addCleanup(patch.stopall)
        patch.object(push, "REPO_ROOT", self.root).start()

    def command(self, *args: str) -> subprocess.CompletedProcess:
        result = subprocess.run(args, cwd=self.root, capture_output=True, text=True, encoding="utf-8")
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def run_script(self, *extra: str, session: str = "recovery") -> tuple[int, str]:
        output = io.StringIO()
        with redirect_stdout(output), redirect_stderr(output):
            code = push.main(["--ci-session", session, "--message", "phase implementation", *extra])
        return code, output.getvalue()

    def remote_head(self) -> str:
        return self.command("git", "--git-dir", str(self.remote), "rev-parse", "refs/heads/main").stdout.strip()

    def fail_push(self, *args: str, capture: bool = False):
        if args[0] == "push":
            return subprocess.CompletedProcess(args, 1, "", "simulated transport failure")
        return self.original_git(*args, capture=capture)

    def create_failed_push(self) -> str:
        self.source.write_text("implementation\n", encoding="utf-8")
        self.original_git = push.git
        with patch.object(push, "git", side_effect=self.fail_push):
            code, _ = self.run_script()
        self.assertEqual(code, 1)
        head = push.current_head()
        self.assertNotEqual(head, self.base_head)
        self.assertEqual(self.remote_head(), self.base_head)
        self.assertEqual(self.command("git", "status", "--short").stdout, "")
        return head

    def test_failed_push_retries_existing_commit_without_creating_another(self) -> None:
        head = self.create_failed_push()
        code, output = self.run_script()
        self.assertEqual(code, 0, output)
        self.assertEqual(push.current_head(), head)
        self.assertEqual(self.remote_head(), head)
        self.assertEqual(push.load_push_record("recovery")["status"], "completed")

    def test_clean_tree_with_new_session_pushes_unpublished_commit(self) -> None:
        head = self.create_failed_push()
        code, output = self.run_script(session="fresh-retry")
        self.assertEqual(code, 0, output)
        self.assertEqual(self.remote_head(), head)

    def test_runtime_cursor_write_does_not_block_pending_recovery(self) -> None:
        self.create_failed_push()
        runtime = self.root / ".ai" / "super-run" / "demo" / ".state.json"
        runtime.parent.mkdir(parents=True)
        runtime.write_text('{"status":"in_progress"}\n', encoding="utf-8")
        code, output = self.run_script()
        self.assertEqual(code, 0, output)
        self.assertEqual(self.remote_head(), push.current_head())

    def test_interruption_after_commit_recovers_from_committing_record(self) -> None:
        self.source.write_text("implementation\n", encoding="utf-8")
        save = push.save_push_record

        def interrupt(session: str, record: dict) -> None:
            if record["status"] == "pending":
                raise RuntimeError("simulated interruption after git commit")
            save(session, record)

        with patch.object(push, "save_push_record", side_effect=interrupt):
            code, _ = self.run_script()
        self.assertEqual(code, 1)
        head = push.current_head()
        self.assertEqual(push.load_push_record("recovery")["status"], "committing")
        code, output = self.run_script()
        self.assertEqual(code, 0, output)
        self.assertEqual((push.current_head(), self.remote_head()), (head, head))

    def test_code_or_target_change_blocks_pending_recovery(self) -> None:
        self.create_failed_push()
        self.source.write_text("later edit\n", encoding="utf-8")
        code, output = self.run_script()
        self.assertEqual(code, 1)
        self.assertIn("Code changed", output)
        self.source.write_text("implementation\n", encoding="utf-8")
        self.command("git", "config", "branch.main.merge", "refs/heads/other")
        code, output = self.run_script()
        self.assertEqual(code, 1)
        self.assertIn("Push target changed", output)
        self.assertEqual(self.remote_head(), self.base_head)

    def test_remote_descendant_and_unconfirmed_remote_handling(self) -> None:
        self.source.write_text("implementation\n", encoding="utf-8")
        with patch.object(push, "remote_contains", return_value=False):
            code, output = self.run_script()
        self.assertEqual(code, 1)
        self.assertIn("Remote has not confirmed", output)
        self.assertEqual(push.load_push_record("recovery")["status"], "pending")

        head = push.current_head()
        other = self.root.parent / "other"
        self.command("git", "clone", "--branch", "main", str(self.remote), str(other))
        for key, value in (("user.name", "Other"), ("user.email", "other@example.invalid")):
            self.command("git", "-C", str(other), "config", key, value)
        (other / "later.txt").write_text("later\n", encoding="utf-8")
        self.command("git", "-C", str(other), "add", ".")
        self.command("git", "-C", str(other), "commit", "-m", "later remote commit")
        self.command("git", "-C", str(other), "push")
        remote_head = self.remote_head()

        code, output = self.run_script()
        self.assertEqual(code, 0, output)
        self.assertEqual((push.current_head(), self.remote_head()), (head, remote_head))

    def test_invalid_record_preserved_and_commit_hook_change_blocked(self) -> None:
        path = push.push_record_path("corrupt")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("{broken", encoding="utf-8")
        self.assertEqual(self.run_script(session="corrupt")[0], 1)
        self.assertEqual(path.read_text(encoding="utf-8"), "{broken")

        self.source.write_text("implementation\n", encoding="utf-8")
        original_git = push.git

        def hook(*args: str, capture: bool = False):
            if args[0] == "commit":
                self.source.write_text("hook change\n", encoding="utf-8")
                original_git("add", "sample.txt")
            return original_git(*args, capture=capture)

        with patch.object(push, "git", side_effect=hook):
            code, output = self.run_script()
        self.assertEqual(code, 1)
        self.assertIn("Commit hooks changed", output)
        self.assertEqual(self.remote_head(), self.base_head)


class HeraldAreaTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory(prefix="herald-push-area-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name) / "work"
        self.root.mkdir()
        self.command("git", "init", "-b", "main")
        self.command("git", "config", "user.name", "Push Test")
        self.command("git", "config", "user.email", "push-test@example.invalid")
        (self.root / "base.txt").write_text("base\n", encoding="utf-8")
        self.command("git", "add", ".")
        self.command("git", "commit", "-m", "base")
        self.addCleanup(patch.stopall)
        patch.object(push, "REPO_ROOT", self.root).start()

    def command(self, *args: str) -> subprocess.CompletedProcess:
        result = subprocess.run(args, cwd=self.root, capture_output=True, text=True, encoding="utf-8")
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def write(self, relpath: str, content: str) -> None:
        target = self.root / relpath
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")

    def test_sdk_areas_resolve_to_their_real_directories(self) -> None:
        self.assertEqual(
            push.detect_areas(["sdk/node/src/client.ts", "sdk/web/src/a.ts", "sdk/rust/src/lib.rs", "README.md"]),
            {"sdk-node", "sdk-web", "backend"},
        )
        self.assertEqual(push.area_pathspecs("sdk-node"), ["sdk/node/"])
        self.assertEqual(push.area_pathspecs("backend"), ["backend/"])

    def test_sdk_area_fingerprint_tracks_sdk_content(self) -> None:
        # The session cache keys a passed area by this fingerprint. With a
        # name-derived pathspec (`sdk-node/`) it covered no files, so a fix
        # applied after a failed CI run was skipped as "unchanged" and its
        # area was committed without ever being re-checked.
        self.write("sdk/node/src/client.ts", "before\n")
        untracked = push.area_fingerprint("sdk-node")
        self.write("sdk/node/src/client.ts", "after\n")
        self.assertNotEqual(push.area_fingerprint("sdk-node"), untracked)

        self.command("git", "add", ".")
        self.command("git", "commit", "-m", "sdk change")
        committed = push.area_fingerprint("sdk-node")
        self.write("sdk/node/src/client.ts", "after a fix\n")
        self.assertNotEqual(push.area_fingerprint("sdk-node"), committed)

    def test_area_steps_cover_the_sdk_packages_and_the_rust_crate(self) -> None:
        # The Rust SDK is a standalone crate outside backend/'s workspace, and
        # the JS SDK packages are separate npm packages: each needs its own
        # checks or a broken SDK ships while `backend/` and `frontend/` pass.
        self.write("backend/Cargo.toml", "[package]\n")
        self.write("sdk/rust/Cargo.toml", "[package]\n")
        self.write("sdk/node/package.json", json.dumps({"scripts": {"type-check": "tsc --noEmit", "build": "tsup"}}))

        self.assertEqual(
            [step.name for step in push.backend_steps()],
            [f"{label} {check}" for label in ("Backend", "Rust SDK") for check in ("clippy fix", "format", "clippy check")],
        )
        self.assertEqual(
            [(step.name, step.cwd) for step in push.sdk_node_steps()],
            [("SDK-NODE type check", self.root / "sdk" / "node"), ("SDK-NODE build", self.root / "sdk" / "node")],
        )


if __name__ == "__main__":
    unittest.main()
