import { describe, expect, it } from "vitest";
import { fold, frecency, fuzzyScore, rank, type Candidate } from "./fuzzy";

const app = (name: string, keywords?: string[]): Candidate => ({ id: name, name, kind: "app", ...(keywords ? { keywords } : {}) });

describe("fuzzyScore", () => {
  it("orders exact, prefix, word prefix, initials, substring and scattered letters", () => {
    const scores = [
      fuzzyScore("code", "Code"),
      fuzzyScore("code", "Codex"),
      fuzzyScore("code", "Visual Studio Code"),
      fuzzyScore("vsc", "Visual Studio Code"),
      fuzzyScore("ode", "Visual Studio Code"),
      fuzzyScore("vscd", "Visual Studio Code"),
    ];
    for (let i = 1; i < scores.length; i++) expect(scores[i - 1]).toBeGreaterThan(scores[i] ?? 0);
    expect(fuzzyScore("xyz", "Visual Studio Code")).toBe(-1);
  });

  it("ignores case and accents, and needs every word", () => {
    expect(fuzzyScore("CAFE", "Café Finder")).toBeGreaterThan(800);
    expect(fold("Ünïcödé")).toBe("unicode");
    expect(fuzzyScore("studio code", "Visual Studio Code")).toBeGreaterThan(700);
    expect(fuzzyScore("studio zebra", "Visual Studio Code")).toBe(-1);
  });
});

describe("rank", () => {
  const apps = [app("Calculator"), app("Calendar"), app("Camera"), app("Visual Studio Code", ["code"])];

  it("puts the best match first and drops non-matches", () => {
    expect(rank("calc", apps, [], 0).map((a) => a.name)).toEqual(["Calculator"]);
    // Among prefix matches, the closest (shortest) name first.
    expect(rank("ca", apps, [], 0).map((a) => a.name)).toEqual(["Camera", "Calendar", "Calculator"]);
  });

  it("lets keywords find an app", () => {
    expect(rank("code", apps, [], 0)[0]?.name).toBe("Visual Studio Code");
  });

  it("puts what the user runs often and lately first", () => {
    const now = 10 * 86_400_000;
    const history = [{ id: "Camera", count: 12, last: now - 1000 }];
    expect(rank("ca", apps, history, now)[0]?.name).toBe("Camera");
    expect(frecency(undefined, now)).toBe(0);
    expect(frecency({ id: "x", count: 1, last: 0 }, now)).toBeLessThan(frecency({ id: "x", count: 1, last: now }, now));
  });

  it("prefers apps over settings over files at equal scores", () => {
    const mixed: Candidate[] = [
      { id: "f", name: "Display", kind: "file" },
      { id: "s", name: "Display", kind: "setting" },
      { id: "a", name: "Display", kind: "app" },
    ];
    expect(rank("display", mixed, [], 0).map((c) => c.kind)).toEqual(["app", "setting", "file"]);
  });
});
