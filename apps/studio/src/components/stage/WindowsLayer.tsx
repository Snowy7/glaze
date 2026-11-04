import { ChevronRight, File, Folder, HardDrive, Image, Music, Star } from "lucide-react";
import { useState, type CSSProperties } from "react";

import type { Corners, MenuBuiltin, MenuEntry, WindowsStyle } from "../../lib/api";
import type { WindowsTarget } from "../../lib/nav";
import { Outline } from "./Stage";

// A sample window, the desktop's right-click menu and the pointer, drawn on the stage with the
// theme's Windows values, so a change to the accent, title bar, corners, material or pointer shows
// where it will. (The real ones are Windows' own; this is the closest honest picture.)

const RADIUS: Record<Corners, number> = { default: 8, round: 8, small: 4, square: 0 };

/** What "From wallpaper" stands for in the picture: the wallpaper's own color isn't known here. */
const AUTO_ACCENT = "#9aa4b8";

export const MENU_LABELS: Record<MenuBuiltin, string> = {
  separator: "Separator",
  view: "View",
  sort: "Sort by",
  refresh: "Refresh",
  new: "New",
  paste: "Paste",
  wallpaper: "Change wallpaper",
  widgets: "Edit widgets",
  display: "Display settings",
  personalize: "Personalize",
  terminal: "Open in Terminal",
  more: "Show more options",
};

const isHex = (value: string | null): value is string => value !== null && /^#[0-9a-f]{6}$/i.test(value);

type Props = {
  screen: { width: number; height: number };
  style: WindowsStyle;
  /** The menu's items, or null when mypaper's menu is off (Windows' own shows). */
  menu: readonly MenuEntry[] | null;
  selected: WindowsTarget | null;
  onSelect: ((target: WindowsTarget) => void) | null;
};

export function WindowsLayer({ screen, style, menu, selected, onSelect }: Props) {
  const [hovered, setHovered] = useState<WindowsTarget | null>(null);
  const light = style.dark_mode === false;
  const accent = isHex(style.accent) ? style.accent : style.accent === "auto" ? AUTO_ACCENT : light ? "#5b5b5b" : "#cfcfcf";
  const radius = RADIUS[style.corners ?? "default"];
  const window = { x: screen.width * 0.17, y: screen.height * 0.16, width: screen.width * 0.47, height: screen.height * 0.56 };
  const menuAt = { x: screen.width * 0.69, y: screen.height * 0.2 };
  const menuBox = { ...menuAt, width: 250, height: menuHeight(menu ?? []) };

  const pick = (target: WindowsTarget) =>
    onSelect
      ? {
          onPointerEnter: () => setHovered(target),
          onPointerLeave: () => setHovered(null),
          onClick: () => onSelect(target),
        }
      : {};

  return (
    <>
      <div className="absolute" style={{ left: window.x, top: window.y, width: window.width, height: window.height }} {...pick("style")}>
        <SampleWindow style={style} light={light} accent={accent} radius={radius} />
        <Outline box={{ x: 0, y: 0, width: window.width, height: window.height }} selected={selected === "style"} hovered={hovered === "style"} label="Windows" radius={radius} />
      </div>
      {menu && (
        <div className="absolute" style={{ left: menuBox.x, top: menuBox.y, width: menuBox.width, height: menuBox.height }} {...pick("menu")}>
          <SampleMenu items={menu} />
          <Outline box={{ x: 0, y: 0, width: menuBox.width, height: menuBox.height }} selected={selected === "menu"} hovered={hovered === "menu"} label="Right-click menu" radius={10} />
        </div>
      )}
      <Pointer at={{ x: menuAt.x - 6, y: menuAt.y - 8 }} set={style.cursors} />
    </>
  );
}

function SampleWindow({ style, light, accent, radius }: { style: WindowsStyle; light: boolean; accent: string; radius: number }) {
  const caption = isHex(style.caption) ? style.caption : null;
  const captionText = isHex(style.caption_text) ? style.caption_text : light ? "#1b1b1b" : "#f2f2f2";
  const border = isHex(style.border) ? style.border : style.border === "auto" ? AUTO_ACCENT : light ? "rgb(0 0 0 / 0.18)" : "rgb(255 255 255 / 0.14)";
  const backdrop = style.backdrop ?? "auto";
  const see = backdrop === "acrylic" ? 0.55 : backdrop === "mica" || backdrop === "auto" ? 0.78 : backdrop === "tabbed" ? 0.84 : 1;
  const base = light ? `rgb(243 243 243 / ${see})` : `rgb(32 32 32 / ${see})`;
  const pane = light ? "rgb(255 255 255 / 0.7)" : "rgb(255 255 255 / 0.045)";
  const ink = light ? "#1b1b1b" : "#f2f2f2";
  const muted = light ? "rgb(0 0 0 / 0.55)" : "rgb(255 255 255 / 0.55)";
  const shell: CSSProperties = {
    borderRadius: radius,
    background: base,
    backdropFilter: see < 1 ? `blur(${backdrop === "acrylic" ? 26 : 60}px) saturate(${backdrop === "acrylic" ? 1.3 : 1.8})` : undefined,
    boxShadow: `0 0 0 1px ${border}, 0 24px 60px -12px rgb(0 0 0 / 0.55)`,
    color: ink,
  };
  const files = [
    { icon: Folder, name: "Projects", detail: "Folder" },
    { icon: Image, name: "Wallpapers", detail: "Folder" },
    { icon: Music, name: "Mixtape.flac", detail: "38.2 MB" },
    { icon: File, name: "Notes.txt", detail: "4 KB" },
    { icon: File, name: "Budget 2026.xlsx", detail: "61 KB" },
  ];
  return (
    <div className="absolute inset-0 flex flex-col overflow-hidden font-sans" style={shell}>
      <div className="flex h-10 shrink-0 items-center gap-2 px-4 text-[13px]" style={{ background: caption ?? "transparent", color: captionText }}>
        <Folder className="size-4" />
        <span className="font-medium">Documents</span>
        <span className="ml-auto flex items-center gap-6 pr-1 text-[15px] tracking-widest opacity-80">
          <span>–</span>
          <span>▢</span>
          <span>✕</span>
        </span>
      </div>
      <div className="flex h-11 shrink-0 items-center gap-2 px-3">
        <span className="flex h-8 items-center rounded-md px-3.5 text-[13px] font-semibold" style={{ background: accent, color: "#fff" }}>
          New
        </span>
        <span className="h-8 flex-1 rounded-md px-3 text-[12.5px] leading-8" style={{ background: pane, color: muted }}>
          This PC › Documents
        </span>
      </div>
      <div className="flex min-h-0 flex-1 gap-1 p-2 pt-0">
        <div className="w-40 shrink-0 space-y-0.5 p-1 text-[12.5px]">
          {[Star, HardDrive, Folder, Image].map((Icon, i) => (
            <div key={i} className="flex h-8 items-center gap-2 rounded-md px-2" style={i === 2 ? { background: pane } : undefined}>
              <Icon className="size-4" style={{ color: i === 2 ? accent : muted }} />
              <span>{["Home", "This PC", "Documents", "Pictures"][i]}</span>
            </div>
          ))}
        </div>
        <div className="min-w-0 flex-1 rounded-lg p-1.5" style={{ background: pane }}>
          {files.map(({ icon: Icon, name, detail }, i) => (
            <div key={name} className="flex h-9 items-center gap-2.5 rounded-md px-2.5 text-[12.5px]" style={i === 1 ? { background: `color-mix(in srgb, ${accent} 28%, transparent)` } : undefined}>
              <Icon className="size-4 shrink-0" style={{ color: i < 2 ? accent : muted }} />
              <span className="min-w-0 flex-1 truncate">{name}</span>
              <span style={{ color: muted }}>{detail}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

const ROW = 30;
const SEPARATOR = 9;

function menuHeight(items: readonly MenuEntry[]): number {
  return 12 + items.reduce((sum, item) => sum + (item === "separator" ? SEPARATOR : ROW), 0);
}

const SUBMENU: ReadonlySet<string> = new Set(["view", "sort", "new"]);

function SampleMenu({ items }: { items: readonly MenuEntry[] }) {
  return (
    <div
      className="absolute inset-0 overflow-hidden p-1.5 font-sans text-[12.5px] text-white"
      style={{ borderRadius: 10, background: "rgb(28 28 30 / 0.92)", boxShadow: "inset 0 0 0 1px rgb(255 255 255 / 0.1), 0 18px 40px -10px rgb(0 0 0 / 0.65)" }}
    >
      {items.map((item, i) =>
        item === "separator" ? (
          <div key={i} className="mx-2 my-1 h-px bg-white/12" />
        ) : (
          <div key={i} className="flex h-[30px] items-center gap-2 rounded-md px-2.5" style={i === 0 ? { background: "rgb(255 255 255 / 0.08)" } : undefined}>
            <span className="min-w-0 flex-1 truncate">{typeof item === "string" ? MENU_LABELS[item] : item.label}</span>
            {typeof item === "string" && SUBMENU.has(item) && <ChevronRight className="size-3.5 opacity-60" />}
          </div>
        ),
      )}
    </div>
  );
}

/** The pointer: the built-in dark or light set, or Windows' own arrow. */
function Pointer({ at, set }: { at: { x: number; y: number }; set: string | null }) {
  const dark = set === "mypaper-dark";
  const custom = set !== null && !set.startsWith("mypaper-");
  return (
    <svg viewBox="0 0 24 24" className="pointer-events-none absolute z-30 drop-shadow-[0_2px_3px_rgb(0_0_0/0.5)]" style={{ left: at.x, top: at.y, width: 30, height: 30 }} aria-hidden>
      <path
        d="M4 2.5 L4 19 L8.6 15.1 L11.6 21.6 L14.3 20.4 L11.4 14 L17.6 14 Z"
        fill={dark ? "#121212" : "#ffffff"}
        stroke={dark ? "#ffffff" : "#111111"}
        strokeWidth={dark ? 1.4 : 1.1}
        strokeLinejoin="round"
        strokeDasharray={custom ? "2 1.4" : undefined}
      />
    </svg>
  );
}
