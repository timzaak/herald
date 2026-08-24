import { afterAll, afterEach, beforeAll } from 'vitest'
import { server } from './mocks/server'

// MSW lifecycle: start once, reset handlers (and let per-test cleanup clear
// storage), stop once. Unhandled requests error loudly so missing handlers
// don't silently pass.
beforeAll(() => server.listen({ onUnhandledRequest: 'error' }))
afterEach(() => {
  server.resetHandlers()
  localStorage.clear()
})
afterAll(() => server.close())
