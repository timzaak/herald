"""
Demo 环境管理模块。

提供启动、停止和检查 Demo 测试环境健康状态的功能。
"""

import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

from . import docker
from .cli import require_executable
from .net import wait_for_http_ok, wait_for_tcp, wait_for_tcp_with_compile_awareness
from .paths import LOG_DIR, REPO_ROOT, ensure_dir
from .demo_seed import ensure_demo_seed_data
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .logger import Logger


@dataclass
class HealthStatus:
    """健康检查结果。"""

    healthy: bool = False
    services: dict[str, str] = field(default_factory=dict)
    errors: list[str] = field(default_factory=list)

    def add_service(self, name: str, status: str) -> None:
        """添加服务状态。"""
        self.services[name] = status

    def add_error(self, error: str) -> None:
        """添加错误信息。"""
        self.errors.append(error)

# Demo 环境端口和容器名称
BACKEND_PORT = 8080
FRONTEND_PORT = 3000
POSTGRES_CONTAINER = "cas-demo-postgres"
REDIS_CONTAINER = "cas-demo-redis"
# Removed state file names: BACKEND_STATE_NAME, FRONTEND_STATE_NAME

# 与测试环境 (scripts/test-start.py) 相同的 OpenLDAP 目录：镜像按 digest 固定，
# 证书与种子数据复用 backend/infra/tests/ldap-directory-assets 固件。
# Demo 沿用原生端口直映射（同 postgres/redis），与测试环境的 13890/13636 不冲突。
LDAP_CONTAINER = "cas-demo-ldap"
LDAP_IMAGE = "osixia/openldap@sha256:80a577d7d4471c4db662195111e5709665b6fcbd6679094d05402b8c620e2607"
LDAP_ASSETS = REPO_ROOT / "backend" / "infra" / "tests" / "ldap-directory-assets"
LDAP_STARTTLS_PORT = 3890
LDAP_LDAPS_PORT = 6360

DEFAULT_ADMIN_REALM = "admin"
DEFAULT_ADMIN_EMAIL = "admin@cas.com"
DEFAULT_ADMIN_PASSWORD = "password"
DEFAULT_ADMIN_CLIENT_ID = "admin-web-console"


def check_postgres_container(status: HealthStatus) -> bool:
    """检查 PostgreSQL 容器状态。"""
    if not docker.container_exists(POSTGRES_CONTAINER):
        status.add_error(f"PostgreSQL container '{POSTGRES_CONTAINER}' not found")
        status.add_service("postgres", "not found")
        return False

    if not docker.container_running(POSTGRES_CONTAINER):
        status.add_error(f"PostgreSQL container '{POSTGRES_CONTAINER}' not running")
        status.add_service("postgres", "stopped")
        return False

    code, _ = docker.exec_check(POSTGRES_CONTAINER, ["pg_isready", "-U", "postgres"])
    if code != 0:
        status.add_error(f"PostgreSQL not ready (pg_isready failed)")
        status.add_service("postgres", "not ready")
        return False

    status.add_service("postgres", "healthy")
    return True


def check_redis_container(status: HealthStatus) -> bool:
    """检查 Redis 容器状态。"""
    if not docker.container_exists(REDIS_CONTAINER):
        status.add_error(f"Redis container '{REDIS_CONTAINER}' not found")
        status.add_service("redis", "not found")
        return False

    if not docker.container_running(REDIS_CONTAINER):
        status.add_error(f"Redis container '{REDIS_CONTAINER}' not running")
        status.add_service("redis", "stopped")
        return False

    code, out = docker.exec_check(REDIS_CONTAINER, ["redis-cli", "ping"])
    if code != 0 or out != "PONG":
        status.add_error(f"Redis not ready (ping failed: {out})")
        status.add_service("redis", "not ready")
        return False

    status.add_service("redis", "healthy")
    return True


def _ldap_ready() -> bool:
    """通过认证的 StartTLS 搜索验证 LDAP 目录就绪（容器内探测，端口为容器内部端口）。"""
    probe = (
        "LDAPTLS_CACERT=/container/services/openldap/assets/certs/ca.crt "
        "ldapsearch -x -ZZ -H ldap://127.0.0.1:3890 "
        "-D cn=admin,dc=herald,dc=test -w svc-password "
        "-b dc=herald,dc=test '(uid=alice)' dn"
    )
    code, out = docker.exec_check(LDAP_CONTAINER, ["sh", "-c", probe])
    return code == 0 and "uid=alice,ou=people" in out


def _start_ldap(logger: "Logger") -> bool:
    """启动 OpenLDAP Demo 容器（StartTLS 在 3890，LDAPS 在 6360）。"""
    logger.info("Starting OpenLDAP demo container...")
    certs_dir = LDAP_ASSETS / "certs"
    seed_ldif = LDAP_ASSETS / "seed.ldif"
    if not certs_dir.is_dir() or not seed_ldif.is_file():
        logger.error(f"LDAP fixtures missing under {LDAP_ASSETS}")
        return False

    if not docker.run_detached(
        [
            "--name",
            LDAP_CONTAINER,
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
            "-e",
            "OPENLDAP_BOOTSTRAP_DATA_ROOT_PASSWORD_HASHED={SSHA}/oGEfntBnpHAZEkLsEDHKBZGVD65KQQv",
            "-p",
            f"{LDAP_STARTTLS_PORT}:3890",
            "-p",
            f"{LDAP_LDAPS_PORT}:6360",
            "-v",
            f"{certs_dir.as_posix()}:/container/services/openldap/assets/certs:ro",
            "-v",
            f"{seed_ldif.as_posix()}:/container/services/openldap-bootstrap/assets/ldif/data/custom/10-seed.ldif:ro",
            LDAP_IMAGE,
        ]
    ):
        logger.error("OpenLDAP demo container failed to start")
        return False

    for _attempt in range(30):
        if _ldap_ready():
            logger.info("OpenLDAP demo container is ready")
            return True
        time.sleep(1)

    logger.error("OpenLDAP demo container failed to start")
    logs = subprocess.run(
        ["docker", "logs", LDAP_CONTAINER, "--tail", "30"],
        capture_output=True,
        text=True,
    )
    log_output = (logs.stdout or logs.stderr).strip()
    if log_output:
        logger.error(log_output)
    return False


def check_ldap_container(status: HealthStatus) -> bool:
    """检查 LDAP 容器状态。"""
    if not docker.container_exists(LDAP_CONTAINER):
        status.add_error(f"LDAP container '{LDAP_CONTAINER}' not found")
        status.add_service("ldap", "not found")
        return False

    if not docker.container_running(LDAP_CONTAINER):
        status.add_error(f"LDAP container '{LDAP_CONTAINER}' not running")
        status.add_service("ldap", "stopped")
        return False

    if not _ldap_ready():
        status.add_error("LDAP not ready (authenticated StartTLS search failed)")
        status.add_service("ldap", "not ready")
        return False

    status.add_service("ldap", "healthy")
    return True


def check_backend_process(status: HealthStatus) -> bool:
    """检查后端进程状态。"""
    # Skip state file check - check port and health endpoint directly
    # 检查端口是否可访问
    if not wait_for_tcp("127.0.0.1", BACKEND_PORT, 1):
        status.add_error(f"Backend port {BACKEND_PORT} not accessible")
        status.add_service("backend", "port not accessible")
        return False

    # 检查健康端点
    if not wait_for_http_ok(f"http://localhost:{BACKEND_PORT}/health", 2):
        status.add_error("Backend health check failed")
        status.add_service("backend", "health check failed")
        return False

    status.add_service("backend", "healthy")
    return True


def check_frontend_process(status: HealthStatus) -> bool:
    """检查前端进程状态（可选，不阻塞）。"""
    # Skip state file check - check port directly
    if not wait_for_http_ok(f"http://localhost:{FRONTEND_PORT}", 2):
        status.add_error("Frontend not ready")
        status.add_service("frontend", "not ready")
        return False

    status.add_service("frontend", "healthy")
    return True


def check_environment_health(require_frontend: bool = False) -> HealthStatus:
    """检查 Demo 环境是否运行且健康。

    Args:
        require_frontend: 是否要求前端必须健康（默认为 False，因为前端不是必需的）

    Returns:
        HealthStatus 对象，包含健康状态和服务详情
    """
    status = HealthStatus()

    # 检查 PostgreSQL
    pg_ok = check_postgres_container(status)

    # 检查 Redis
    redis_ok = check_redis_container(status)

    # 检查 LDAP
    ldap_ok = check_ldap_container(status)

    # 检查后端
    backend_ok = check_backend_process(status)

    # 检查前端（可选）
    if require_frontend:
        frontend_ok = check_frontend_process(status)
    else:
        # 不需要前端，直接跳过检查（避免不必要的网络等待）
        frontend_ok = True

    # 更新环境状态
    if pg_ok and redis_ok and ldap_ok and backend_ok:
        status.healthy = True

    # Skip environment state file updating - it may be inaccurate
    return status


def check_default_admin_credentials() -> bool:
    """Validate that the reused Demo stack still matches the expected default admin state."""
    login_url = f"http://127.0.0.1:{BACKEND_PORT}/api/auth/{DEFAULT_ADMIN_REALM}/login"
    payload = json.dumps(
        {
            "clientId": DEFAULT_ADMIN_CLIENT_ID,
            "email": DEFAULT_ADMIN_EMAIL,
            "password": DEFAULT_ADMIN_PASSWORD,
        }
    ).encode("utf-8")
    request = urllib.request.Request(
        login_url,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )

    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    try:
        with opener.open(request, timeout=10) as response:
            if response.status != 200:
                return False
            body = json.loads(response.read().decode("utf-8"))
            return body.get("requiresTotp") is not True
    except (TimeoutError, urllib.error.URLError, urllib.error.HTTPError, json.JSONDecodeError):
        return False


def start_environment(
    logger: "Logger",
    timeout: int = 60,
) -> bool:
    """启动 Demo 环境并验证健康状态。

    Args:
        logger: Logger 实例（用于详细日志和性能分析）
        timeout: 启动超时时间（秒）

    Returns:
        启动成功返回 True，否则返回 False
    """
    logger.info("Starting Demo environment...")

    # Skip environment state file tracking - it may be inaccurate
    total_steps = 7

    # Step 1: Stop old environment (if running)
    with logger.step(1, total_steps, "Stopping old environment"):
        logger.verbose_info("Executing demo-stop.py script...")
        try:
            stop_result = subprocess.run(
                [sys.executable, str(REPO_ROOT / "scripts" / "demo-stop.py"), "--quiet"],
                capture_output=True,
                timeout=120,  # demo-stop's docker CLI calls can exceed 30s on Windows
            )
            logger.verbose_info(f"demo-stop.py completed with exit code: {stop_result.returncode}")
            if stop_result.stdout:
                logger.verbose_info(f"Stop stdout: {stop_result.stdout.decode('utf-8', errors='replace')}")
            if stop_result.stderr:
                logger.verbose_info(f"Stop stderr: {stop_result.stderr.decode('utf-8', errors='replace')}")

            if stop_result.returncode != 0:
                logger.error(f"Failed to stop old environment (exit code: {stop_result.returncode})")
                if stop_result.stderr:
                    logger.error(f"Stop error output: {stop_result.stderr.decode('utf-8', errors='replace')}")
                return False
        except subprocess.TimeoutExpired:
            logger.error("Old environment stop timed out after 30 seconds")
            return False
        except Exception as e:
            logger.error(f"Failed to stop old environment: {e}")
            return False

    # Step 2: Start PostgreSQL container
    with logger.step(2, total_steps, "Starting PostgreSQL"):
        if not docker.run_detached(
            [
                "--name",
                POSTGRES_CONTAINER,
                "--memory=1g",
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
                "POSTGRES_DB=herald_demo",
                "-p",
                "5432:5432",
                "postgres:18-alpine",
            ]
        ):
            logger.error("PostgreSQL container start failed")
            return False

        if not docker.wait_pg_ready(POSTGRES_CONTAINER, "postgres", logger=logger):
            logger.error("PostgreSQL failed to start")
            return False

    # Step 3: Start Redis container
    with logger.step(3, total_steps, "Starting Redis"):
        if not docker.run_detached(
            [
                "--name",
                REDIS_CONTAINER,
                "--memory=256m",
                "--restart=unless-stopped",
                "--log-opt",
                "max-size=10m",
                "--log-opt",
                "max-file=3",
                "-p",
                "6379:6379",
                "redis:8.4-alpine",
            ]
        ):
            logger.error("Redis container start failed")
            return False

        if not docker.wait_redis_ready(REDIS_CONTAINER, logger=logger):
            logger.error("Redis failed to start")
            return False

        # Wait for Redis to fully initialize
        time.sleep(5)

    # Step 4: Start OpenLDAP container
    with logger.step(4, total_steps, "Starting OpenLDAP"):
        if not _start_ldap(logger):
            return False

    # Prepare log paths
    backend_log_base = LOG_DIR / "backend-demo.log"
    frontend_log_base = LOG_DIR / "frontend-demo.log"
    backend_out = backend_log_base.with_suffix(".log.out")
    backend_err = backend_log_base.with_suffix(".log.err")
    frontend_out = frontend_log_base.with_suffix(".log.out")
    frontend_err = frontend_log_base.with_suffix(".log.err")

    cargo = require_executable("cargo")
    npm = require_executable("npm", windows_fallback="npm.cmd")

    # Step 5: Start backend process
    with logger.step(5, total_steps, "Starting backend"):
        backend_env = dict(os.environ)
        backend_env["HERALD_CONFIG"] = str((REPO_ROOT / "backend" / "config" / "demo.toml").resolve())
        backend_env["TOTP_SECRET_KEY"] = "demo-totp-encryption-key-32-bytes-long"
        backend_env["ADMIN_REALM_ID"] = "admin"
        backend_env["INTERNAL_API_KEY"] = "demo-internal-api-key"
        backend_env["TEST_API_TOKEN"] = "test-token-123"
        backend_env["RP_ID"] = "localhost"
        backend_env["RP_ORIGIN"] = "http://localhost:3000"

        from .proc import spawn_background

        spawn_background(
            name=None,
            command=[cargo, "run", "--bin", "herald-app"],
            cwd=REPO_ROOT / "backend",
            stdout_path=backend_out,
            stderr_path=backend_err,
            env=backend_env,
        )

        if not wait_for_tcp_with_compile_awareness("127.0.0.1", BACKEND_PORT, 300, logger=logger):
            logger.error(f"Backend start failed. Check {backend_out}")
            return False

        # Wait for backend to be fully healthy
        time.sleep(5)
        if not wait_for_http_ok(f"http://localhost:{BACKEND_PORT}/health", 30, logger=logger):
            logger.error(f"Backend health check failed. Check {backend_out}")
            return False

    # Step 6: Start frontend process
    with logger.step(6, total_steps, "Starting frontend"):
        spawn_background(
            name=None,
            command=[npm, "run", "dev"],
            cwd=REPO_ROOT / "frontend",
            stdout_path=frontend_out,
            stderr_path=frontend_err,
        )

        if not wait_for_http_ok(f"http://localhost:{FRONTEND_PORT}", timeout, logger=logger):
            logger.warning(f"Frontend not ready within {timeout} seconds")
            logger.verbose_info(f"Check logs: {frontend_out}, {frontend_err}")

    # Verify health
    health_status = check_environment_health(require_frontend=True)
    if not health_status.healthy:
        logger.error("Environment health check failed")
        for error in health_status.errors:
            logger.error(f"  - {error}")
        return False

    with logger.step(7, total_steps, "Seeding demo data"):
        if not ensure_demo_seed_data(logger):
            logger.error("Demo seed data setup failed")
            return False

    # Skip environment state file saving - it may be inaccurate
    logger.info("Demo Environment started "
                f"(LDAP StartTLS=localhost:{LDAP_STARTTLS_PORT} LDAPS=localhost:{LDAP_LDAPS_PORT})")
    return True


def stop_environment() -> bool:
    """优雅停止 Demo 环境。

    Returns:
        停止成功返回 True，否则返回 False
    """
    print("Stopping Demo environment...")

    # Skip environment state file clearing - it may be inaccurate

    # 停止后端进程 - use port instead of state file
    print("  Stopping backend...")
    # Kill processes by port 8080
    try:
        if sys.platform == "win32":
            # Windows
            result = subprocess.run(
                ["powershell", "-NoProfile", "-Command",
                 f"Get-NetTCPConnection -LocalPort {BACKEND_PORT} -State Listen -ErrorAction SilentlyContinue | "
                 "Select-Object -ExpandProperty OwningProcess | ForEach-Object { taskkill /PID $_ /F /T }"],
                capture_output=True,
                check=False,
                timeout=5,
            )
        else:
            # Unix-like
            for cmd in [["lsof", "-ti", f":{BACKEND_PORT}"], ["fuser", "-k", f"{BACKEND_PORT}/tcp"]]:
                try:
                    subprocess.run(cmd, capture_output=True, check=False)
                    break
                except FileNotFoundError:
                    continue
    except Exception:
        pass  # Ignore errors, best-effort cleanup

    # 停止前端进程 - use demo ports instead of state file
    print("  Stopping frontend...")
    for port in [3000, 3001, 3002, 3003]:
        try:
            if sys.platform == "win32":
                result = subprocess.run(
                    ["powershell", "-NoProfile", "-Command",
                     f"Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue | "
                     "Select-Object -ExpandProperty OwningProcess | ForEach-Object { taskkill /PID $_ /F /T }"],
                    capture_output=True,
                    check=False,
                    timeout=5,
                )
            else:
                for cmd in [["lsof", "-ti", f":{port}"], ["fuser", "-k", f"{port}/tcp"]]:
                    try:
                        subprocess.run(cmd, capture_output=True, check=False)
                        break
                    except FileNotFoundError:
                        continue
        except Exception:
            pass  # Ignore errors, best-effort cleanup

    # 停止 Docker 容器
    print("  Stopping containers...")
    if docker.container_exists(REDIS_CONTAINER):
        docker.stop_container(REDIS_CONTAINER)
    if docker.container_exists(POSTGRES_CONTAINER):
        docker.stop_container(POSTGRES_CONTAINER)
    if docker.container_exists(LDAP_CONTAINER):
        docker.stop_container(LDAP_CONTAINER)

    time.sleep(1.0)

    if docker.container_exists(REDIS_CONTAINER):
        docker.rm_force_container(REDIS_CONTAINER)
    if docker.container_exists(POSTGRES_CONTAINER):
        docker.rm_force_container(POSTGRES_CONTAINER)
    if docker.container_exists(LDAP_CONTAINER):
        docker.rm_force_container(LDAP_CONTAINER)

    print("Demo environment stopped")
    return True


