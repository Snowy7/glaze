import { defineConfig } from "vite";

// The bars page. Served by the host at https://bars.mypaper/index.html, so assets are relative.
// `public/` (the bundled icon packs) is copied into dist as is.
export default defineConfig({
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
    modulePreload: { polyfill: false },
  },
  // The dev mock reads the host's preset bundles (src/mock-config.ts).
  server: { fs: { allow: [".", "../../crates/bars/presets"] } },
});
