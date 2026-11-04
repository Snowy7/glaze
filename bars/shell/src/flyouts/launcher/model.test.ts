import { describe, expect, it } from "vitest";
import type { LauncherApp } from "../protocol";
import { alphabetical, gridStep, recentApps, resolvePins, search, step } from "./model";
import { SETTINGS_PAGES } from "./settings-pages";

const app = (name: string, id = name): LauncherApp => ({ id, name, path: `shell:AppsFolder\\${id}`, packaged: false, icon: `${id}.png` });
const apps = [app("Terminal", "Microsoft.WindowsTerminal!App"), app("Firefox"), app("7-Zip"), app("éditeur"), app("Calculator")];

describe("resolvePins", () => {
  it("finds pins in the index by path, then by name, else keeps them bare", () => {
    const pins = resolvePins(
      [
        { path: "shell:AppsFolder\\Microsoft.WindowsTerminal!App" },
        { path: "C:\\Users\\me\\AppData\\Roaming\\...\\TaskBar\\Firefox.lnk" },
        { path: "C:\\Tools\\thing.exe", name: "Thing" },
      ],
      apps,
    );
    expect(pins.map((p) => [p.name, p.icon])).toEqual([
      ["Terminal", "Microsoft.WindowsTerminal!App.png"],
      ["Firefox", "Firefox.png"],
      ["Thing", undefined],
    ]);
    expect(pins[1]?.path).toMatch(/Firefox\.lnk$/);
  });
});

describe("alphabetical", () => {
  it("groups by folded first letter, symbols first", () => {
    expect(alphabetical(apps).map((g) => [g.letter, g.apps.map((a) => a.name)])).toEqual([
      ["#", ["7-Zip"]],
      ["C", ["Calculator"]],
      ["E", ["éditeur"]],
      ["F", ["Firefox"]],
      ["T", ["Terminal"]],
    ]);
  });
});

describe("search and history", () => {
  it("searches apps, settings pages and files", () => {
    const files = [{ name: "calc notes", path: "C:\\Recent\\calc notes.lnk", at: 0 }];
    const results = search("calc", { apps, settings: SETTINGS_PAGES, files }, [], 0);
    expect(results[0]?.name).toBe("Calculator");
    expect(results.some((r) => r.kind === "file")).toBe(true);
    expect(search("wallpaper", { apps, settings: SETTINGS_PAGES, files: [] }, [], 0)[0]?.path).toBe(
      "ms-settings:personalization-background",
    );
  });

  it("lists recent apps that are still installed", () => {
    const recent = [
      { id: "Firefox", path: "", count: 3, last: 2 },
      { id: "gone", path: "", count: 1, last: 1 },
      { id: "Calculator", path: "", count: 1, last: 0 },
    ];
    expect(recentApps(recent, apps, 5).map((e) => e.name)).toEqual(["Firefox", "Calculator"]);
  });
});

describe("keyboard selection", () => {
  it("wraps through a list", () => {
    expect(step(-1, 3, 1)).toBe(0);
    expect(step(-1, 3, -1)).toBe(2);
    expect(step(2, 3, 1)).toBe(0);
    expect(step(0, 3, -1)).toBe(2);
    expect(step(0, 0, 1)).toBe(-1);
  });

  it("moves through a grid, clamped", () => {
    expect(gridStep(1, 10, 4, "down")).toBe(5);
    expect(gridStep(1, 10, 4, "up")).toBe(0);
    expect(gridStep(9, 10, 4, "right")).toBe(9);
    expect(gridStep(8, 10, 4, "down")).toBe(9);
  });
});
