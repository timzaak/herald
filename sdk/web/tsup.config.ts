import { defineConfig } from 'tsup'

// Library build (design §5.6 / DEC-js-sdk-005, revised DEC-js-sdk-012).
//
// The SDK targets the browser. We ship:
//   1. ESM (`dist/index.js` + `.d.ts`) as the npm primary — modern bundlers
//      (Vite/webpack/Next/esbuild) and ESM CDNs (esm.sh) consume this.
//   2. A minified IIFE bundle (`dist/index.global.js`) exposing a `Herald`
//      global, for third-party `<script src="...">` integration with no build
//      step.
//
// CJS is intentionally NOT shipped: its only use case is Node `require()`, which
// is the separate server SDK's domain (browser/server are different solutions).
// All SDK state is per-instance, so there is no dual-package-instance hazard.
export default defineConfig([
  {
    entry: ['src/index.ts'],
    format: ['esm'],
    dts: true,
    clean: true,
    target: 'es2020',
    treeshake: true,
    sourcemap: true,
    external: [],
  },
  {
    entry: ['src/index.ts'],
    format: ['iife'],
    globalName: 'Herald',
    minify: true,
    target: 'es2020',
    sourcemap: true,
    external: [],
  },
])
