#!/usr/bin/env python
import argparse
import sys
import time
from pathlib import Path

from lib import docker
from lib import ngrok
from lib.proc import kill_process_by_port, kill_processes_by_path
from lib.paths import LOG_DIR, REPO_ROOT


def should_print(quiet: bool) -> bool:
    """Helper function to determine if we should print based on quiet mode."""
    return not quiet


BACKEND_PORT = 8080
LOG_DELETE_RETRIES = 5
LOG_DELETE_RETRY_INTERVAL_SECONDS = 0.3
DEMO_LOG_FILES = [
    LOG_DIR / "backend-demo.log.out",
    LOG_DIR / "backend-demo.log.err",
    LOG_DIR / "frontend-demo.log.out",
    LOG_DIR / "frontend-demo.log.err",
]
DEMO_RUNTIME_FILES: list[Path] = []

verbose = False


def log_verbose(message: str) -> None:
    """Print message only in verbose mode."""
    if verbose:
        print(message)


def _delete_with_retry(path: Path) -> bool:
    """Delete file with retry for lock-contention scenarios."""
    for _ in range(LOG_DELETE_RETRIES):
        try:
            if not path.exists():
                return True
            log_verbose(f"Deleting file: {path}")
            path.unlink()
            return True
        except FileNotFoundError:
            return True
        except (PermissionError, OSError):
            time.sleep(LOG_DELETE_RETRY_INTERVAL_SECONDS)
    return not path.exists()


def cleanup_demo_files() -> tuple[list[Path], list[Path]]:
    """Delete demo runtime/log files and return (failed_runtime, failed_logs)."""
    failed_runtime: list[Path] = []
    failed_logs: list[Path] = []

    for path in DEMO_RUNTIME_FILES:
        if not _delete_with_retry(path):
            failed_runtime.append(path)

    for path in DEMO_LOG_FILES:
        if not _delete_with_retry(path):
            failed_logs.append(path)

    return failed_runtime, failed_logs


def main() -> int:
    parser = argparse.ArgumentParser(description="Stop the demo environment")
    parser.add_argument("--quiet", action="store_true", help="Suppress all output")
    parser.add_argument("--verbose", action="store_true", help="Enable verbose logging")
    parser.add_argument(
        "--keep-ngrok-image",
        action="store_true",
        help="Keep the ngrok Docker image after removing the container (default: remove it)",
    )
    args = parser.parse_args()
    quiet = args.quiet
    global verbose
    verbose = args.verbose

    if should_print(quiet):
        print("Stopping backend...")
    log_verbose(f"Checking port {BACKEND_PORT} for backend process...")
    kill_process_by_port(BACKEND_PORT)
    log_verbose(f"Backend process on port {BACKEND_PORT} killed (if present)")

    if should_print(quiet):
        print("Stopping frontend...")
    frontend_dir = REPO_ROOT / "frontend"
    killed = kill_processes_by_path(frontend_dir)
    log_verbose(f"Killed {killed} frontend process tree(s) under {frontend_dir}")

    if should_print(quiet):
        print("Stopping ngrok tunnel...")
    ngrok.stop()

    # Asymmetric with postgres/redis (whose images are kept) — intentional, per request.
    if not args.keep_ngrok_image:
        if should_print(quiet):
            print("Removing ngrok image...")
        ngrok.remove_image()

    if should_print(quiet):
        print("Stopping containers...")
    containers_to_stop = ["cas-demo-redis", "cas-demo-postgres", "cas-demo-ldap"]
    for container in containers_to_stop:
        if docker.container_exists(container):
            log_verbose(f"Stopping container: {container}")
            docker.stop_container(container)
        else:
            log_verbose(f"Container not found (already stopped): {container}")
    time.sleep(1.0)
    for container in containers_to_stop:
        if docker.container_exists(container):
            log_verbose(f"Removing container: {container}")
            docker.rm_force_container(container)

    failed_logs = cleanup_demo_files()[1]
    if failed_logs and should_print(quiet):
        for failed in failed_logs:
            print(f"WARN: Failed to delete log file (can be cleaned later): {failed}")

    if should_print(quiet):
        print("Demo stopped")
    return 0


if __name__ == "__main__":
    sys.exit(main())
