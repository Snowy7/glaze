// Copies the shared files into every widgets/<id>/ folder, because a widget is served from its
// own origin and can't reach outside its folder. Run by `pnpm build`.
//   dist/mypaper.js      the drop-in runtime
//   widget-kit/base.css  tokens and the shared surface
//   widget-kit/kit.js    helpers on top of `mypaper` (size, visibility, timers)
import { copyFileSync, existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const pkg = fileURLToPath(new URL("..", import.meta.url));
export const widgetsDir = join(pkg, "..", "..", "widgets");

/** @type {[from: string, name: string][]} */
const shared = [
  [join(pkg, "dist", "mypaper.js"), "mypaper.js"],
  [join(pkg, "widget-kit", "base.css"), "base.css"],
  [join(pkg, "widget-kit", "kit.js"), "kit.js"],
];

/**
 * [from, to] pairs for every widget folder (those with a widget.json).
 * @returns {[from: string, to: string][]}
 */
export function copyPlan() {
  return readdirSync(widgetsDir, { withFileTypes: true })
    .filter((d) => d.isDirectory() && existsSync(join(widgetsDir, d.name, "widget.json")))
    .flatMap((d) => shared.map(([from, name]) => /** @type {[string, string]} */ ([from, join(widgetsDir, d.name, name)])));
}

if (import.meta.url === `file:///${process.argv[1]?.replaceAll("\\", "/")}`) {
  const plan = copyPlan();
  for (const [from, to] of plan) copyFileSync(from, to);
  console.log(`synced ${plan.length} files into ${plan.length / shared.length} widgets`);
}
