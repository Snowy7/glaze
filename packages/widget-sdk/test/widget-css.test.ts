// The appearance contract for bundled widgets: no hard-coded colors, so users and themes can
// restyle them (docs/writing-widgets.md, "Appearance and style"). A declaration that really is
// artwork (a cover's shadow, a coin's gain and loss) opts out with `/* art */` on its line.
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const widgetsDir = join(import.meta.dirname, "..", "..", "..", "widgets");

const stylesheets = readdirSync(widgetsDir, { withFileTypes: true })
  .filter((d) => d.isDirectory() && existsSync(join(widgetsDir, d.name, "widget.json")))
  .flatMap((d) =>
    readdirSync(join(widgetsDir, d.name))
      .filter((f) => f.endsWith(".css") && f !== "base.css")
      .map((f) => ({ widget: d.name, file: f, path: join(widgetsDir, d.name, f) })),
  );

const NAMED = /\b(white|black|red|green|blue|yellow|orange|purple|pink|gray|grey|silver|gold|cyan|magenta|navy|teal|lime|maroon|olive|aqua|fuchsia|brown)\b/i;

/** Declarations (`prop: value`) of a stylesheet that name a color of their own. */
export function hardCodedColors(css: string): string[] {
  const found: string[] = [];
  for (const line of css.split("\n")) {
    if (line.includes("/* art */")) continue;
    const code = line.replace(/\/\*.*?\*\//g, "").replace(/url\([^)]*\)/g, "");
    for (const m of code.matchAll(/([a-z-]+)\s*:\s*([^;{}]+)(?:;|}|$)/gi)) {
      const [, prop = "", value = ""] = m;
      // Only property values: selectors like `a:hover` never reach a terminator on the same capture.
      const bad =
        /#[0-9a-f]{3,8}\b/i.test(value) ||
        /\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch)\(\s*(?!var\()/i.test(value) ||
        (NAMED.test(value) && !/^(font|font-family|content|animation-name|grid-area|will-change|transition-property)$/.test(prop));
      if (bad) found.push(`${prop}: ${value.trim()}`);
    }
  }
  return found;
}

describe("bundled widget CSS", () => {
  it("finds stylesheets", () => {
    expect(stylesheets.length).toBeGreaterThan(0);
  });

  it.each(stylesheets)("$widget/$file has no hard-coded colors", ({ path }) => {
    expect(hardCodedColors(readFileSync(path, "utf8"))).toEqual([]);
  });

  describe("the detector", () => {
    it("flags hex, rgb() and named colors", () => {
      expect(hardCodedColors(".a { color: #fff; }")).toHaveLength(1);
      expect(hardCodedColors(".a { background: rgb(0 0 0 / 0.5); }")).toHaveLength(1);
      expect(hardCodedColors(".a { border-color: red; }")).toHaveLength(1);
      expect(hardCodedColors(".a { box-shadow: 0 1px 2px rgba(0,0,0,.4) }")).toHaveLength(1);
    });

    it("allows the variables, channel triples, keywords and opted-out artwork", () => {
      expect(hardCodedColors(".a { color: var(--t1); background: rgb(var(--ink) / 0.3); }")).toEqual([]);
      expect(hardCodedColors(".a { fill: url(#fade); stroke: currentColor; background: transparent; }")).toEqual([]);
      expect(hardCodedColors(".a:hover { color: var(--t0) }\n#beef { margin: 0 }")).toEqual([]);
      expect(hardCodedColors(".a { box-shadow: 0 6px 18px rgb(0 0 0 / 0.35); /* art */ }")).toEqual([]);
    });
  });
});
