// Builds dist/index.js (ESM) and dist/mypaper.js (the drop-in, a single minified IIFE).
// Type declarations come from `tsc -p tsconfig.build.json` (see the package.json build script).
import { fileURLToPath } from "node:url";
import { build } from "vite";

const root = fileURLToPath(new URL("..", import.meta.url));
/** @type {import("vite").InlineConfig & { build: object }} */
const common = { root, configFile: false, logLevel: "warn", build: { target: "chrome120" } };

await build({
  ...common,
  build: {
    ...common.build,
    emptyOutDir: false,
    minify: false,
    lib: { entry: "src/index.ts", formats: ["es"], fileName: () => "index.js" },
  },
});

await build({
  ...common,
  build: {
    ...common.build,
    emptyOutDir: false,
    minify: true,
    lib: {
      entry: "src/dropin.ts",
      formats: ["iife"],
      name: "mypaperDropin",
      fileName: () => "mypaper.js",
    },
  },
});

console.log("built dist/index.js and dist/mypaper.js");
