import type { HttpHandler } from 'msw'

// No default handlers: each test registers the exact endpoints it exercises via
// `server.use(...)`. This keeps assertions local and avoids cross-test leakage.
export const handlers: HttpHandler[] = []
