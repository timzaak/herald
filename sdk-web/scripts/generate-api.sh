#!/usr/bin/env bash
# Unix entry for OpenAPI generation (design §5.6). Mirrors generate-api.mjs.
# Run from the sdk-web package root: `./scripts/generate-api.sh`.
set -euo pipefail

cargo run --manifest-path ../backend/app/Cargo.toml --bin herald-app -- --export-openapi api.json
npx --no-install openapi-ts

echo "Herald SDK: OpenAPI spec exported and client generated."
