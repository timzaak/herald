#!/usr/bin/env python
import sys

from lib import docker


def main() -> int:
    if docker.container_running("cas-test-pgdog"):
        docker.stop_container("cas-test-pgdog")
    if docker.container_exists("cas-test-pgdog"):
        docker.rm_container("cas-test-pgdog")

    if docker.container_running("cas-test-postgres"):
        docker.stop_container("cas-test-postgres")
    if docker.container_exists("cas-test-postgres"):
        docker.rm_container("cas-test-postgres")

    if docker.container_running("cas-test-redis"):
        docker.stop_container("cas-test-redis")
    if docker.container_exists("cas-test-redis"):
        docker.rm_container("cas-test-redis")

    if docker.container_running("cas-test-ldap"):
        docker.stop_container("cas-test-ldap")
    if docker.container_exists("cas-test-ldap"):
        docker.rm_container("cas-test-ldap")

    print("Backend test environment stopped")
    return 0


if __name__ == "__main__":
    sys.exit(main())
