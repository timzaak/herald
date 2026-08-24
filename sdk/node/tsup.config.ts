import { defineConfig } from 'tsup'

// Library build for the Node server SDK (`herald-sdk`).
//
// Unlike the browser SDK (`sdk/web` / herald-auth-web, ESM + IIFE only), this
// package ships ESM **and** CJS: Node `require()` interop is exactly the server
// SDK's domain (see DEC-js-sdk-012's browser/server split). All state is
// per-client-instance, so there is no dual-package-instance hazard.
export default defineConfig({
  entry: ['src/index.ts'],
  format: ['esm', 'cjs'],
  dts: true,
  clean: true,
  target: 'node18',
  treeshake: true,
  sourcemap: true,
  external: [],
})
