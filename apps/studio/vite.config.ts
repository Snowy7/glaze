import { cpSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

/** The built bars page; `pnpm -C bars/shell build` makes it. */
const BARS_DIST = fileURLToPath(new URL("../../bars/shell/dist", import.meta.url));
const BARS_ROUTE = "bars-shell";
const SHIM = "studio-embed.js";

/**
 * Inside Studio's WebView2 a frame may see `chrome.webview`, which the bars page would take for its
 * desktop host and wait on forever. Hiding it makes the page use its own sample host, which is what
 * the live preview drives (bars/shell/src/mock.ts, `?embed`).
 */
const SHIM_SOURCE = "try { Object.defineProperty(window, 'chrome', { value: {}, configurable: true }); } catch {}\n";

/** The bars page's index.html, loading the shim before its own module. */
const page = () => readFileSync(join(BARS_DIST, "index.html"), "utf8").replace('<script type="module"', `<script src="./${SHIM}"></script>\n    <script type="module"`);

const TYPES: Record<string, string> = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".svg": "image/svg+xml", ".png": "image/png", ".woff2": "font/woff2" };

/**
 * Serves the real bars page at `/bars-shell/`, same-origin with Studio, for the live bar previews
 * (Studio reads its DOM to select and drag modules). Copied into the build; served from the
 * bars folder in dev. Without a built bars page, Studio draws bar silhouettes instead.
 */
function barsShell(): Plugin {
  return {
    name: "mypaper-bars-shell",
    configureServer(server) {
      server.middlewares.use(`/${BARS_ROUTE}`, (req, res, next) => {
        const route = decodeURIComponent((req.url ?? "/").split("?")[0] ?? "/");
        if (route === "/" || route === "/index.html") return void res.setHeader("content-type", TYPES[".html"] ?? "").end(page());
        if (route === `/${SHIM}`) return void res.setHeader("content-type", TYPES[".js"] ?? "").end(SHIM_SOURCE);
        const file = normalize(join(BARS_DIST, route));
        if (!file.startsWith(BARS_DIST) || !existsSync(file)) return next();
        res.setHeader("content-type", TYPES[extname(file)] ?? "application/octet-stream");
        res.end(readFileSync(file));
      });
    },
    writeBundle(options) {
      if (!existsSync(join(BARS_DIST, "index.html"))) {
        this.warn("bars/shell/dist isn't built (pnpm -C bars/shell build): Studio will draw bar silhouettes instead of the real bars.");
        return;
      }
      const target = join(options.dir ?? "dist", BARS_ROUTE);
      cpSync(BARS_DIST, target, { recursive: true });
      writeFileSync(join(target, "index.html"), page());
      writeFileSync(join(target, SHIM), SHIM_SOURCE);
    },
  };
}

// Tauri expects a fixed dev port (see src-tauri/tauri.conf.json `devUrl`).
export default defineConfig({
  plugins: [react(), tailwindcss(), barsShell()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: { target: "chrome120" },
});
