#!/usr/bin/env python
import platform
import subprocess
import sys
import time
from pathlib import Path

from lib import docker
from lib.net import is_port_open
from lib.paths import REPO_ROOT


def _ports_free() -> bool:
    ports = [15433, 6382, 16432, 13890, 13636]
    occupied = [port for port in ports if is_port_open("127.0.0.1", port)]
    if not occupied:
        return True
    print("ERROR: Occupied test ports:", ", ".join(str(p) for p in occupied))
    return False


# Real OpenLDAP directory for the herald-infra LDAP integration tests
# (backend/infra/tests/ldap_directory.rs). Pinned by digest: "2.6.10-alpha" is
# the only current release of the v2 line and we must not drift silently.
# Contract verified against this digest: internal ports 3890/6360, cert files
# cert.crt/cert.key/ca.crt, rootDN password via SSHA env, data/custom LDIF seed.
LDAP_IMAGE = "osixia/openldap@sha256:80a577d7d4471c4db662195111e5709665b6fcbd6679094d05402b8c620e2607"
LDAP_ASSETS = REPO_ROOT / "backend" / "infra" / "tests" / "ldap-directory-assets"


def _start_ldap() -> bool:
    """Start the OpenLDAP test container (StartTLS on 13890, LDAPS on 13636).

    The TLS key pair and CA are committed fixtures; the integration tests
    trust the same CA via the realm setting caCertPem. The rootDN password
    matches the SSHA hash below.
    """
    print("Starting OpenLDAP test container...")
    certs_dir = LDAP_ASSETS / "certs"
    seed_ldif = LDAP_ASSETS / "seed.ldif"
    if not certs_dir.is_dir() or not seed_ldif.is_file():
        print("ERROR: LDAP test fixtures missing under", LDAP_ASSETS)
        return False

    if not docker.run_detached(
        [
            "--name",
            "cas-test-ldap",
            "--memory=256m",
            "--restart=unless-stopped",
            "--log-opt",
            "max-size=10m",
            "--log-opt",
            "max-file=3",
            "-e",
            "OPENLDAP_BOOTSTRAP_SUFFIX=dc=herald,dc=test",
            "-e",
            "OPENLDAP_BOOTSTRAP_TLS=true",
            # SSHA hash of "svc-password" (slappasswd), fixed so the tests
            # can bind without parsing generated passwords from the logs.
            "-e",
            "OPENLDAP_BOOTSTRAP_DATA_ROOT_PASSWORD_HASHED={SSHA}/oGEfntBnpHAZEkLsEDHKBZGVD65KQQv",
            "-p",
            "13890:3890",
            "-p",
            "13636:6360",
            "-v",
            f"{certs_dir.as_posix()}:/container/services/openldap/assets/certs:ro",
            "-v",
            f"{seed_ldif.as_posix()}:/container/services/openldap-bootstrap/assets/ldif/data/custom/10-seed.ldif:ro",
            LDAP_IMAGE,
        ]
    ):
        print("ERROR: OpenLDAP test container failed to start")
        return False

    # Wait until an authenticated StartTLS search returns a seeded entry.
    probe = (
        "LDAPTLS_CACERT=/container/services/openldap/assets/certs/ca.crt "
        "ldapsearch -x -ZZ -H ldap://127.0.0.1:3890 "
        "-D cn=admin,dc=herald,dc=test -w svc-password "
        "-b dc=herald,dc=test '(uid=alice)' dn"
    )
    for _attempt in range(30):
        code, out = docker.exec_check("cas-test-ldap", ["sh", "-c", probe])
        if code == 0 and "uid=alice,ou=people" in out:
            print("OpenLDAP test container is ready")
            return True
        time.sleep(1)

    print("ERROR: OpenLDAP test container failed to start")
    logs = subprocess.run(
        ["docker", "logs", "cas-test-ldap", "--tail", "30"],
        capture_output=True,
        text=True,
    )
    log_output = (logs.stdout or logs.stderr).strip()
    if log_output:
        print(log_output)
    return False


def _host_gateway_args() -> list[str]:
    if platform.system() == "Linux":
        return ["--add-host", "host.docker.internal:host-gateway"]
    return []


def _build_pgdog_bootstrap_command(pgdog_config: str, users_config: str) -> str:
    return f"""cat > /tmp/pgdog.toml <<'PGDOG_CONFIG'
{pgdog_config}
PGDOG_CONFIG
cat > /tmp/users.toml <<'USERS_CONFIG'
{users_config}
USERS_CONFIG
exec /usr/local/bin/pgdog -c /tmp/pgdog.toml -u /tmp/users.toml run
"""


def _print_pgdog_failure_diagnostics(last_probe_output: str) -> None:
    if last_probe_output:
        print(f"PgDog last probe output: {last_probe_output}")

    inspect = subprocess.run(
        [
            "docker",
            "inspect",
            "cas-test-pgdog",
            "--format",
            "{{json .State}} {{json .Mounts}}",
        ],
        capture_output=True,
        text=True,
    )
    if inspect.returncode == 0 and inspect.stdout.strip():
        print(f"PgDog inspect: {inspect.stdout.strip()}")

    logs = subprocess.run(
        ["docker", "logs", "cas-test-pgdog", "--tail", "50"],
        capture_output=True,
        text=True,
    )
    log_output = (logs.stdout or logs.stderr).strip()
    if log_output:
        print("PgDog logs:")
        print(log_output)


def _start_pgdog() -> bool:
    """Start PgDog proxy container.

    Returns:
        True if PgDog started successfully, False otherwise.
    """
    print("Starting PgDog proxy...")

    # Check if pgdog port is free
    if is_port_open("127.0.0.1", 16432):
        print("Port 16432 already in use, stopping existing PgDog...")
        if docker.container_running("cas-test-pgdog"):
            docker.stop_container("cas-test-pgdog")
        if docker.container_exists("cas-test-pgdog"):
            docker.rm_container("cas-test-pgdog")

    # Create PgDog configuration
    # Note: PgDog connects to PostgreSQL via localhost:15433 (host.docker.internal on Windows/Mac)
    #
    # Tuning rationale:
    # - pooler_mode = "transaction": releases server connections between transactions,
    #   allowing many test clients to share a smaller pool of server connections.
    # - pool_size = 64: accommodates 8 parallel tests × ~4 connections each + shared pool.
    # - checkout_timeout = 60000: generous timeout for CI environments with slow I/O.
    # - workers = 4: more event loops to handle concurrent test traffic.
    pgdog_config = """[general]
host = "0.0.0.0"
port = 6432
workers = 4
default_pool_size = 64
min_pool_size = 2
checkout_timeout = 60000
idle_timeout = 600000
healthcheck_timeout = 5000
healthcheck_interval = 10000

[[databases]]
name = "postgres"
host = "host.docker.internal"
port = 15433
database_name = "postgres"
user = "postgres"
password = "postgres"
pool_size = 64
min_pool_size = 2
"""

    # PgDog also requires users.toml for authentication
    users_config = """[[users]]
name = "postgres"
password = "postgres"
database = "postgres"
pooler_mode = "transaction"
pool_size = 64
min_pool_size = 2
"""

    # Create Docker network if it doesn't exist
    subprocess.run(
        ["docker", "network", "create", "cas-test-network"],
        capture_output=True,
    )

    bootstrap_cmd = _build_pgdog_bootstrap_command(pgdog_config, users_config)
    if not docker.run_detached(
        [
            "--name",
            "cas-test-pgdog",
            "--memory=512m",
            "--restart=unless-stopped",
            "--log-opt",
            "max-size=10m",
            "--log-opt",
            "max-file=3",
            "-e",
            "RUST_LOG=error",  # Reduce pgdog logging to errors only
            "-e",
            "RUST_BACKTRACE=0",  # Disable backtrace
            *_host_gateway_args(),
            "-p",
            "16432:6432",
            "--entrypoint",
            "sh",
            "ghcr.io/pgdogdev/pgdog:v0.1.35",
            "-lc",
            bootstrap_cmd,
        ]
    ):
        print("ERROR: PgDog container failed to start")
        return False

    # Wait for PgDog to accept authenticated SQL traffic, not just TCP connections.
    last_probe_output = ""
    for attempt in range(30):
        code, out = docker.exec_check(
            "cas-test-postgres",
            [
                "psql",
                "postgresql://postgres:postgres@host.docker.internal:16432/postgres?sslmode=disable",
                "-c",
                "select 1",
            ],
        )
        last_probe_output = out
        if code == 0 and "1" in out:
            print("PgDog is ready")
            return True
        time.sleep(1)

    print("ERROR: PgDog failed to start")
    _print_pgdog_failure_diagnostics(last_probe_output)
    return False


def main() -> int:
    stop_result = subprocess.run([sys.executable, str(REPO_ROOT / "scripts" / "test-stop.py")])
    if stop_result.returncode != 0:
        return stop_result.returncode

    if not _ports_free():
        return 1

    if docker.container_running("cas-test-postgres"):
        docker.stop_container("cas-test-postgres")
    if docker.container_exists("cas-test-postgres"):
        docker.rm_container("cas-test-postgres")

    # Note: Not using custom Docker network on Windows for compatibility
    # Containers will communicate via localhost port mappings instead

    if not docker.run_detached(
        [
            "--name",
            "cas-test-postgres",
            "--memory=1g",
            "--shm-size=512m",
            "--restart=unless-stopped",
            "--log-opt",
            "max-size=10m",
            "--log-opt",
            "max-file=3",
            "-e",
            "POSTGRES_USER=postgres",
            "-e",
            "POSTGRES_PASSWORD=postgres",
            "-e",
            "POSTGRES_DB=postgres",
            *_host_gateway_args(),
            "-p",
            "15433:5432",
            "postgres:18-alpine",
        ]
    ):
        print("ERROR: PostgreSQL test container failed to start")
        return 1

    if not docker.wait_pg_ready("cas-test-postgres", "postgres"):
        print("ERROR: PostgreSQL test container failed to start")
        return 1

    # 清理遗留的测试 Schema
    print("Cleaning up leftover test schemas...")
    cleanup_cmd = [
        "docker", "exec", "cas-test-postgres",
        "psql",
        "-U", "postgres",
        "-h", "localhost",
        "-d", "postgres",
        "-c",
        """DO $$
DECLARE
    schema_record RECORD;
BEGIN
    FOR schema_record IN
        SELECT schema_name FROM information_schema.schemata
        WHERE schema_name LIKE 'test_%' OR schema_name LIKE 'template_test_schema_%'
    LOOP
        EXECUTE 'DROP SCHEMA IF EXISTS "' || schema_record.schema_name || '" CASCADE';
        RAISE NOTICE 'Dropped schema: %', schema_record.schema_name;
    END LOOP;
END $$;"""
    ]
    result = subprocess.run(cleanup_cmd, capture_output=True, text=True)
    if result.returncode == 0:
        print("[OK] Test schema cleanup completed")
    else:
        print(f"[WARN] Schema cleanup had issues: {result.stderr}")

    if docker.container_running("cas-test-redis"):
        docker.stop_container("cas-test-redis")
    if docker.container_exists("cas-test-redis"):
        docker.rm_container("cas-test-redis")

    if not docker.run_detached(
        [
            "--name",
            "cas-test-redis",
            "--memory=256m",
            "--restart=unless-stopped",
            "--log-opt",
            "max-size=10m",
            "--log-opt",
            "max-file=3",
            "-p",
            "6382:6379",
            "redis:8.4-alpine",
        ]
    ):
        print("ERROR: Redis test container failed to start")
        return 1

    if not docker.wait_redis_ready("cas-test-redis"):
        print("ERROR: Redis test container failed to start")
        return 1

    # Start OpenLDAP for herald-infra LDAP integration tests
    if docker.container_running("cas-test-ldap"):
        docker.stop_container("cas-test-ldap")
    if docker.container_exists("cas-test-ldap"):
        docker.rm_container("cas-test-ldap")

    if not _start_ldap():
        return 1

    # Start PgDog proxy
    if not _start_pgdog():
        return 1

    if not docker.wait_redis_ready("cas-test-redis"):
        print("ERROR: Redis test container failed to start")
        return 1

    # Verify PgDog connectivity with a real SQL round-trip.
    code, out = docker.exec_check(
        "cas-test-postgres",
        [
            "psql",
            "postgresql://postgres:postgres@host.docker.internal:16432/postgres?sslmode=disable",
            "-c",
            "select 1",
        ],
    )
    if code != 0 or "1" not in out:
        print("ERROR: PgDog verification failed")
        return 1

    print("Test environment is ready. PgDog=localhost:16432 Redis=localhost:6382 "
          "LDAP StartTLS=localhost:13890 LDAPS=localhost:13636")
    return 0


if __name__ == "__main__":
    sys.exit(main())
