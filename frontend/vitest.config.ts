import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { tanstackRouter } from '@tanstack/router-plugin/vite'
import tailwindcss from '@tailwindcss/vite'
import { paraglideVitePlugin } from '@inlang/paraglide-js'
import path from 'path'

/**
 * Vitest Configuration
 *
 * This file configures Vitest for frontend testing with:
 * - JSDOM environment for fast, isolated testing
 * - MSW for API mocking
 * - React and TanStack Router plugins
 * - Tailwind CSS for styling
 */
export default defineConfig({
  plugins: [
    paraglideVitePlugin({
      project: './project.inlang',
      outdir: './src/paraglide',
      strategy: ['localStorage', 'baseLocale'],
      localStorageKey: 'herald-locale',
      emitTsDeclarations: true,
    }),
    tailwindcss(),
    tanstackRouter({
      target: 'react',
      autoCodeSplitting: true,
      routeFileIgnorePattern: '__tests__',
    }),
    react(),
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  test: {
    // JSDOM environment for fast, isolated testing
    environment: 'jsdom',

    // Ensure proper test isolation
    isolate: true,

    // Enable global test utilities
    globals: true,

    // Minimal reporter: only failed tests and errors, optimized for AI agents
    reporters: ['minimal'],

    // Keep enough per-test budget for the full suite under parallel JSDOM load.
    testTimeout: 15000,

    // Cap parallel workers below the logical-CPU count: a fully-subscribed
    // worker pool starves individual JSDOM workers on dev machines that also
    // run editors/dev servers, which scrambles userEvent typing order and
    // trips real-timer waits in otherwise-green form tests.
    maxWorkers: 8,

    // Don't fail tests on unhandled promise rejections (handled by try-catch in components)
    errorOnUnhandledRejections: false,

    // Filter out *expected* unhandled rejections from fire-and-forget mutations.
    // Some forms call `void mutation.mutate()` (where `mutate === mutateAsync`)
    // and rely on the UI / toast for error feedback rather than awaiting the
    // promise. On a backend failure that promise rejects with no `.catch()`,
    // which Vitest 4 reports as an "Unhandled Error" (the `errorOnUnhandledRejections`
    // flag above is a no-op in v4). These are intentional failure-path tests that
    // assert the UI outcome, so we filter the known expected messages and let
    // genuinely unexpected errors still fail the run.
    onUnhandledError(error) {
      const message =
        error && typeof error === 'object' && 'message' in error
          ? String((error as { message: unknown }).message)
          : String(error)
      if (message.includes('Passkey registration failed')) {
        // Expected: intended-to-reject passkey begin/finish mutation in error-path tests.
        return false
      }
    },

    // Ensure test files are correctly resolved
    include: ['**/__tests__/**/*.{test,spec}.{js,jsx,ts,tsx}'],
    exclude: [
      '**/node_modules/**',
      '**/dist/**',
      '**/tests/e2e/**',
      '**/demo/**',
      '**/.git/**',
      '**/.vscode/**',
    ],
    // Setup file for MSW and global utilities
    setupFiles: ['./src/test/setup.ts'],
    // Optimize dependency pre-bundling
    optimizeDeps: {
      include: ['class-variance-authority', 'clsx', 'tailwind-merge', 'react', 'react-dom'],
    },
    // Coverage configuration
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json', 'html'],
      exclude: [
        'node_modules/',
        'src/test/',
        '**/__tests__/',
        '*.config.{js,ts}',
        'src/main.tsx',
        'src/vite-env.d.ts',
        '**/*.d.ts',
      ],
    },
  },
})
