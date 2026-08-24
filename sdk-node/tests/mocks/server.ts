import { setupServer } from 'msw/node'

// Handlers are registered per-test via `server.use(...)`; there is no shared
// default handler set, so every test states exactly the endpoints it expects.
export const server = setupServer()
