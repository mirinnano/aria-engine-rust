import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";

const edition = process.env.VITE_UMIKAZE_EDITION === "demo" ? "demo" : "full";
const editionModule = (name: "scene-assets" | "chapter-preview" | "stage-mapping") => fileURLToPath(
  new URL(`./src/${name}.${edition}.ts`, import.meta.url),
);
const projectRoot = fileURLToPath(new URL("../..", import.meta.url));

// `aria build --target web` copies this dist directory next to the bytecode,
// pak, wasm glue, and scene renderer. Relative paths keep the same package
// usable from a PWA host and from Tauri's bundled WebView.
export default defineConfig({
  base: "./",
  plugins: [react()],
  resolve: {
    alias: {
      "#scene-assets": editionModule("scene-assets"),
      "#chapter-preview": editionModule("chapter-preview"),
      "#stage-mapping": editionModule("stage-mapping"),
    },
  },
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    fs: {
      // Story-owned photographs live beside the native PAK source tree so
      // Web and Native resolve the same logical path during development.
      allow: [projectRoot],
    },
  },
  build: {
    outDir: process.env.ARIA_PRESENTATION_OUT_DIR || "dist",
    emptyOutDir: true,
    // Source maps are useful for local diagnosis but needlessly enlarge and
    // disclose a release build. CI/debug builds can opt in explicitly.
    sourcemap: process.env.ARIA_PRESENTATION_SOURCEMAP === "true",
  },
});
