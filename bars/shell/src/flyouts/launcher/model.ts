// The launcher's data, shaped for its views: pins resolved against the app index, the A–Z list,
// what to recommend, and search results across apps, settings pages and files. Pure.

import type { LauncherApp, Pin, Recent, RecentFile } from "../protocol";
import { fold, rank, type Candidate } from "./fuzzy";
import type { SettingsPage } from "./settings-pages";

/** One thing the launcher can open. */
export interface Entry extends Candidate {
  /** What `run` opens. */
  path: string;
  icon?: string;
  /** A second line: the kind, or where a file is. */
  detail?: string;
}

export function appEntry(app: LauncherApp): Entry {
  return { id: app.id, name: app.name, path: app.path, kind: "app", ...(app.icon ? { icon: app.icon } : {}), ...(app.keywords ? { keywords: app.keywords } : {}) };
}

export function settingEntry(page: SettingsPage): Entry {
  return { id: page.uri, name: page.name, path: page.uri, kind: "setting", keywords: page.keywords, detail: "Settings" };
}

export function fileEntry(file: RecentFile): Entry {
  return { id: file.path, name: file.name, path: file.path, kind: "file", detail: "Recent file", ...(file.icon ? { icon: file.icon } : {}) };
}

const stem = (path: string): string =>
  (path.split(/[\\/]/).pop() ?? path).replace(/\.(lnk|exe|url)$/i, "");

/**
 * Pins as launcher entries: an index app with the same path, else the same name (taskbar pins are
 * shortcuts, the index lists AppsFolder paths), else the pin itself without an icon.
 */
export function resolvePins(pins: readonly Pin[], apps: readonly LauncherApp[]): Entry[] {
  const byPath = new Map(apps.map((a) => [a.path.toLowerCase(), a]));
  const byName = new Map(apps.map((a) => [fold(a.name), a]));
  return pins.map((pin) => {
    const name = pin.name ?? stem(pin.path);
    const app = byPath.get(pin.path.toLowerCase()) ?? byName.get(fold(name));
    return app ? { ...appEntry(app), path: pin.path } : { id: pin.path, name, path: pin.path, kind: "app" as const };
  });
}

/** Apps grouped by first letter, A–Z, with "#" for digits and symbols first. */
export function alphabetical(apps: readonly LauncherApp[]): { letter: string; apps: LauncherApp[] }[] {
  const groups = new Map<string, LauncherApp[]>();
  for (const app of [...apps].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }))) {
    const first = fold(app.name).charAt(0).toUpperCase();
    const letter = /\p{L}/u.test(first) ? first : "#";
    groups.set(letter, [...(groups.get(letter) ?? []), app]);
  }
  return [...groups.entries()]
    .sort(([a], [b]) => (a === "#" ? -1 : b === "#" ? 1 : a.localeCompare(b)))
    .map(([letter, list]) => ({ letter, apps: list }));
}

/** The apps run most lately, as entries (history ids are app ids). */
export function recentApps(recent: readonly Recent[], apps: readonly LauncherApp[], limit: number): Entry[] {
  const byId = new Map(apps.map((a) => [a.id, a]));
  return recent
    .flatMap((r) => {
      const app = byId.get(r.id);
      return app ? [appEntry(app)] : [];
    })
    .slice(0, limit);
}

/** Search across everything; apps first at equal scores, the user's history on top. */
export function search(
  query: string,
  sources: { apps: readonly LauncherApp[]; settings: readonly SettingsPage[]; files: readonly RecentFile[] },
  recent: readonly Recent[],
  now: number,
  limit = 30,
): Entry[] {
  const candidates = [...sources.apps.map(appEntry), ...sources.settings.map(settingEntry), ...sources.files.map(fileEntry)];
  return rank(query, candidates, recent, now).slice(0, limit);
}

/** Moves a selection by `step` through `count` items, wrapping; -1 means nothing is selected yet. */
export function step(selected: number, count: number, by: number): number {
  if (count === 0) return -1;
  if (selected < 0) return by > 0 ? 0 : count - 1;
  return (((selected + by) % count) + count) % count;
}

/** In a grid of `columns`, the item above/below/left/right of `selected` (clamped). */
export function gridStep(selected: number, count: number, columns: number, key: "up" | "down" | "left" | "right"): number {
  if (count === 0) return -1;
  if (selected < 0) return 0;
  const next =
    key === "left" ? selected - 1 : key === "right" ? selected + 1 : key === "up" ? selected - columns : selected + columns;
  return Math.max(0, Math.min(count - 1, next));
}
