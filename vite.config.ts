import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// NOTES ON TREE-SHAKING & CHUNKING:
// The original project split node_modules into many small chunks
// (xlsx, syncfusion, prosemirror, codemirror, react-vendor, etc).
// In dev mode each chunk is loaded as its own ES module, but in
// production Rollup rewrites every cross-module identifier to a
// short letter (`a`, `b`, ...).  When two chunks re-use the same
// short name to refer to *different* bindings (because Rollup decided
// helper X belongs to chunk A but module Y is in chunk B and also
// uses helper X), the resulting `import { a as z } from "./A.js"`
// statement at the top of B resolves to `undefined` at runtime,
// and the page silently renders blank.
//
// The previous config reproduced this twice:
//   - `react-vendor`: React 19 bind names hoisted, but chunk still
//     tried to `import { r as hy, c as ry, ... } from "./vendor-..."`
//     for symbols that vendor never declared.
//   - `prosemirror`: same problem with `OrderedMap.empty`.
//
// Fix: stop fighting Rollup.  Drop the per-package manualChunks and
// let Rollup's automatic chunker produce a small number of files.
// We still get useful splitting via dynamic imports if the codebase
// already uses them; otherwise the vendor bundle stays monolithic.
// In exchange we get a build that runs without runtime undefined
// references in any chunk.

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],
  // Relative base so production builds emit `./assets/...` instead of
  // `/assets/...`. Tauri's release webview serves assets via the
  // `tauri://localhost/` custom protocol where absolute paths resolve to
  // the wrong root and the page renders as a blank (black) screen.
  base: './',

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
    },
  },
  build: {
    // Match Tauri side: emit sourcemaps for easier debugging in WebView2.
    sourcemap: false,
    // Larger chunks are fine — Tauri loads them locally. Disable the
    // default 500kB warning so we can keep one big vendor file.
    chunkSizeWarningLimit: 8000,
    rollupOptions: {
      output: {
        // Group Tauri-side helpers into their own chunk for cleanliness,
        // and everything else stays together so cross-imports stay in-file.
        manualChunks(id: string) {
          if (id.includes('/node_modules/@tauri-apps/')) {
            return 'tauri-vendor';
          }
          return undefined;
        },
      },
    },
  },
}));
