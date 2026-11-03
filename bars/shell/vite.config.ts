import { defineConfig } from "vite";

// The bars page. Served by the host at https://bars.mypaper/index.html, so assets are relative.
export default defineConfig({
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
    modulePreload: { polyfill: false },
  },
});
