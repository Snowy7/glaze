// Headless screenshot of a harness page, with Microsoft Edge (no window is shown).
//   node scripts/shot.mjs <name> <path-and-query> [WxH]
// Examples (the harness must be running: `pnpm dev`, default http://localhost:5173):
//   node scripts/shot.mjs clock "/gallery.html?widget=clock"                      the contact sheet
//   node scripts/shot.mjs clock-glass "/?ui=0&widget=clock&surface=glass&bg=photo-dark"
// Writes <tmp>/mypaper-shots/<name>.png and prints the path. MYPAPER_HARNESS sets the base URL.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [name, target, size = "1500x1100"] = process.argv.slice(2);
if (!name || !target) {
  console.error("usage: node scripts/shot.mjs <name> <path-and-query> [WxH]");
  process.exit(2);
}

const candidates = [
  "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
  "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
];
const edge = candidates.find((p) => existsSync(p));
if (!edge) {
  console.error("Microsoft Edge not found");
  process.exit(1);
}

const dir = join(tmpdir(), "mypaper-shots");
mkdirSync(dir, { recursive: true });
const out = join(dir, `${name}.png`);
const base = process.env["MYPAPER_HARNESS"] ?? "http://localhost:5173";
const profile = join(dir, "profile");
rmSync(out, { force: true });

const result = spawnSync(
  edge,
  [
    "--headless=new",
    "--disable-gpu",
    "--hide-scrollbars",
    "--no-first-run",
    `--user-data-dir=${profile}`,
    `--screenshot=${out}`,
    `--window-size=${size.replace("x", ",")}`,
    "--virtual-time-budget=6000",
    `${base}${target}`,
  ],
  { stdio: "ignore", timeout: 60_000 },
);
// The new headless mode can return before the file is flushed.
for (let i = 0; i < 60 && !existsSync(out); i++) await new Promise((r) => setTimeout(r, 250));
if (result.status !== 0 || !existsSync(out)) {
  console.error(`screenshot failed (exit ${result.status})`);
  process.exit(1);
}
console.log(out);
