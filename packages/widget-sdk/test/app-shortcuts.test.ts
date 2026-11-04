import { describe, expect, it } from "vitest";
import { dockPose, fitIconSize, iconPixels, monogram, nameOf, peakScale, readShortcuts } from "../../../widgets/app-shortcuts/apps.js";

describe("app shortcuts list", () => {
  it("keeps items with a target and names the ones without a label", () => {
    const raw = [
      { label: " Files ", target: " C:\\Windows\\explorer.exe " },
      { label: "", target: "C:\\Windows\\System32\\notepad.exe" },
      { label: "No target", target: "  " },
      { target: 5 },
      null,
      "x",
    ];
    expect(readShortcuts(raw)).toEqual([
      { label: "Files", target: "C:\\Windows\\explorer.exe" },
      { label: "notepad", target: "C:\\Windows\\System32\\notepad.exe" },
    ]);
    expect(readShortcuts("nope")).toEqual([]);
  });

  it("caps the list", () => {
    const many = Array.from({ length: 40 }, (_, i) => ({ label: `a${i}`, target: `t${i}` }));
    expect(readShortcuts(many)).toHaveLength(24);
  });

  it("derives names from paths and app ids", () => {
    expect(nameOf("C:\\Windows\\System32\\calc.exe")).toBe("calc");
    expect(nameOf("shell:AppsFolder\\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel")).toBe(
      "immersivecontrolpanel",
    );
    expect(nameOf("https://github.com")).toBe("github.com");
    expect(monogram({ label: "terminal", target: "wt.exe" })).toBe("T");
    expect(monogram({ label: "", target: "C:\\x\\paint.exe" })).toBe("P");
  });
});

describe("icon request size", () => {
  it("asks for at least twice the drawn size, from a few steps", () => {
    expect([16, 24, 32, 40, 48, 64, 100].map((s) => iconPixels(s))).toEqual([32, 48, 64, 96, 96, 128, 256]);
    expect(iconPixels(300)).toBe(256);
  });

  it("covers a denser screen than 2x", () => {
    expect(iconPixels(32, 3)).toBe(96);
    expect(iconPixels(32, 1)).toBe(64);
  });
});

describe("icon size for the grid and list", () => {
  it("keeps the chosen size while everything fits", () => {
    expect(fitIconSize("grid", 6, 800, 300, true, 48)).toBe(48);
    expect(fitIconSize("list", 4, 400, 300, true, 48)).toBe(48);
  });

  it("shrinks until every shortcut fits, and stops at the minimum", () => {
    const size = fitIconSize("grid", 6, 380, 170, true, 64);
    expect(size).toBeLessThan(64);
    expect(size).toBeGreaterThanOrEqual(28);
    expect(fitIconSize("grid", 24, 200, 80, true, 64)).toBe(28);
  });

  it("fits more without labels", () => {
    expect(fitIconSize("grid", 8, 300, 120, false, 64)).toBeGreaterThan(fitIconSize("grid", 8, 300, 120, true, 64));
  });
});

describe("dock magnification", () => {
  const centers = [50, 100, 150, 200, 250];
  const shape = { base: 40, peak: 1.8, radius: 90 };

  it("leaves everything alone when the pointer is far away", () => {
    for (const pose of dockPose(centers, 900, shape)) expect(pose).toEqual({ scale: 1, shift: 0 });
  });

  it("grows the icon under the pointer to the peak, and its neighbours less and less", () => {
    const scales = dockPose(centers, 150, shape).map((p) => p.scale);
    expect(scales[2]).toBeCloseTo(1.8);
    expect(scales[1]).toBeGreaterThan(1);
    expect(scales[1]).toBeLessThan(scales[2] ?? 0);
    expect(scales[1]).toBeCloseTo(scales[3] ?? 0);
    expect(scales[0]).toBe(1);
  });

  it("pushes the neighbours apart symmetrically around the middle", () => {
    const pose = dockPose(centers, 150, shape);
    expect(pose[2]?.shift).toBeCloseTo(0);
    expect(pose[1]?.shift).toBeCloseTo(-(pose[3]?.shift ?? 0));
    expect(pose[0]?.shift).toBeCloseTo(-(pose[4]?.shift ?? 0));
    expect(pose[0]?.shift ?? 0).toBeLessThan(0);
  });

  it("keeps the dock's outer edges at half the added width, wherever the pointer is", () => {
    const pose = dockPose(centers, 60, shape);
    const added = pose.reduce((sum, p) => sum + shape.base * (p.scale - 1), 0);
    const first = pose[0];
    const last = pose[4];
    // The left edge of the first icon and the right edge of the last one moved out by half the total each.
    expect((first?.shift ?? 0) - (shape.base * ((first?.scale ?? 1) - 1)) / 2).toBeCloseTo(-added / 2);
    expect((last?.shift ?? 0) + (shape.base * ((last?.scale ?? 1) - 1)) / 2).toBeCloseTo(added / 2);
  });

  it("limits the peak to the room above the dock", () => {
    expect(peakScale(200, 48, 64, 28)).toBe(1.8);
    expect(peakScale(96, 40, 56, 24)).toBeCloseTo(1.4);
    expect(peakScale(70, 48, 64, 28)).toBe(1);
  });
});
