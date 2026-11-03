import { defineConfig } from "vite";

// dist/harden.js: a self-contained classic script (IIFE), injected by the host into every frame.
export default defineConfig({
  build: {
    outDir: "dist",
    emptyOutDir: false,
    target: "es2022",
    copyPublicDir: false,
    lib: {
      entry: "src/harden.ts",
      formats: ["iife"],
      name: "mypaperHarden",
      fileName: () => "harden.js",
    },
  },
});
