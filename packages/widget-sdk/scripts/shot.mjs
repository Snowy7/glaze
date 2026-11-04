// Headless screenshot of a harness page, with Microsoft Edge (no window is shown).
//   node scripts/shot.mjs <name> <path-and-query> [WxH]
// Examples (the harness must be running: `pnpm dev`, default http://localhost:5173):
//   node scripts/shot.mjs clock "/gallery.html?widget=clock"                      the contact sheet
//   node scripts/shot.mjs clock-glass "/?ui=0&widget=clock&surface=glass&bg=photo-dark"
// Writes <tmp>/mypaper-shots/<name>.png and prints the path. MYPAPER_HARNESS sets the base URL;
// MYPAPER_SHOT_SCALE renders at a device scale (1.5 is a 150% display, to check crispness).
//
// Every run gets its own throwaway profile, and always (even on failure) ends by killing every
// Edge process that uses it (headless Edge leaves renderer and GPU children running after the
// screenshot) and deleting it, so nothing piles up.
import { spawn, spawnSync } from "node:child_process";
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
// Unique per run, so shots of the same name (or from several agents) never share a profile.
const profile = join(dir, `profile-${name}-${process.pid}-${Date.now()}`);
rmSync(out, { force: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Kills the Edge process tree, and any process still using this profile (orphaned children). */
function killEdge(pid) {
  if (pid) spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore" });
  const needle = profile.replaceAll("'", "''");
  spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      `Get-CimInstance Win32_Process -Filter "Name = 'msedge.exe'" | Where-Object { $_.CommandLine -like '*${needle}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`,
    ],
    { stdio: "ignore" },
  );
}

/** Deletes the profile, retrying while Windows still holds files of processes just killed. */
async function removeProfile() {
  for (let i = 0; i < 20; i++) {
    try {
      rmSync(profile, { recursive: true, force: true });
      return;
    } catch {
      await sleep(250);
    }
  }
  console.error(`could not delete ${profile}`);
}

let child;
let ok = false;
try {
  child = spawn(
    edge,
    [
      "--headless=new",
      "--disable-gpu",
      "--hide-scrollbars",
      "--no-first-run",
      `--user-data-dir=${profile}`,
      `--screenshot=${out}`,
      `--window-size=${size.replace("x", ",")}`,
      `--force-device-scale-factor=${Number(process.env["MYPAPER_SHOT_SCALE"]) || 1}`,
      "--virtual-time-budget=6000",
      `${base}${target}`,
    ],
    { stdio: "ignore" },
  );
  const exited = new Promise((resolve) => child.on("exit", resolve).on("error", resolve));
  // Done when the file is there (the new headless mode can exit before it is flushed, or linger
  // after it is written), or after a minute.
  const deadline = Date.now() + 60_000;
  let gone = false;
  void exited.then(() => (gone = true));
  while (Date.now() < deadline && !existsSync(out)) {
    if (gone) {
      // Exited: give the file a moment to appear.
      for (let i = 0; i < 20 && !existsSync(out); i++) await sleep(250);
      break;
    }
    await sleep(150);
  }
  // A just-created file may still be being written.
  if (existsSync(out)) await sleep(300);
  ok = existsSync(out);
} finally {
  killEdge(child?.pid);
  await removeProfile();
}
if (!ok) {
  console.error("screenshot failed");
  process.exit(1);
}
console.log(out);
