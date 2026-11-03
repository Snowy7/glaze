import { Check, RotateCcw } from "lucide-react";
import { useEffect, useId, useState, type ReactNode } from "react";

import { api, errorText, type Backdrop, type Corners, type ThemeEntry, type WindowsStyle } from "../lib/api";
import { Button, Segmented } from "./ui";

const UNSET: WindowsStyle = {
  accent: null,
  border: null,
  caption: null,
  caption_text: null,
  corners: null,
  backdrop: null,
  dark_mode: null,
};

const same = (a: WindowsStyle, b: WindowsStyle) => JSON.stringify(a) === JSON.stringify(b);

const CORNERS: readonly { value: Corners; label: string }[] = [
  { value: "default", label: "Default" },
  { value: "round", label: "Round" },
  { value: "small", label: "Small" },
  { value: "square", label: "Square" },
];

const BACKDROPS: readonly { value: Backdrop; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "none", label: "None" },
  { value: "mica", label: "Mica" },
  { value: "acrylic", label: "Acrylic" },
  { value: "tabbed", label: "Tabbed" },
];

/** Edits the `windows` section of one user theme. Every setting can be "Don't change". */
export function StyleEditor({ theme }: { theme: ThemeEntry }) {
  const [saved, setSaved] = useState<WindowsStyle | null>(null);
  const [draft, setDraft] = useState<WindowsStyle>(UNSET);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [justSaved, setJustSaved] = useState(false);

  useEffect(() => {
    let current = true;
    api.getStyle(theme.dir).then(
      (style) => {
        if (!current) return;
        setSaved(style);
        setDraft(style);
      },
      (e: unknown) => current && setError(errorText(e)),
    );
    return () => {
      current = false;
    };
  }, [theme.dir]);

  function set<K extends keyof WindowsStyle>(key: K, value: WindowsStyle[K]) {
    setJustSaved(false);
    setDraft((style) => ({ ...style, [key]: value }));
  }

  async function save() {
    setSaving(true);
    setError(null);
    try {
      const style = await api.setStyle(theme.dir, draft);
      setSaved(style);
      setDraft(style);
      setJustSaved(true);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSaving(false);
    }
  }

  const dirty = saved !== null && !same(saved, draft);

  return (
    <>
      <div className="flex-1 overflow-y-auto px-6 py-5">
        <StylePreview style={draft} />

        <div className="mt-6 divide-y divide-line">
          <PaintRow label="Accent color" hint="Taskbar, Start and highlights." allowAuto value={draft.accent} onChange={(v) => set("accent", v)} />
          <PaintRow label="Window border" hint="The thin outline around windows." allowAuto value={draft.border} onChange={(v) => set("border", v)} />
          <PaintRow label="Title bar" hint="Background of window title bars." value={draft.caption} onChange={(v) => set("caption", v)} />
          <PaintRow label="Title bar text" hint="Title and caption button color." value={draft.caption_text} onChange={(v) => set("caption_text", v)} />
          <Row label="Corners" hint="How round window corners are.">
            <Choice label="Corners" value={draft.corners} options={CORNERS} onChange={(v) => set("corners", v)} />
          </Row>
          <Row label="Backdrop" hint="Material behind window content.">
            <Choice label="Backdrop" value={draft.backdrop} options={BACKDROPS} onChange={(v) => set("backdrop", v)} />
          </Row>
          <Row label="Dark mode" hint="Force apps and Windows dark or light.">
            <Choice
              label="Dark mode"
              value={draft.dark_mode === null ? null : draft.dark_mode ? "dark" : "light"}
              options={[
                { value: "dark", label: "Dark" },
                { value: "light", label: "Light" },
              ]}
              onChange={(v) => set("dark_mode", v === null ? null : v === "dark")}
            />
          </Row>
        </div>
      </div>

      <footer className="border-t border-line bg-sidebar px-6 py-4">
        {error && (
          <p role="alert" className="selectable mb-3 text-[13px] text-ink">
            {error}
          </p>
        )}
        <div className="flex items-center gap-2">
          <Button variant="ghost" icon={RotateCcw} disabled={same(draft, UNSET)} onClick={() => setDraft(UNSET)}>
            Clear all
          </Button>
          <span className="ml-auto flex items-center gap-1.5 text-xs text-ink-3" aria-live="polite">
            {justSaved && !dirty && (
              <>
                <Check className="size-3.5" aria-hidden /> Saved
              </>
            )}
          </span>
          <Button disabled={!dirty} onClick={() => saved && setDraft(saved)}>
            Revert
          </Button>
          <Button variant="primary" busy={saving} disabled={!dirty} onClick={() => void save()}>
            Save
          </Button>
        </div>
      </footer>
    </>
  );
}

function Row({ label, hint, children }: { label: string; hint: string; children: ReactNode }) {
  return (
    <div className="py-4 first:pt-0">
      <div className="mb-2 flex items-baseline justify-between gap-3">
        <span className="text-[13.5px] font-medium text-ink">{label}</span>
        <span className="truncate text-xs text-ink-3">{hint}</span>
      </div>
      {children}
    </div>
  );
}

/** A "Don't change" segment followed by `options`. */
function Choice<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T | null;
  options: readonly { value: T; label: string }[];
  onChange: (value: T | null) => void;
}) {
  return (
    <Segmented<T | "unset">
      label={label}
      hideLabel
      value={value ?? "unset"}
      options={[{ value: "unset", label: "Don't change" }, ...options]}
      onChange={(v) => onChange(v === "unset" ? null : v)}
    />
  );
}

type PaintMode = "unset" | "auto" | "color";

const DEFAULT_COLOR = "#808080";
const isHex = (value: string | null): value is string => value !== null && /^#[0-9a-f]{6}$/i.test(value);

/** Don't change, Auto (optional), or a picked color. */
function PaintRow({
  label,
  hint,
  allowAuto = false,
  value,
  onChange,
}: {
  label: string;
  hint: string;
  allowAuto?: boolean;
  value: string | null;
  onChange: (value: string | null) => void;
}) {
  const pickerId = useId();
  const mode: PaintMode = value === null ? "unset" : value === "auto" ? "auto" : "color";
  const options: { value: PaintMode; label: string }[] = [
    { value: "unset", label: "Don't change" },
    ...(allowAuto ? [{ value: "auto" as const, label: "Auto" }] : []),
    { value: "color", label: "Color" },
  ];

  return (
    <Row label={label} hint={hint}>
      <div className="flex items-center gap-3">
        <Segmented<PaintMode>
          className="min-w-0 flex-1"
          label={label}
          hideLabel
          value={mode}
          options={options}
          onChange={(next) => onChange(next === "unset" ? null : next === "auto" ? "auto" : isHex(value) ? value : DEFAULT_COLOR)}
        />
        {mode === "color" && (
          <label htmlFor={pickerId} className="flex shrink-0 cursor-default items-center gap-2 rounded-lg bg-canvas py-1 pr-3 pl-1 ring-1 ring-line-strong hover:ring-line-hi has-focus-visible:outline-2 has-focus-visible:outline-ink">
            <input
              id={pickerId}
              type="color"
              aria-label={`${label} color`}
              value={isHex(value) ? value.toLowerCase() : DEFAULT_COLOR}
              onChange={(e) => onChange(e.target.value)}
              className="size-7 cursor-default rounded-md border-0 bg-transparent p-0"
            />
            <span className="font-mono text-xs text-ink-2 uppercase">{isHex(value) ? value : DEFAULT_COLOR}</span>
          </label>
        )}
      </div>
    </Row>
  );
}

const RADIUS: Record<Corners, string> = { default: "8px", round: "12px", small: "4px", square: "0px" };

/** A tiny mock window that reflects the draft, so choices read at a glance. */
function StylePreview({ style }: { style: WindowsStyle }) {
  const light = style.dark_mode === false;
  const border = isHex(style.border) ? style.border : "var(--line-hi)";
  return (
    <div
      aria-hidden
      className="relative grid h-40 place-items-center overflow-hidden rounded-xl ring-1 ring-line"
      style={{ background: "linear-gradient(135deg, var(--surface-hi), var(--canvas))" }}
    >
      <div
        className="w-56 overflow-hidden shadow-card"
        style={{
          borderRadius: RADIUS[style.corners ?? "default"],
          border: `1.5px ${style.border === "auto" ? "dashed" : "solid"} ${border}`,
          background: light ? "#f3f3f3" : "#202020",
        }}
      >
        <div
          className="flex h-7 items-center justify-between px-3 text-[11px]"
          style={{
            background: isHex(style.caption) ? style.caption : light ? "#e6e6e6" : "#2b2b2b",
            color: isHex(style.caption_text) ? style.caption_text : light ? "#222" : "#ddd",
          }}
        >
          <span>Window</span>
          <span className="tracking-widest">– ▢ ✕</span>
        </div>
        <div className="space-y-2 p-3">
          <div className="h-2 w-3/4 rounded-full" style={{ background: light ? "#cfcfcf" : "#3a3a3a" }} />
          <div className="h-2 w-1/2 rounded-full" style={{ background: light ? "#cfcfcf" : "#3a3a3a" }} />
          <div
            className="flex h-6 w-20 items-center justify-center rounded-md text-[10px] font-medium"
            style={{
              background: isHex(style.accent) ? style.accent : style.accent === "auto" ? "repeating-linear-gradient(135deg,#777 0 4px,#999 4px 8px)" : light ? "#bdbdbd" : "#555",
              color: "#fff",
            }}
          >
            {style.accent === "auto" ? "auto" : "Accent"}
          </div>
        </div>
      </div>
      {style.backdrop && (
        <span className="absolute right-2 bottom-2 rounded-full bg-black/50 px-2 py-0.5 text-[10px] font-medium text-white capitalize">{style.backdrop}</span>
      )}
    </div>
  );
}
