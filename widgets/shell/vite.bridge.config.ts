import { defineConfig } from "vite";

// dist/bridge.js: a self-contained classic script (IIFE), injected by the host into every frame.
export default defineConfig({
  build: {
    outDir: "dist",
    emptyOutDir: false,
    target: "es2022",
    copyPublicDir: false,
    lib: {
      entry: "src/bridge.ts",
      formats: ["iife"],
      name: "mypaperBridge",
      fileName: () => "bridge.js",
    },
  },
});
