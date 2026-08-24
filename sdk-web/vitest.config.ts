import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    // jsdom (not happy-dom): msw/node intercepts vitest's undici globalThis.fetch,
    // which happy-dom's own fetch would bypass. Matches the Herald frontend suite.
    environment: 'jsdom',
    globals: true,
    include: ['tests/**/*.test.ts'],
    setupFiles: ['./tests/setup.ts'],
  },
})
