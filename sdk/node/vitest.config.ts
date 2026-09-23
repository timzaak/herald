import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    // Node environment: this SDK targets Node servers (native fetch, no DOM).
    // msw/node intercepts undici's globalThis.fetch.
    environment: 'node',
    globals: true,
    include: ['tests/**/*.test.ts'],
    setupFiles: ['./tests/setup.ts'],
    // Agent reporter (alias of 'minimal'): only failed tests and errors — explicit,
    // so AI-friendly output does not depend on agent environment detection.
    reporters: ['agent'],
  },
})
