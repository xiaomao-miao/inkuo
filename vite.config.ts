import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// NOTES ON TREE-SHAKING & CHUNKING:
//
// Previous attempts at manual chunking (e.g. `react-vendor`, `prosemirror`)
// broke production builds: Rollup renamed cross-module helpers to single
// letters and ended up resolving an `import { a as z } from "./vendor-..."`
// to `undefined` when the helper lived in a different chunk than the call
// site. The fix that worked was to keep one large vendor bundle alongside
// small, dependency-free isolates for the heaviest optional chunks.
//
// We intentionally split ONLY chunks that are unambiguously self-contained:
//   - `tauri`: never re-exports from other vendors (only `@tauri-apps/*`)
//   - `office-editor` / `fortune-sheet` / `pdfjs` / `codemirror` /
//     `prosemirror` / `markdown` / `icons` — each is the only consumer of
//     its own internal helpers, so cross-chunk aliasing doesn't apply.
//
// This shrinks first paint noticeably (Office is ~3MB, pdfjs ~5MB, etc)
// without re-introducing the runtime-undefined bug.

const chunkSplit = (id: string): string | undefined => {
  if (id.includes('/node_modules/@tauri-apps/')) return 'tauri';
  if (id.includes('/node_modules/@eigenpal/')) return 'office-editor';
  if (id.includes('/node_modules/@fortune-sheet/')) return 'fortune-sheet';
  if (id.includes('/node_modules/pdfjs-dist/')) return 'pdfjs';
  if (id.includes('/node_modules/@codemirror/')) return 'codemirror';
  if (id.includes('/node_modules/prosemirror-')) return 'prosemirror';
  if (id.includes('/node_modules/react-markdown/')) return 'markdown-renderer';
  if (id.includes('/node_modules/lucide-react/')) return 'icons';
  return undefined;
};

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
    // We split the heaviest chunks into their own files (see manualChunks
    // below) so the main bundle stays well under 2MB. 1.5MB is enough
    // headroom for the remaining vendor code (React, Zustand, etc.).
    chunkSizeWarningLimit: 1500,
    target: 'es2022',
    cssCodeSplit: true,
    minify: 'esbuild',
    rollupOptions: {
      output: {
        manualChunks: chunkSplit,
        // Stable file naming so Tauri can fingerprint assets predictably.
        chunkFileNames: 'assets/[name]-[hash].js',
        entryFileNames: 'assets/[name]-[hash].js',
        assetFileNames: 'assets/[name]-[hash][extname]',
      },
    },
  },
}));
