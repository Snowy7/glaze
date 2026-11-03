import { defineConfig } from "vite";
import { devHostPlugin } from "./dev/server.ts";

// `pnpm dev`: the mock-host harness in ./dev. (The library/drop-in build is scripts/build.mjs.)
export default defineConfig({
  root: "dev",
  plugins: [devHostPlugin()],
  server: { fs: { allow: ["../../.."] } },
});
