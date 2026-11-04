import { describe, expect, it } from "vitest";
import type { SettingDef } from "./protocol";
import {
  decodeColor,
  encodeColor,
  fileName,
  formatReadout,
  isFontName,
  optionLabel,
  optionText,
  optionValue,
  sections,
  selectStyle,
} from "./settings-model";
import { placeLine, readCandidates, readPlace } from "./settings-model";

const str = (key: string, group?: string): SettingDef => ({
  key,
  type: "string",
  label: key,
  default: "",
  ...(group !== undefined && { group }),
});

describe("sections", () => {
  it("puts ungrouped settings first, then one section per run of a group", () => {
    const result = sections([str("a", "Look"), str("b"), str("c", "Look"), str("d", "Data"), str("e"), str("f", "Look")]);
    expect(result.map((s) => [s.group, s.settings.map((x) => x.key)])).toEqual([
      [undefined, ["b", "e"]],
      ["Look", ["a", "c"]],
      ["Data", ["d"]],
      ["Look", ["f"]], // a group that comes back is a new section
    ]);
  });

  it("has no ungrouped section when everything has a group, and none at all when empty", () => {
    expect(sections([str("a", "X")]).map((s) => s.group)).toEqual(["X"]);
    expect(sections([])).toEqual([]);
  });
});

describe("select options", () => {
  it("reads plain strings and objects alike", () => {
    expect(optionValue("a")).toBe("a");
    expect(optionLabel("a")).toBe("a");
    expect(optionValue({ value: "v", label: "Label", icon: "★" })).toBe("v");
    expect(optionLabel({ value: "v" })).toBe("v");
    expect(optionText({ value: "v", label: "Label", icon: "★" })).toBe("★ Label");
    expect(optionText({ value: "v", label: "Label" })).toBe("Label");
  });

  it("picks the control by count and icons", () => {
    const icon = (value: string) => ({ value, icon: "★" });
    expect(selectStyle(["a", "b", "c"])).toBe("segmented");
    expect(selectStyle(["a", "b", "c", "d"])).toBe("dropdown");
    expect(selectStyle([icon("a"), icon("b"), icon("c"), icon("d"), icon("e")])).toBe("icons");
    expect(selectStyle([icon("a"), "b", "c", "d", "e", "f"])).toBe("dropdown"); // six is too many for a bar
    expect(selectStyle([{ value: "a", label: "A" }, "b"])).toBe("segmented"); // labels alone are not icons
  });
});

describe("color with alpha", () => {
  it("decodes #rrggbb always, and #rrggbbaa only with alpha", () => {
    expect(decodeColor("#FF8000", false)).toEqual({ rgb: "#ff8000", alpha: 1 });
    expect(decodeColor("#ff800080", true)).toEqual({ rgb: "#ff8000", alpha: 128 / 255 });
    expect(decodeColor("#ff8000", true)).toEqual({ rgb: "#ff8000", alpha: 1 });
    expect(decodeColor("#ff800080", false)).toBeUndefined();
    expect(decodeColor("red", true)).toBeUndefined();
    expect(decodeColor(3, true)).toBeUndefined();
  });

  it("encodes 8 digits with alpha (even when opaque) and 6 without", () => {
    expect(encodeColor({ rgb: "#FF8000", alpha: 1 }, true)).toBe("#ff8000ff");
    expect(encodeColor({ rgb: "#ff8000", alpha: 0.5 }, true)).toBe("#ff800080");
    expect(encodeColor({ rgb: "#ff8000", alpha: 0 }, true)).toBe("#ff800000");
    expect(encodeColor({ rgb: "#ff8000", alpha: 0.5 }, false)).toBe("#ff8000");
  });

  it("round-trips every opacity step", () => {
    for (let byte = 0; byte <= 255; byte++) {
      const encoded = encodeColor({ rgb: "#123456", alpha: byte / 255 }, true);
      const decoded = decodeColor(encoded, true);
      expect(encodeColor(decoded ?? { rgb: "", alpha: 0 }, true)).toBe(encoded);
    }
  });
});

describe("readouts, fonts and files", () => {
  it("appends the unit and trims float noise", () => {
    expect(formatReadout(12, "px")).toBe("12px");
    expect(formatReadout(0.1 + 0.2, "%")).toBe("0.3%");
    expect(formatReadout(7, undefined)).toBe("7");
  });

  it("accepts the font names the host accepts", () => {
    expect(isFontName("Cascadia Code")).toBe(true);
    expect(isFontName("")).toBe(true);
    for (const bad of ["a;b", "a{b", "a}b", "a<b", "a>b", "a\\b", "x".repeat(201)]) expect(isFontName(bad)).toBe(false);
    expect(isFontName("x".repeat(200))).toBe(true);
  });

  it("takes the last path segment of a served URL, decoded", () => {
    expect(fileName("https://photo.widget.mypaper/__data/photo-1/My%20Pic.png")).toBe("My Pic.png");
    expect(fileName("https://photo.widget.mypaper/__data/photo-1/a.png?v=2#x")).toBe("a.png");
    expect(fileName("")).toBe("");
    expect(fileName("/x/%E0%A4%A")).toBe("%E0%A4%A"); // not valid percent-encoding: shown as is
  });
});

describe("place settings", () => {
  it("reads a saved place, and anything else as automatic", () => {
    expect(readPlace({ name: "Bergen", lat: 60.39, lon: 5.32, extra: 1 })).toEqual({ name: "Bergen", lat: 60.39, lon: 5.32 });
    expect(readPlace({ name: "Oslo" })).toEqual({ name: "Oslo" });
    for (const auto of [null, "", { name: "" }, 3, { lat: 1, lon: 2 }]) expect(readPlace(auto)).toBeNull();
  });

  it("keeps well-formed candidates and says where each one is", () => {
    const found = readCandidates([
      { name: "Springfield", admin1: "Illinois", country: "United States", lat: 39.8, lon: -89.6 },
      { name: "Nowhere", lat: "x", lon: 1 },
      null,
    ]);
    expect(found.map(placeLine)).toEqual(["Springfield, Illinois, United States"]);
    expect(placeLine({ name: "Monaco", admin1: "Monaco", country: "Monaco", lat: 43.7, lon: 7.4 })).toBe("Monaco");
    expect(readCandidates("nope")).toEqual([]);
  });
});
