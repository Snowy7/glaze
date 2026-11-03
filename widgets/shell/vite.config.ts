import { defineConfig } from "vite";

// The shell page. Served by the host at https://shell.mypaper/, so assets are relative.
// harden.js is built separately (vite.harden.config.ts) into the same dist/.
export default defineConfig({
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
    modulePreload: { polyfill: false },
  },
});
