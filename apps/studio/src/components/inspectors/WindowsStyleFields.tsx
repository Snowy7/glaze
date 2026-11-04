import { FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";

import { api, pickFolder, type Backdrop, type Corners, type WindowsStyle } from "../../lib/api";
import { ColorField, IconButton, InspectorSection, More, Prop, Segmented, Select, TilePicker, type Tile } from "../ui";

// A theme's `windows` section (docs/windows-styling.md): every field is either left as Windows has
// it (null, "As is") or set. Looks set several at once; the pointer has its own picker.

export const NO_STYLE: WindowsStyle = { accent: null, border: null, caption: null, caption_text: null, corners: null, backdrop: null, dark_mode: null, cursors: null };

const LOOK_IDS = ["as-is", "wallpaper", "dark", "light", "glass", "sharp"] as const;
type LookId = (typeof LOOK_IDS)[number];

/** One-click looks. Each sets the fields it names and leaves the pointer alone. */
const LOOKS: Record<LookId, { label: string; hint: string; style: Omit<WindowsStyle, "cursors"> }> = {
  "as-is": { label: "As is", hint: "Leave Windows as it is", style: { ...NO_STYLE } },
  wallpaper: { label: "Wallpaper", hint: "Accent and borders from the wallpaper", style: { ...NO_STYLE, accent: "auto", border: "auto" } },
  dark: { label: "Dark", hint: "Dark apps, Mica, round corners", style: { ...NO_STYLE, accent: "auto", dark_mode: true, backdrop: "mica", corners: "round" } },
  light: { label: "Light", hint: "Light apps, Mica, round corners", style: { ...NO_STYLE, accent: "auto", dark_mode: false, backdrop: "mica", corners: "round" } },
  glass: { label: "Glass", hint: "Dark apps, Acrylic, wallpaper borders", style: { ...NO_STYLE, accent: "auto", border: "auto", dark_mode: true, backdrop: "acrylic", corners: "round" } },
  sharp: { label: "Sharp", hint: "Square corners, no material, wallpaper borders", style: { ...NO_STYLE, dark_mode: true, corners: "square", backdrop: "none", border: "auto" } },
};

function lookOf(style: WindowsStyle): LookId | null {
  const { cursors: _pointer, ...rest } = style;
  return LOOK_IDS.find((id) => JSON.stringify(LOOKS[id].style) === JSON.stringify(rest)) ?? null;
}

const CORNER_OPTIONS: readonly { value: Corners | "as-is"; label: string }[] = [
  { value: "as-is", label: "As is" },
  { value: "round", label: "Round" },
  { value: "small", label: "Small" },
  { value: "square", label: "Square" },
];

const BACKDROPS: readonly { value: Backdrop | "as-is"; label: string }[] = [
  { value: "as-is", label: "As is" },
  { value: "mica", label: "Mica" },
  { value: "acrylic", label: "Acrylic" },
  { value: "tabbed", label: "Mica, tabbed" },
  { value: "none", label: "None" },
  { value: "auto", label: "Windows decides" },
];

type Props = {
  style: WindowsStyle;
  onChange: (next: WindowsStyle, label: string) => void;
  disabled?: boolean;
};

export function WindowsStyleFields({ style, onChange, disabled = false }: Props) {
  const set = <K extends keyof WindowsStyle>(key: K, value: WindowsStyle[K], label: string) => !disabled && onChange({ ...style, [key]: value }, label);
  const reset = (key: keyof WindowsStyle, label: string) => (style[key] === null ? undefined : () => set(key, null, `Reset ${label}`));

  return (
    <>
      <InspectorSection title="Look">
        <TilePicker<LookId>
          label="Window look"
          value={lookOf(style)}
          options={LOOK_IDS.map((id): Tile<LookId> => ({ value: id, label: LOOKS[id].label, hint: LOOKS[id].hint, preview: <MiniWindow style={{ ...LOOKS[id].style, cursors: null }} /> }))}
          onChange={(id) => !disabled && onChange({ ...LOOKS[id].style, cursors: style.cursors }, `Use the ${LOOKS[id].label.toLowerCase()} look`)}
          columns={3}
        />
        <Prop id="windows.accent" label="Accent color" onReset={reset("accent", "accent color")} stacked>
          <PaintChoice label="Accent color" value={style.accent} auto onChange={(v) => set("accent", v, "Change accent color")} />
        </Prop>
        <Prop id="windows.mode" label="Apps" onReset={reset("dark_mode", "light or dark")}>
          <Segmented<"as-is" | "dark" | "light">
            label="Light or dark apps"
            hideLabel
            size="sm"
            value={style.dark_mode === null ? "as-is" : style.dark_mode ? "dark" : "light"}
            options={[
              { value: "as-is", label: "As is" },
              { value: "dark", label: "Dark" },
              { value: "light", label: "Light" },
            ]}
            onChange={(v) => set("dark_mode", v === "as-is" ? null : v === "dark", "Change light or dark")}
          />
        </Prop>
        <Prop id="windows.corners" label="Corners" onReset={reset("corners", "corners")} stacked>
          <Segmented<Corners | "as-is">
            label="Window corners"
            hideLabel
            size="sm"
            className="w-full"
            value={style.corners === null || style.corners === "default" ? "as-is" : style.corners}
            options={CORNER_OPTIONS}
            onChange={(v) => set("corners", v === "as-is" ? null : v, "Change corners")}
          />
        </Prop>
        <Prop id="windows.backdrop" label="Material" onReset={reset("backdrop", "material")}>
          <Select<Backdrop | "as-is"> label="Window material" className="w-40" size="sm" value={style.backdrop ?? "as-is"} options={BACKDROPS} onChange={(v) => set("backdrop", v === "as-is" ? null : v, "Change material")} />
        </Prop>
      </InspectorSection>
      <InspectorSection title="Pointer">
        <PointerPicker value={style.cursors} onChange={(v) => set("cursors", v, "Change pointer")} />
      </InspectorSection>
      <More>
        <Prop id="windows.border" label="Window edge" onReset={reset("border", "window edge")} stacked>
          <PaintChoice label="Window edge" value={style.border} auto onChange={(v) => set("border", v, "Change window edge")} />
        </Prop>
        <Prop id="windows.caption" label="Title bar" onReset={reset("caption", "title bar")} stacked>
          <PaintChoice label="Title bar" value={style.caption} onChange={(v) => set("caption", v, "Change title bar")} />
        </Prop>
        <Prop id="windows.captionText" label="Title text" onReset={reset("caption_text", "title text")} stacked>
          <PaintChoice label="Title text" value={style.caption_text} onChange={(v) => set("caption_text", v, "Change title text")} />
        </Prop>
      </More>
    </>
  );
}

const isHex = (value: string | null): value is string => value !== null && /^#[0-9a-f]{6}$/i.test(value);

/** As is, From wallpaper (when `auto`), or a picked color. */
function PaintChoice({ label, value, auto = false, onChange }: { label: string; value: string | null; auto?: boolean; onChange: (value: string | null) => void }) {
  const mode = value === null ? "as-is" : value === "auto" ? "auto" : "color";
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Segmented<"as-is" | "auto" | "color">
        label={label}
        hideLabel
        size="sm"
        value={mode}
        options={[{ value: "as-is", label: "As is" }, ...(auto ? [{ value: "auto" as const, label: "From wallpaper" }] : []), { value: "color", label: "Pick" }]}
        onChange={(next) => onChange(next === "as-is" ? null : next === "auto" ? "auto" : isHex(value) ? value : "#7a8599")}
      />
      {mode === "color" && <ColorField label={`${label} color`} value={isHex(value) ? value : "#7a8599"} onChange={onChange} />}
    </div>
  );
}

type PointerId = "as-is" | "mypaper-dark" | "mypaper-light" | "other";

/** Windows' own pointer, the two built-in sets, or another set (a folder of cursors). */
function PointerPicker({ value, onChange }: { value: string | null; onChange: (value: string | null) => void }) {
  const [sets, setSets] = useState<string[]>([]);
  useEffect(() => {
    let current = true;
    void api.listCursorSets().then((list) => current && setSets(list), () => undefined);
    return () => {
      current = false;
    };
  }, []);
  const chosen: PointerId = value === null ? "as-is" : value === "mypaper-dark" || value === "mypaper-light" ? value : "other";

  async function pick(next: PointerId) {
    if (next === "as-is") return onChange(null);
    if (next !== "other") return onChange(next);
    if (chosen === "other") return;
    const first = sets[0];
    if (first) return onChange(first);
    const folder = await pickFolder();
    if (folder) onChange(folder);
  }

  const tile = (id: PointerId, label: string, hint: string): Tile<PointerId> => ({ value: id, label, hint, preview: <PointerTile kind={id} /> });
  return (
    <div data-setting="windows.cursors" className="setting-anchor flex flex-col gap-2.5">
      <TilePicker<PointerId>
        label="Pointer"
        value={chosen}
        options={[tile("as-is", "As is", "Windows' own pointer"), tile("mypaper-dark", "Dark", "A dark pointer with a white edge"), tile("mypaper-light", "Light", "A light pointer with a dark edge"), tile("other", "Other", "A set of your own")]}
        onChange={(next) => void pick(next)}
        columns={4}
        aspect="aspect-square"
      />
      {chosen === "other" && value !== null && (
        <div className="flex items-center gap-2">
          <Select<string>
            label="Pointer set"
            size="sm"
            className="min-w-0 flex-1"
            value={sets.includes(value) ? value : ""}
            options={[...(sets.includes(value) ? [] : [{ value: "", label: value.split(/[\\/]/).pop() ?? value }]), ...sets.map((set) => ({ value: set, label: set }))]}
            onChange={(next) => next !== "" && onChange(next)}
          />
          <IconButton icon={FolderOpen} size="sm" label="Choose a folder of cursors" onClick={() => void pickFolder().then((folder) => folder && onChange(folder))} />
        </div>
      )}
    </div>
  );
}

function PointerTile({ kind }: { kind: PointerId }) {
  const dark = kind === "mypaper-dark";
  return (
    <span className="ap-stage absolute inset-0 grid place-items-center" data-wallpaper={kind === "mypaper-light" ? "dark" : "light"}>
      <svg viewBox="0 0 24 24" className="size-7 drop-shadow-[0_1px_2px_rgb(0_0_0/0.5)]" aria-hidden>
        <path
          d="M5 3 L5 18.5 L9 15 L11.8 21 L14.2 19.9 L11.5 14.2 L17 14.2 Z"
          fill={dark ? "#111" : "#fff"}
          stroke={dark ? "#fff" : "#111"}
          strokeWidth={1.3}
          strokeLinejoin="round"
          strokeDasharray={kind === "other" ? "2 1.4" : undefined}
        />
      </svg>
    </span>
  );
}

/** A window in a tile: title bar, a pane and an accent button, painted with a look. */
function MiniWindow({ style }: { style: WindowsStyle }) {
  const light = style.dark_mode === false;
  const radius = style.corners === "square" ? 0 : style.corners === "small" ? 2 : 4;
  const accent = isHex(style.accent) ? style.accent : style.accent === "auto" ? "#9aa4b8" : light ? "#777" : "#bbb";
  const edge = style.border === "auto" ? "#9aa4b8" : light ? "rgb(0 0 0 / 0.2)" : "rgb(255 255 255 / 0.18)";
  const glass = style.backdrop === "mica" || style.backdrop === "acrylic" || style.backdrop === "tabbed";
  return (
    <span className="ap-stage absolute inset-0 grid place-items-center" data-wallpaper="dark">
      <span
        className="block h-[64%] w-[76%] overflow-hidden"
        style={{ borderRadius: radius, background: light ? `rgb(243 243 243 / ${glass ? 0.82 : 1})` : `rgb(32 32 32 / ${glass ? 0.78 : 1})`, boxShadow: `0 0 0 1px ${edge}, 0 4px 10px rgb(0 0 0 / 0.4)`, backdropFilter: glass ? "blur(6px)" : undefined }}
      >
        <span className="block h-[22%]" style={{ background: light ? "rgb(0 0 0 / 0.05)" : "rgb(255 255 255 / 0.04)" }} />
        <span className="m-[10%] mt-[12%] block h-[22%] w-[38%] rounded-[2px]" style={{ background: accent }} />
      </span>
    </span>
  );
}
