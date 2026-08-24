// Cross-platform OpenAPI generation entry for CI / npm scripts.
//
// 1. Export the OpenAPI spec from the Herald backend (same source the frontend
//    consumes) into ./api.json.
// 2. Generate the self-contained typed fetch client into ./src/generated.
//
// Run from the sdk/web package root: `npm run generate-api`.
import { execSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const here = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(here, '..')

function run(command) {
  execSync(command, {
    stdio: 'inherit',
    cwd: root,
    shell: process.platform === 'win32' ? 'cmd.exe' : undefined,
  })
}

run('cargo run --manifest-path ../backend/app/Cargo.toml --bin herald-app -- --export-openapi api.json')
run('npx --no-install openapi-ts')

console.log('Herald SDK: OpenAPI spec exported and client generated.')
