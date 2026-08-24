import { defineConfig } from '@hey-api/openapi-ts'

// Generate a self-contained typed fetch client from the Herald backend OpenAPI
// spec (design §5.6 / DEC-js-sdk-004). `client: 'fetch'` is explicit — we do NOT
// inherit the stale `client: 'axios'` field from the frontend config. The
// generated layer is an internal transport detail, not a public contract.
export default defineConfig({
  input: './api.json',
  output: {
    path: './src/generated',
  },
  services: {
    asClass: false,
    name: '{{name}}',
    include: 'responses|requests|all',
    operationId: true,
    response: 'body',
  },
  client: 'fetch',
})
