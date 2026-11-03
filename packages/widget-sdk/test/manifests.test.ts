// The bundled manifests must satisfy the schemas, and the shared files copied into every widget
// folder must match their sources (a widget can't import from outside its folder).
import Ajv from "ajv";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { copyPlan } from "../scripts/sync-widgets.mjs";

const root = join(import.meta.dirname, "..", "..", "..");
const read = (path: string): unknown => JSON.parse(readFileSync(path, "utf8"));
const ajv = new Ajv({ allErrors: true, strict: false });
const validateWidget = ajv.compile(read(join(import.meta.dirname, "..", "schema", "widget.schema.json")) as object);
const validatePlugin = ajv.compile(read(join(import.meta.dirname, "..", "schema", "plugin.schema.json")) as object);

function folders(dir: string, manifest: string): string[] {
  return readdirSync(join(root, dir), { withFileTypes: true })
    .filter((d) => d.isDirectory() && existsSync(join(root, dir, d.name, manifest)))
    .map((d) => d.name);
}

/** Every widget the repo ships (a missing folder is a mistake, so it is listed here). */
const BUNDLED_WIDGETS = [
  "analog-clock", "battery", "calendar", "clock", "countdown", "crypto", "disk", "launcher", "network", "notes",
  "now-playing", "photo", "quote", "rss", "search", "system-info", "system-monitor", "timer", "todo", "weather",
  "world-clock",
];

const widgets = folders("widgets", "widget.json");
const plugins = folders("plugins", "plugin.json");

describe("bundled manifests", () => {
  it("finds the bundled widgets and plugins", () => {
    expect(widgets).toEqual(expect.arrayContaining(BUNDLED_WIDGETS));
    expect(plugins).toEqual(expect.arrayContaining(["weather", "rss"]));
  });

  it.each(widgets)("widget %s matches the schema and the spec's rules", (id) => {
    const manifest = read(join(root, "widgets", id, "widget.json")) as {
      id: string;
      entry: string;
      size: { default: number[]; min: number[]; max: number[] };
      plugins?: string[];
      $schema?: string;
    };
    expect(validateWidget(manifest), JSON.stringify(validateWidget.errors)).toBe(true);
    expect(manifest.id).toBe(id); // the id is the folder name
    expect(manifest.$schema).toBeTruthy();
    expect(existsSync(join(root, "widgets", id, manifest.entry))).toBe(true);
    // min <= default <= max, per axis (JSON Schema can't express this)
    for (const axis of [0, 1]) {
      const [lo, def, hi] = [manifest.size.min[axis], manifest.size.default[axis], manifest.size.max[axis]];
      expect(lo).toBeLessThanOrEqual(def ?? NaN);
      expect(def).toBeLessThanOrEqual(hi ?? NaN);
    }
    // every plugin a widget uses exists in plugins/
    for (const p of manifest.plugins ?? []) expect(plugins).toContain(p);
  });

  it.each(plugins)("plugin %s matches the schema", (id) => {
    const manifest = read(join(root, "plugins", id, "plugin.json"));
    expect(validatePlugin(manifest), JSON.stringify(validatePlugin.errors)).toBe(true);
  });

  it("the schema rejects broken manifests", () => {
    const base = read(join(root, "widgets", "clock", "widget.json")) as Record<string, unknown>;
    expect(validateWidget({ ...base, id: "Not Valid" })).toBe(false);
    expect(validateWidget({ ...base, permissions: ["root"] })).toBe(false);
    expect(validateWidget({ ...base, settings: [{ key: "x", type: "select", label: "X", default: "a" }] })).toBe(false);
  });
});

describe("shared widget files", () => {
  const plan = copyPlan();
  const built = existsSync(join(import.meta.dirname, "..", "dist", "mypaper.js"));

  it.each(plan.map(([from, to]) => [to.slice(root.length + 1), from, to] as const))(
    "%s is in sync with its source",
    (name, from, to) => {
      // dist/ is gitignored: the drop-in copy can only be compared after a build.
      if (!built && from.endsWith("mypaper.js")) return;
      expect(readFileSync(to, "utf8"), `${name} is stale: run \`pnpm build\``).toBe(readFileSync(from, "utf8"));
    },
  );
});
