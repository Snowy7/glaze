// Screenshots of the built Studio frontend (apps/studio/dist) in a headless browser, with Tauri's
// IPC mocked by fixtures (fixtures.mjs, mock.js). No window is shown and nothing takes focus. The
// real widget pages run in the stage, served from the repo's widgets folder.
//
// The browser is chrome-headless-shell when one is on disk (Playwright's, or MYPAPER_SHOT_BROWSER):
// it has no windows at all, so nothing reaches the Windows shell. Headless Edge is the fallback,
// with its Windows tab integration off (Alt+Tab lists Edge tabs otherwise). Either way every page
// is titled "shot" from its first byte, each capture gets one page that is closed when it's taken,
// and the run ends by asking the browser to quit; only what's left after that is killed (see
// "The browser" below), then its profile is deleted.
//
//   pnpm -C apps/studio build
//   node apps/studio/tools/shots/shoot.mjs <repo> <outDir> [sizes=1280x800,2560x1440] [only=desktop,settings] [scheme=dark]
//
// Steps are page-side JavaScript using the helpers below, or `key:<Key>[*n][+ctrl]`, `type:<text>`
// and `drag:<expression giving [x1, y1, x2, y2]>` (pressed and moved, held for the shot).
import { spawn, spawnSync } from "node:child_process";
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
  "themes-legacy": [`nav("Themes")`],
  "desktop-credit": [`nav("Desktop")`, settle, `document.querySelector('aside[aria-label=Inspector] section button')?.click(); await sleep(500)`],
  "theme-editor": [`nav("Themes")`, `clickLabel("Edit Northern lights")`, settle],
  "theme-editor-widgets": [`nav("Themes")`, `clickLabel("Edit Northern lights")`, settle, `[...document.querySelectorAll("aside label")].find((l) => l.textContent.trim() === "Widgets").click(); await sleep(500)`],
  "theme-readonly": [`nav("Themes")`, `clickLabel("Copy and edit Star Nest")`, settle],
  new: [`nav("Themes")`, `clickText("New theme")`],
  "new-details": [`nav("Themes")`, `clickText("New theme")`, `clickText("A picture or video")`, `await sleep(600)`],
  widgets: [`nav("Widgets")`, settle],
  "widgets-selected": [`nav("Widgets")`, settle, `clickLabel("Weather widget")`],
  "widgets-place": [`nav("Widgets")`, settle, `clickLabel("Weather widget")`, `clickText("A town")`, `document.querySelector('input[aria-label^="Find a town"]').focus()`, "type:par", `await sleep(900)`],
  "widgets-add": [`nav("Widgets")`, settle, `clickText("Add widget")`],
  "widgets-more": [`nav("Widgets")`, settle, `openMore()`],
  windows: [`nav("Windows")`, settle],
  "windows-menu": [`nav("Windows")`, settle, `clickText("Right-click menu")`],
  bars: [`nav("Bars")`, settle],
  "bars-top": [`nav("Bars")`, settle, `clickLabel("Top bar")`, `await sleep(1500)`],
  "bars-dock": [`nav("Bars")`, settle, `clickLabel("Bottom bar")`, `await sleep(1500)`],
  "bars-top-more": [`nav("Bars")`, settle, `clickLabel("Top bar")`, `openMore()`, `document.querySelector("aside[aria-label=Inspector]").scrollTop = 1500; await sleep(400)`],
  "bars-module": [`nav("Bars")`, settle, `clickLabel("Top bar")`, `await clickIn("aside", "Clock")`],
  "bars-add": [`nav("Bars")`, settle, `clickLabel("Top bar")`, `await clickIn("aside", "Add")`],
  "bars-drag": [`nav("Bars")`, settle, "drag:[...moduleAt('Top bar', 'clock'), ...moduleAt('Top bar', 'start', -12)]"],
  "bars-taskbar": [`nav("Bars")`, settle, `clickText("Windows 11")`, `await sleep(1200)`, `clickLabel("Bottom bar")`, `await sleep(1200)`],
  "bars-taskbar-module": [`nav("Bars")`, settle, `clickText("Windows 11")`, `await sleep(1200)`, `clickLabel("Bottom bar")`, `await sleep(800)`, `await clickIn("aside", "Clock")`, `await sleep(800)`],
  "bars-dock-left": [`nav("Bars")`, settle, `clickLabel("Bottom bar")`, `await clickIn("aside", "Left")`, `await sleep(1500)`],
  "bars-overview-more": [`nav("Bars")`, settle, `openMore()`, `document.querySelector("aside[aria-label=Inspector]").scrollTop = 900; await sleep(400)`],
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
async function clickIn(scope, text) {
  const el = [...document.querySelectorAll(scope + " button, " + scope + " label")].filter(visible).find((b) => b.textContent.trim() === text);
  if (!el) throw new Error("no " + text + " in " + scope);
  el.click();
  await sleep(700);
}
/** The page point at the middle of a module in a bar frame (moved dx page pixels). */
function moduleAt(frameTitle, ref, dx = 0) {
  const frame = document.querySelector('iframe[title="' + frameTitle + '"]');
  const el = frame?.contentDocument?.querySelector('[data-module="' + ref + '"]');
  if (!el) throw new Error("no module " + ref);
  const outer = frame.getBoundingClientRect();
  const k = outer.width / frame.offsetWidth;
  const box = el.getBoundingClientRect();
  return [outer.left + (box.left + box.width / 2) * k + dx, outer.top + (box.top + box.height / 2) * k];
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
      const type = TYPES[path.extname(file).toLowerCase()] ?? "application/octet-stream";
      res.writeHead(200, { "content-type": type });
      // Pages are titled "shot", never "mypaper ...": nothing the harness opens reads as an app window.
      res.end(type === "text/html" ? data.toString("utf8").replace(/<title>[^<]*<\/title>/i, "<title>shot</title>") : data);
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
  /** Sends without waiting for an answer (a browser asked to close may never give one). */
  fire(method) {
    this.ws.send(JSON.stringify({ id: ++this.id, method }));
  }
}

const KEY_CODES = { Tab: 9, Enter: 13, Escape: 27, ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40, k: 75 };

// --- The browser: one per run, closed gently, never left behind ---
//
// Edge registers its pages with the Windows shell (Alt+Tab lists them) and only unregisters them
// when it closes itself: a browser killed outright leaves ghost entries until Explorer restarts.
// So a run never force-kills first: each page is closed (`Target.closeTarget`) once its capture
// is done, the browser is asked to quit (`Browser.close`) and given up to 5 s to exit, and only
// then is anything left (renderer or GPU children of a stuck browser, matched by this run's
// profile folder) killed and the profile deleted. That runs at the end, on error, on Ctrl+C and,
// through a detached watchdog that waits for this node process, even when node itself is killed.
//
// "Exit" means the browser's own process, which CDP names (`SystemInfo.getProcessInfo`): the
// msedge.exe we spawn is only a launcher that hands over to it and exits within a few ms.

const PROFILE_PREFIX = "studio-shots-";
const CALL_TIMEOUT_MS = 45_000;
/** How long the browser gets to quit by itself before what's left is killed. */
const QUIT_GRACE_MS = 5_000;
/** Every page's title while it loads and after: nothing the harness opens reads as an app window. */
const TITLE = "shot";

/** The newest `<root>/<version dir>/<rest>` that exists. */
function newest(root, rest) {
  try {
    return fs
      .readdirSync(root)
      .map((entry) => path.join(root, entry, rest))
      .filter((candidate) => fs.existsSync(candidate))
      .sort()
      .at(-1);
  } catch {
    return undefined;
  }
}

/** chrome-headless-shell if one is on disk (no windows, no shell integration), else headless Edge. */
function findBrowser() {
  const local = process.env.LOCALAPPDATA ?? path.join(os.homedir(), "AppData", "Local");
  const shell =
    process.env.MYPAPER_SHOT_BROWSER ??
    newest(path.join(local, "ms-playwright"), path.join("chrome-headless-shell-win64", "chrome-headless-shell.exe")) ??
    newest(path.join(os.homedir(), ".t3", "tools", "chrome-headless-shell", "win64"), "chrome-headless-shell.exe");
  if (shell && fs.existsSync(shell)) return { exe: shell, args: [] };
  const edge = ["C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", "C:/Program Files/Microsoft/Edge/Application/msedge.exe"].find((p) => fs.existsSync(p));
  if (!edge) throw new Error("no headless browser: install Playwright's chromium-headless-shell, or set MYPAPER_SHOT_BROWSER");
  // msWindowTabManagerPublic is Edge's Windows tab manager, which lists its tabs in Alt+Tab.
  return { exe: edge, args: ["--headless=new", "--disable-features=msWindowTabManagerPublic"] };
}

/**
 * PowerShell for after the browser has had its chance to quit: kills this run's browser processes
 * that are still there (`exe`'s name, matched by profile folder), waits until they're gone (a dying
 * browser still holds its profile's files), then deletes the folder, retrying for a while.
 */
const reapScript = (profile, exe) => {
  const quoted = profile.replace(/'/g, "''");
  const name = path.basename(exe).replace(/'/g, "''");
  const mine = `@(Get-CimInstance Win32_Process -Filter "Name = '${name}'" | Where-Object { $_.CommandLine -like '*${quoted}*' })`;
  return (
    `for ($i = 0; $i -lt 50; $i++) { $p = ${mine}; if ($p.Count -eq 0) { break }; $p | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }; Start-Sleep -Milliseconds 200 }; ` +
    `for ($i = 0; $i -lt 50 -and (Test-Path -LiteralPath '${quoted}'); $i++) { Remove-Item -LiteralPath '${quoted}' -Recurse -Force -ErrorAction SilentlyContinue; if (Test-Path -LiteralPath '${quoted}') { Start-Sleep -Milliseconds 200 } }`
  );
};

/** Whether a process is still running. */
const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

/**
 * The shutdown as a standalone script, for when this process can't do it itself:
 * `node -e SHUTDOWN <wait for pid, or 0> <debug port> <reap script>`. The watchdog runs it once
 * the harness is gone; the exit handler runs it at once (an exit handler can't wait). It finds
 * the browser's process over CDP, asks it to quit, waits up to QUIT_GRACE_MS for it, then reaps.
 */
const SHUTDOWN = `
const [parent, port, script] = process.argv.slice(1);
const alive = (id) => { try { process.kill(Number(id), 0); return true; } catch { return false; } };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function quit() {
  try {
    const version = await fetch("http://127.0.0.1:" + port + "/json/version", { signal: AbortSignal.timeout(1500) }).then((r) => r.json());
    const ws = new WebSocket(version.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; setTimeout(reject, 1500); });
    let closed = false;
    ws.onclose = () => (closed = true);
    const info = await new Promise((resolve) => {
      ws.onmessage = (m) => resolve(JSON.parse(String(m.data)));
      ws.send(JSON.stringify({ id: 1, method: "SystemInfo.getProcessInfo" }));
      setTimeout(() => resolve(null), 1500);
    });
    const pid = info?.result?.processInfo?.find((p) => p.type === "browser")?.id;
    ws.send(JSON.stringify({ id: 2, method: "Browser.close" }));
    const gone = () => (pid ? !alive(pid) : closed);
    for (let waited = 0; waited < ${QUIT_GRACE_MS} && !gone(); waited += 100) await sleep(100);
  } catch {}
}
async function run() {
  await quit();
  require("node:child_process").spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { stdio: "ignore", windowsHide: true });
  process.exit(0);
}
if (parent === "0") run();
else { const timer = setInterval(() => { if (alive(parent)) return; clearInterval(timer); run(); }, 500); }
`;

/** Removes profiles of earlier runs that died before cleaning up (older than an hour, so not a run in progress). */
function sweepStaleProfiles() {
  const hourAgo = Date.now() - 3600_000;
  for (const entry of fs.readdirSync(os.tmpdir(), { withFileTypes: true })) {
    if (!entry.isDirectory() || !entry.name.startsWith(PROFILE_PREFIX)) continue;
    const dir = path.join(os.tmpdir(), entry.name);
    try {
      if (fs.statSync(dir).mtimeMs < hourAgo) fs.rmSync(dir, { recursive: true, force: true });
    } catch {
      // In use: its run (or its watchdog) cleans it.
    }
  }
}

/**
 * Starts the headless browser on a fresh profile. `shutdown(cdp, pid)` (idempotent) asks it to
 * quit, waits for its process `pid` (from CDP) to exit, then reaps what's left and the profile;
 * `shutdownNow()` is the same as a separate, synchronous process, for exit handlers.
 */
function launchBrowser() {
  const { exe, args } = findBrowser();
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), PROFILE_PREFIX));
  const port = 9300 + Math.floor(Math.random() * 500);
  const browser = spawn(exe, [...args, `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "--force-device-scale-factor=1", "--disable-extensions", "--autoplay-policy=no-user-gesture-required", "about:blank"], { stdio: "ignore", windowsHide: true });
  const reap = reapScript(profile, exe);
  const shutdownArgs = (parent) => ["-e", SHUTDOWN, parent, String(port), reap];
  // If this node process is killed outright, nothing below runs: the watchdog does the shutdown
  // then. It is a detached node (a detached PowerShell has no console and exits at once).
  spawn(process.execPath, shutdownArgs(String(process.pid)), { detached: true, stdio: "ignore", windowsHide: true }).unref();

  let done = false;
  const reapNow = () => spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", reap], { stdio: "ignore", windowsHide: true });
  const shutdown = async (cdp, pid) => {
    if (done) return;
    done = true;
    // Without CDP (it never came up) there's no way to ask; what's there is reaped below.
    const main = pid ?? browser.pid;
    if (cdp && main && alive(main)) {
      try {
        cdp.fire("Browser.close");
      } catch {
        // The connection is gone already.
      }
      for (let waited = 0; waited < QUIT_GRACE_MS && alive(main); waited += 100) await new Promise((r) => setTimeout(r, 100));
    }
    const quit = !main || !alive(main);
    // Only now, and only what didn't quit: the browser's tree, then anything on our profile.
    if (!quit) spawnSync("taskkill", ["/PID", String(main), "/T", "/F"], { stdio: "ignore", windowsHide: true });
    reapNow();
    console.error(quit ? "browser quit by itself" : `browser didn't quit within ${QUIT_GRACE_MS / 1000} s: killed what was left`);
  };
  const shutdownNow = () => {
    if (done) return;
    done = true;
    spawnSync(process.execPath, shutdownArgs("0"), { stdio: "ignore", windowsHide: true });
  };
  return { port, shutdown, shutdownNow, name: path.basename(exe) };
}

/** `promise`, or an error after `ms`: a stuck page can't hang the run (and leave its browser). */
function within(promise, ms, what) {
  let timer;
  const timeout = new Promise((_, reject) => (timer = setTimeout(() => reject(new Error(`timed out: ${what}`)), ms)));
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

async function main() {
  fs.mkdirSync(outDir, { recursive: true });
  sweepStaleProfiles();
  const server = await serve();
  const base = `http://127.0.0.1:${server.address().port}/`;
  const { port, shutdown, shutdownNow, name: browserName } = launchBrowser();
  console.error(`browser: ${browserName}`);
  let cdp = null;
  let browserPid;
  process.on("exit", shutdownNow);
  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP", "SIGBREAK"]) {
    process.on(signal, () => void shutdown(cdp, browserPid).finally(() => process.exit(130)));
  }
  const errors = [];
  let ws;

  try {
    let version;
    for (let i = 0; i < 50 && !version; i++) {
      await new Promise((r) => setTimeout(r, 200));
      version = await fetch(`http://127.0.0.1:${port}/json/version`).then((r) => r.json(), () => undefined);
    }
    if (!version) throw new Error("the headless browser didn't start");
    ws = new WebSocket(version.webSocketDebuggerUrl);
    await within(new Promise((r) => ws.addEventListener("open", r)), 10_000, "connecting to the browser");
    const raw = new Cdp(ws);
    cdp = { send: (method, params, sessionId) => within(raw.send(method, params, sessionId), CALL_TIMEOUT_MS, method), fire: (method) => raw.fire(method) };
    browserPid = (await cdp.send("SystemInfo.getProcessInfo").catch(() => null))?.processInfo?.find((p) => p.type === "browser")?.id;
    const mock = fs.readFileSync(path.join(here, "mock.js"), "utf8");

    for (const [name, steps] of Object.entries(SHOTS)) {
      if (only && !only.includes(name)) continue;
      const fixtures = buildFixtures(repo, { empty: name === "empty", ...(name === "themes-legacy" && { active: "webdemo" }) });
      for (const [width, height] of sizes) {
        // A page for this capture, closed (by CDP, so the browser lets go of it) once it's taken.
        const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
        try {
          const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
          const s = (method, params) => cdp.send(method, params, sessionId);
          await s("Page.enable");
          await s("Runtime.enable");
          await s("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
          await s("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }, { name: "prefers-reduced-motion", value: motion }] });
          await s("Storage.clearDataForOrigin", { origin: base.replace(/\/$/, ""), storageTypes: "local_storage" });
          // The fixtures and helpers, in the app's own page only: the widget frames must talk to
          // Studio, not to the mock. Every page keeps the neutral title (served pages have it too).
          await s("Page.addScriptToEvaluateOnNewDocument", {
            source: `document.title = ${JSON.stringify(TITLE)}; new MutationObserver(() => document.title !== ${JSON.stringify(TITLE)} && (document.title = ${JSON.stringify(TITLE)})).observe(document, { subtree: true, childList: true, characterData: true });\nif (window !== window.top) throw new Error("not the app");\nwindow.__FIXTURES__ = ${JSON.stringify(fixtures)};\n${mock}\n${HELPERS}`,
          });
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
            if (step.startsWith("drag:")) {
              // Press, then move in steps, and hold: the shot shows the drag in progress.
              const found = await s("Runtime.evaluate", { expression: step.slice(5), returnByValue: true });
              if (found.exceptionDetails) {
                errors.push(`${name} ${width}: ${found.exceptionDetails.exception?.description ?? found.exceptionDetails.text}`);
                continue;
              }
              const [x1, y1, x2, y2] = found.result.value;
              await s("Input.dispatchMouseEvent", { type: "mouseMoved", x: x1, y: y1 });
              await s("Input.dispatchMouseEvent", { type: "mousePressed", x: x1, y: y1, button: "left", buttons: 1, clickCount: 1 });
              for (let i = 1; i <= 10; i++) await s("Input.dispatchMouseEvent", { type: "mouseMoved", x: x1 + ((x2 - x1) * i) / 10, y: y1 + ((y2 - y1) * i) / 10, button: "left", buttons: 1 });
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
          await s("Runtime.evaluate", { expression: `document.title = ${JSON.stringify(TITLE)}` });
          const shot = await s("Page.captureScreenshot", { format: "png" });
          const file = path.join(outDir, `${name}-${width}.png`);
          fs.writeFileSync(file, Buffer.from(shot.data, "base64"));
          console.log(file);
        } finally {
          await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
        }
      }
    }
  } finally {
    await shutdown(cdp, browserPid);
    ws?.close();
    server.close();
    if (errors.length) console.error("ERRORS:\n" + errors.join("\n"));
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
