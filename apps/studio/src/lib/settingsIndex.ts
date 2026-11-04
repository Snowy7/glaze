// Every setting Studio has, as data: what search finds and where it leads. A setting's `id` is the
// `data-setting` anchor on its control; search goes to the route, opens a closed "More options"
// around it, scrolls to it and flashes it. Add a line here for every new setting.

import type { Route } from "./nav";

export type SettingEntry = {
  id: string;
  title: string;
  /** Where it lives, in words: "Widgets › Look". */
  section: string;
  route: Route;
  /** Other words people use for it. */
  keywords?: string;
};

const look = (field: string, title: string, keywords?: string): SettingEntry => ({
  id: `widgets.look.${field}`,
  title,
  section: "Widgets › All widgets",
  route: { page: "widgets" },
  keywords: `widget ${keywords ?? ""}`,
});

const windows = (id: string, title: string, keywords?: string): SettingEntry => ({ id: `windows.${id}`, title, section: "Windows", route: { page: "windows" }, keywords });
const menu = (id: string, title: string, keywords?: string): SettingEntry => ({ id: `menu.${id}`, title, section: "Windows › Right-click menu", route: { page: "windows", select: "menu" }, keywords: `context menu ${keywords ?? ""}` });
const settings = (id: string, title: string, keywords?: string): SettingEntry => ({ id: `settings.${id}`, title, section: "Settings", route: { page: "settings" }, keywords });
/** A Bars setting: `id` is its path in the file (see BarSettings), `bar` the bar whose inspector holds it. */
const bars = (id: string, title: string, keywords: string, bar?: "top" | "dock"): SettingEntry => ({
  id: `bars.${id}`,
  title,
  section: bar === "top" ? "Bars › Top bar" : bar === "dock" ? "Bars › Bottom bar" : "Bars",
  route: { page: "bars", ...(bar && { select: bar }) },
  keywords,
});

export const SETTINGS: readonly SettingEntry[] = [
  look("style", "Widget style", "minimal depth neon paper aero design"),
  look("surface", "Widget surface", "background glass solid none"),
  look("tint", "Widget color", "tint background"),
  look("opacity", "Widget opacity", "transparency see-through"),
  look("blur", "Widget blur", "glass frosted"),
  look("radius", "Widget corners", "radius rounded"),
  look("text", "Widget text color", "light dark"),
  look("accent", "Widget accent color", "highlight"),
  look("font", "Widget font", "typeface"),
  look("scale", "Widget size", "scale bigger smaller zoom"),
  look("border", "Widget edge line", "border outline"),
  look("shadow", "Widget text shadow", "readable"),
  { id: "widgets.overlay", title: "Always-on-top widgets", section: "Widgets › All widgets", route: { page: "widgets" }, keywords: "overlay topmost float above windows media pill" },
  { id: "widgets.add", title: "Add a widget", section: "Widgets", route: { page: "widgets", adding: true }, keywords: "new place install catalog" },

  windows("accent", "Accent color", "color highlight taskbar start"),
  windows("mode", "Light or dark apps", "dark mode light mode theme"),
  windows("corners", "Window corners", "rounded square radius"),
  windows("backdrop", "Window material", "mica acrylic backdrop transparency blur"),
  windows("cursors", "Mouse pointer", "cursor arrow"),
  windows("border", "Window edge color", "border outline"),
  windows("caption", "Title bar color", "caption header"),
  windows("captionText", "Title bar text color", "caption text"),
  menu("enabled", "Desktop right-click menu", "desktop menu"),
  menu("items", "Right-click menu items", "reorder hide"),
  menu("custom", "Add your own menu item", "command program shortcut"),
  menu("classic", "Full menus in Explorer", "classic show more options windows 11"),

  settings("autostart", "Start with Windows", "startup sign in login boot"),
  settings("snapshot", "Show the theme while Windows starts", "static wallpaper still startup"),
  settings("battery", "Pause on battery", "power laptop unplugged"),
  settings("fullscreen", "Pause behind fullscreen apps", "games video performance gpu"),
  settings("engine", "Start or stop mypaper", "engine running quit"),
  settings("panic", "Emergency stop hotkey", "panic restore keyboard shortcut"),
  settings("parts", "Parts of mypaper", "components processes installed"),

  { id: "wallpaper.interactive", title: "Wallpaper reacts to the mouse", section: "Desktop › Wallpaper", route: { page: "desktop", select: { kind: "wallpaper" } }, keywords: "interactive web scene camera" },

  bars("preset", "Bar pairing", "preset macos windows 11 chromeos islands neon floating style"),
  bars("top.enabled", "Show the top bar", "menu bar on off", "top"),
  bars("top.design", "Top bar design", "menu bar style look macos windows", "top"),
  bars("top.position", "Top bar edge", "menu bar bottom position", "top"),
  bars("top.height", "Top bar height", "menu bar size", "top"),
  bars("top.layout", "Top bar shape", "strip floating islands", "top"),
  bars("top.modules", "Top bar modules", "clock tray battery volume network media add remove reorder", "top"),
  bars("top.clock.seconds", "Clock seconds", "time format", "top"),
  bars("dock.enabled", "Show the bottom bar", "dock taskbar on off", "dock"),
  bars("dock.design", "Bottom bar design", "dock taskbar style look macos windows chromeos", "dock"),
  bars("dock.style", "Bottom bar style", "dock floating taskbar", "dock"),
  bars("dock.position", "Bottom bar edge", "dock left right top position", "dock"),
  bars("dock.size", "Dock icon size", "bottom bar bigger smaller", "dock"),
  bars("dock.autohide", "Auto-hide the dock", "bottom bar hide", "dock"),
  bars("dock.magnify", "Dock magnify", "zoom bottom bar", "dock"),
  bars("dock.pinned", "Pinned apps", "dock bottom bar pin programs shortcuts", "dock"),
  bars("dock.indicator", "Running indicator", "dock dot bar glow", "dock"),
  bars("dock.preview", "Window previews", "dock thumbnails hover", "dock"),
  bars("look.scheme", "Bar light or dark", "dark mode light mode color", "top"),
  bars("look.glass.style", "Bar glass", "liquid frosted blur transparency material", "top"),
  bars("animation.enabled", "Bar animations", "motion speed spring curve", "top"),
  bars("taskbar.mode", "Windows taskbar", "hide auto-hide keep"),
  bars("launcher.layout", "Start menu layout", "launcher grid compact dashboard"),
  bars("launcher.winKey", "Windows key opens the Start menu", "launcher keyboard"),
  bars("calendar.zones", "Other time zones", "calendar world clock"),
  bars("calendar.firstDay", "Week starts on", "calendar monday sunday"),
  bars("icons", "Icon packs", "app icons glyphs"),
  bars("animation.reduceMotion", "Reduce motion", "animation accessibility"),
  bars("performance", "Save power", "performance battery blur animations"),
];

/** Whether every word of `query` appears in the entry's title, section or keywords. */
export function matches(query: string, ...fields: (string | undefined)[]): boolean {
  const haystack = fields.join(" ").toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter((word) => word !== "")
    .every((word) => haystack.includes(word));
}
