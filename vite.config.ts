import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

const VENDOR_CHUNKS: Array<[string, string]> = [
  ["node_modules/xlsx", "xlsx"],
  ["node_modules/@syncfusion", "syncfusion"],
  ["node_modules/@eigenpal", "office-editor"],
  ["node_modules/prosemirror", "prosemirror"],
  ["node_modules/@codemirror", "codemirror"],
  ["node_modules/@uiw", "codemirror-ui"],
  ["node_modules/react-dom", "react-vendor"],
  ["node_modules/react", "react-vendor"],
  ["node_modules/lucide-react", "icon-vendor"],
  ["node_modules/marked", "markdown-vendor"],
  ["node_modules/react-markdown", "markdown-vendor"],
  ["node_modules/remark-gfm", "markdown-vendor"],
  ["node_modules/rehype-highlight", "markdown-vendor"],
  ["node_modules/rehype-raw", "markdown-vendor"],
  ["node_modules/diff", "diff-vendor"],
  ["node_modules/zustand", "state-vendor"],
  ["node_modules/@tauri-apps", "tauri-vendor"],
];

function getManualChunk(id: string) {
  for (const [matcher, chunkName] of VENDOR_CHUNKS) {
    if (id.indexOf(matcher) !== -1) {
      return chunkName;
    }
  }

  if (id.indexOf("node_modules") !== -1) {
    return "vendor";
  }

  return undefined;
}

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

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
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks: getManualChunk,
      },
    },
  },
}));
