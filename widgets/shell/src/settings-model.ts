// Pure helpers behind the settings form (docs/widgets.md, "Widget manifest"): grouping, select
// options, color encoding with alpha, file names. No DOM, so they are unit-tested.

import type { SelectOption, SettingDef, Place, PlaceCandidate } from "./protocol";

// ---- groups ----

export interface Section {
  /** The heading, or `undefined` for the ungrouped settings at the top. */
  group: string | undefined;
  settings: SettingDef[];
}

/**
 * The form's sections: ungrouped settings first (in order), then one section per run of
 * consecutive settings with the same `group`. A group that comes back later is a new section.
 */
export function sections(settings: readonly SettingDef[]): Section[] {
  const result: Section[] = [];
  const ungrouped = settings.filter((s) => s.group === undefined);
  if (ungrouped.length > 0) result.push({ group: undefined, settings: ungrouped });
  for (const setting of settings) {
    if (setting.group === undefined) continue;
    const last = result.at(-1);
    if (last && last.group === setting.group) last.settings.push(setting);
    else result.push({ group: setting.group, settings: [setting] });
  }
  return result;
}

// ---- select options ----

export const optionValue = (option: SelectOption): string => (typeof option === "string" ? option : option.value);

export const optionLabel = (option: SelectOption): string =>
  typeof option === "string" ? option : (option.label ?? option.value);

export const optionIcon = (option: SelectOption): string | undefined =>
  typeof option === "string" ? undefined : option.icon;

/** Text for a dropdown entry: "icon label". */
export const optionText = (option: SelectOption): string => {
  const icon = optionIcon(option);
  return icon ? `${icon} ${optionLabel(option)}` : optionLabel(option);
};

/** A saved `place` value: a picked town, or null (automatic, or anything malformed). */
export function readPlace(value: unknown): Place | null {
  if (typeof value !== "object" || value === null || !("name" in value) || typeof value.name !== "string" || value.name === "") return null;
  const lat = "lat" in value && typeof value.lat === "number" ? value.lat : undefined;
  const lon = "lon" in value && typeof value.lon === "number" ? value.lon : undefined;
  return lat !== undefined && lon !== undefined ? { name: value.name, lat, lon } : { name: value.name };
}

/** The place a picked candidate is saved as: its coordinates, and where it is for showing. */
export function placeOf(c: PlaceCandidate): Place {
  return {
    name: c.name,
    lat: c.lat,
    lon: c.lon,
    ...(c.admin1 ? { region: c.admin1 } : {}),
    ...(c.country ? { country: c.country } : {}),
  };
}

/** A search answer as candidates, keeping only well-formed ones (the plugin is untrusted). */
export function readCandidates(value: unknown): PlaceCandidate[] {
  if (!Array.isArray(value)) return [];
  return value.filter(
    (c): c is PlaceCandidate =>
      typeof c === "object" && c !== null && typeof c.name === "string" && c.name !== "" && Number.isFinite(c.lat) && Number.isFinite(c.lon),
  );
}

/** A candidate as one line, "Maadi, Cairo, Egypt", so homonyms tell apart (no repeats, no blanks). */
export function placeLine(c: PlaceCandidate): string {
  const parts = [c.name, c.admin2, c.admin1, c.country].filter((p): p is string => typeof p === "string" && p.trim() !== "");
  return parts.filter((p, i) => parts.indexOf(p) === i).join(", ");
}

export type SelectStyle = "segmented" | "icons" | "dropdown";

/** Icon bar for up to 5 options with icons; text segments for up to 3 plain ones; else a dropdown. */
export function selectStyle(options: readonly SelectOption[]): SelectStyle {
  if (options.length <= 5 && options.some((o) => optionIcon(o) !== undefined)) return "icons";
  return options.length <= 3 ? "segmented" : "dropdown";
}

// ---- color ----

const HEX6 = /^#[0-9a-f]{6}$/i;
const HEX8 = /^#[0-9a-f]{8}$/i;

export const isHexColor = (value: string): boolean => HEX6.test(value);

export interface Rgba {
  /** `#rrggbb`, lowercase. */
  rgb: string;
  /** 0..1 */
  alpha: number;
}

/** Reads `#rrggbb`, or (with `alpha`) `#rrggbbaa`. `undefined` if it is neither. */
export function decodeColor(value: unknown, alpha: boolean): Rgba | undefined {
  if (typeof value !== "string") return undefined;
  if (HEX6.test(value)) return { rgb: value.toLowerCase(), alpha: 1 };
  if (alpha && HEX8.test(value)) {
    return { rgb: value.slice(0, 7).toLowerCase(), alpha: parseInt(value.slice(7), 16) / 255 };
  }
  return undefined;
}

/** `#rrggbbaa` when `alpha` is set (always, even when opaque), else `#rrggbb`. */
export function encodeColor({ rgb, alpha }: Rgba, withAlpha: boolean): string {
  if (!withAlpha) return rgb.toLowerCase();
  const byte = Math.round(Math.min(Math.max(alpha, 0), 1) * 255);
  return rgb.toLowerCase() + byte.toString(16).padStart(2, "0");
}

// ---- numbers and fonts ----

const pretty = (n: number) => String(Math.round(n * 1000) / 1000);

/** A slider's readout: the value with its unit suffix ("12px", "40%"). */
export const formatReadout = (value: number, unit: string | undefined): string => `${pretty(value)}${unit ?? ""}`;

/** Common fonts offered by the `font` setting. */
export const COMMON_FONTS = [
  "Segoe UI Variable",
  "Segoe UI",
  "Cascadia Code",
  "Consolas",
  "Georgia",
  "Bahnschrift",
  "Arial",
  "Verdana",
  "Trebuchet MS",
  "Times New Roman",
  "Courier New",
  "Impact",
  "Comic Sans MS",
] as const;

/** The host's rule for a `font` value: at most 200 characters, none of `; { } < > \`. */
export const isFontName = (value: string): boolean => value.length <= 200 && !/[;{}<>\\]/.test(value);

/** A CSS `font-family` value for previewing a font name in its own face. */
export const fontFamilyCss = (name: string): string => `"${name.replace(/["\\]/g, "")}", sans-serif`;

// ---- files ----

/** The file name of a served URL: its last path segment, decoded; `""` for no file. */
export function fileName(url: string): string {
  const path = url.split(/[?#]/, 1)[0] ?? "";
  const last = path.split("/").filter(Boolean).at(-1) ?? "";
  try {
    return decodeURIComponent(last);
  } catch {
    return last;
  }
}
