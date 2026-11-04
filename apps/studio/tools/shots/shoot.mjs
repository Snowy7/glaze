// Screenshots of the built Studio frontend (apps/studio/dist) in headless Edge, with Tauri's IPC
// mocked by fixtures (fixtures.mjs, mock.js). No window is shown and nothing takes focus. The real
// widget pages run in the stage, served from the repo's widgets folder.
//
//   pnpm -C apps/studio build
//   node apps/studio/tools/shots/shoot.mjs <repo> <outDir> [sizes=1280x800,2560x1440] [only=desktop,settings] [scheme=dark]
//
// Steps are page-side JavaScript using the helpers below, or `key:<Key>[*n][+ctrl]` and `type:<text>`.
import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";

import { buildFixtures } from "./fixtures.mjs";

const [repo, outDir, ...rest] = process.argv.slice(2);
const opts = Object.fromEntries(rest.map((a) => a.split("=")));
const sizes = (opts.sizes ?? "1280x800,2560x1440").split(",").map((s) => s.split("x").map(Number));
const only = opts.only?.split(",");
const scheme = opts.scheme ?? "dark";
const motion = opts.motion ?? "reduce";
const here = path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Z]:)/, "$1"));
const dist = path.join(repo, "apps", "studio", "dist");

// Widget pages take a moment to load and draw inside the stage.
const settle = `await sleep(1400)`;

/** name -> steps */
const SHOTS = {
  desktop: [`nav("Desktop")`, settle],
  "desktop-widget": [`nav("Desktop")`, settle, `clickLabel("Clock widget")`],
  "desktop-wallpaper": [`nav("Desktop")`, settle, `document.querySelector('aside[aria-label=Inspector] section button')?.click(); await sleep(500)`],
  "desktop-bar": [`nav("Desktop")`, settle, `clickLabel("Top bar")`],
  themes: [`nav("Themes")`],
  "theme-editor": [`nav("Themes")`, `clickLabel("Edit Northern lights")`, settle],
  "theme-editor-widgets": [`nav("Themes")`, `clickLabel("Edit Northern lights")`, settle, `tab("Widgets")`],
  "theme-readonly": [`nav("Themes")`, `clickLabel("Copy and edit Earth orbit")`, settle],
  new: [`nav("Themes")`, `clickText("New theme")`],
  "new-details": [`nav("Themes")`, `clickText("New theme")`, `clickText("A picture or video")`, `await sleep(600)`],
  widgets: [`nav("Widgets")`, settle],
  "widgets-selected": [`nav("Widgets")`, settle, `clickLabel("Weather widget")`],
  "widgets-add": [`nav("Widgets")`, settle, `clickText("Add widget")`],
  "widgets-more": [`nav("Widgets")`, settle, `openMore()`],
  windows: [`nav("Windows")`, settle],
  "windows-menu": [`nav("Windows")`, settle, `clickText("Right-click menu")`],
  bars: [`nav("Bars")`, `await sleep(800)`],
  settings: [`nav("Settings")`],
  "settings-details": [`nav("Settings")`, `document.querySelector("details summary")?.click(); await sleep(300)`],
  palette: [`nav("Desktop")`, settle, "key:k+ctrl", "type:blur"],
  "palette-empty": [`nav("Desktop")`, "key:k+ctrl"],
  "search-reveal": [`nav("Desktop")`, "key:k+ctrl", "type:text shadow", "key:Enter", `await sleep(1500)`],
  empty: [`nav("Themes")`],
};

const HELPERS = `
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const visible = (el) => el.offsetParent !== null || el.getClientRects().length > 0;
function find(text, selector = "button, [role=tab], [role=radio], label, a, summary") {
  const all = [...document.querySelectorAll(selector)].filter(visible);
  return all.find((el) => el.textContent.trim() === text) ?? all.find((el) => el.textContent.trim().startsWith(text));
}
async function nav(text) {
  const el = find(text, 'nav[aria-label="Main"] button');
  if (!el) throw new Error("no nav item " + text);
  el.click();
  await sleep(600);
}
async function tab(text) {
  const el = find(text, '[role=tab], [role=radio], label, button');
  if (!el) throw new Error("no tab " + text);
  el.click();
  await sleep(500);
}
async function clickText(text) {
  const el = find(text);
  if (!el) throw new Error("no element " + text);
  el.click();
  await sleep(600);
}
async function clickLabel(label) {
  const el = document.querySelector('[aria-label="' + label + '"]');
  if (!el) throw new Error("no label " + label);
  el.click();
  await sleep(700);
}
async function openMore() {
  for (const d of document.querySelectorAll("aside details")) d.open = true;
  await sleep(300);
}
`;

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css", ".json": "application/json", ".jpg": "image/jpeg", ".png": "image/png", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".mp4": "video/mp4", ".webp": "image/webp" };

function serve() {
  const root = path.resolve(repo);
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, "http://x");
    let file;
    if (url.pathname.startsWith("/asset/")) {
      // Like Tauri's asset protocol: each segment decoded, joined with "/".
      file = path.resolve(url.pathname.slice("/asset/".length).split("/").map(decodeURIComponent).join("/"));
      if (!file.startsWith(root)) return res.writeHead(404).end();
    } else {
      file = path.join(dist, url.pathname === "/" ? "index.html" : decodeURIComponent(url.pathname));
    }
    fs.readFile(file, (err, data) => {
      if (err) return res.writeHead(404).end();
      res.writeHead(200, { "content-type": TYPES[path.extname(file).toLowerCase()] ?? "application/octet-stream" });
      res.end(data);
    });
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.id = 0;
    this.pending = new Map();
    ws.addEventListener("message", (event) => {
      const msg = JSON.parse(event.data);
      if (msg.id && this.pending.has(msg.id)) {
        const { resolve, reject } = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
      }
    });
  }
  send(method, params = {}, sessionId) {
    const id = ++this.id;
    this.ws.send(JSON.stringify({ id, method, params, sessionId }));
    return new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
  }
}

const KEY_CODES = { Tab: 9, Enter: 13, Escape: 27, ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40, k: 75 };

async function main() {
  fs.mkdirSync(outDir, { recursive: true });
  const server = await serve();
  const base = `http://127.0.0.1:${server.address().port}/`;
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), "studio-shots-"));
  const edge = "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe";
  const port = 9300 + Math.floor(Math.random() * 500);
  const browser = spawn(edge, ["--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "--force-device-scale-factor=1", "--disable-extensions", "--autoplay-policy=no-user-gesture-required", "about:blank"], { stdio: "ignore" });
  let version;
  for (let i = 0; i < 50 && !version; i++) {
    await new Promise((r) => setTimeout(r, 200));
    version = await fetch(`http://127.0.0.1:${port}/json/version`).then((r) => r.json(), () => undefined);
  }
  const ws = new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener("open", r));
  const cdp = new Cdp(ws);
  const mock = fs.readFileSync(path.join(here, "mock.js"), "utf8");
  const errors = [];

  try {
    for (const [name, steps] of Object.entries(SHOTS)) {
      if (only && !only.includes(name)) continue;
      const fixtures = buildFixtures(repo, { empty: name === "empty" });
      for (const [width, height] of sizes) {
        const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
        const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
        const s = (method, params) => cdp.send(method, params, sessionId);
        await s("Page.enable");
        await s("Runtime.enable");
        await s("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
        await s("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }, { name: "prefers-reduced-motion", value: motion }] });
        // Only the app's own page: the widget frames must talk to Studio, not to the mock.
        await s("Page.addScriptToEvaluateOnNewDocument", { source: `if (window !== window.top) throw new Error("not the app");\nwindow.__FIXTURES__ = ${JSON.stringify(fixtures)};\n${mock}\n${HELPERS}` });
        await s("Page.navigate", { url: base });
        await new Promise((r) => setTimeout(r, 900));
        for (const step of steps) {
          if (step.startsWith("key:")) {
            const [spec, modifier] = step.slice(4).split("+");
            const [key, times = "1"] = spec.split("*");
            const modifiers = modifier === "ctrl" ? 2 : 0;
            for (let i = 0; i < Number(times); i++) {
              for (const type of ["keyDown", "keyUp"]) await s("Input.dispatchKeyEvent", { type, key, code: key.length === 1 ? `Key${key.toUpperCase()}` : key, windowsVirtualKeyCode: KEY_CODES[key], modifiers });
            }
            await new Promise((r) => setTimeout(r, 300));
            continue;
          }
          if (step.startsWith("type:")) {
            await s("Input.insertText", { text: step.slice(5) });
            await new Promise((r) => setTimeout(r, 300));
            continue;
          }
          const result = await s("Runtime.evaluate", { expression: `(async () => { ${step}; })()`, awaitPromise: true });
          if (result.exceptionDetails) errors.push(`${name} ${width}: ${result.exceptionDetails.exception?.description ?? result.exceptionDetails.text}`);
        }
        await new Promise((r) => setTimeout(r, 500));
        const broken = await s("Runtime.evaluate", { expression: `document.body.innerText.includes("Something went wrong")`, returnByValue: true });
        if (broken.result.value) errors.push(`${name} ${width}: error boundary shown`);
        const shot = await s("Page.captureScreenshot", { format: "png" });
        const file = path.join(outDir, `${name}-${width}.png`);
        fs.writeFileSync(file, Buffer.from(shot.data, "base64"));
        console.log(file);
        await cdp.send("Target.closeTarget", { targetId });
      }
    }
  } finally {
    ws.close();
    browser.kill();
    server.close();
    if (errors.length) console.error("ERRORS:\n" + errors.join("\n"));
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
