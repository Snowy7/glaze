import type { ReactNode } from "react";

import { getIn, tidy, type Path } from "../../lib/barsDoc";
import { useBars } from "../../lib/barsContext";
import type { Gradient, Paint } from "../../lib/barsSchema";
import { INPUT_CLASS, Row, Segmented, Select, Slider, Switch } from "../ui";
import { ColorValue, PaintValue } from "./PaintEditor";

// Controls bound to a path of the `bars.json` being edited. Each shows the value the host will
// use (the file's, else the preset's, else the default) and writes only the field it is for.
// A field the file itself sets can be reset, which removes it so the preset shows through.

export const numberAt = (root: unknown, path: Path): number | undefined => {
  const value = getIn(root, path);
  return typeof value === "number" ? value : undefined;
};
export const boolAt = (root: unknown, path: Path): boolean | undefined => {
  const value = getIn(root, path);
  return typeof value === "boolean" ? value : undefined;
};
export const stringAt = (root: unknown, path: Path): string | undefined => {
  const value = getIn(root, path);
  return typeof value === "string" ? value : undefined;
};

/** A row whose title can be reset to the preset's value when the file sets `path`. */
export function SettingRow({ title, detail, path, children }: { title: string; detail?: ReactNode; path?: Path; children: ReactNode }) {
  const { has, set } = useBars();
  const own = path !== undefined && has(path);
  return (
    <Row
      title={title}
      detail={
        <>
          {detail}
          {own && (
            <button
              type="button"
              onClick={() => set(path, undefined)}
              className="ml-2 cursor-default rounded px-1 text-[11.5px] font-medium text-ink-2 underline decoration-line-hi underline-offset-2 hover:text-ink"
              title="Remove this from your file so the preset's value applies"
            >
              Reset
            </button>
          )}
        </>
      }
    >
      {children}
    </Row>
  );
}

export function ToggleSetting({ path, title, detail, fallback = false }: { path: Path; title: string; detail?: ReactNode; fallback?: boolean }) {
  const { resolved, set } = useBars();
  return (
    <SettingRow title={title} detail={detail} path={path}>
      <Switch label={title} checked={boolAt(resolved, path) ?? fallback} onChange={(on) => set(path, on)} />
    </SettingRow>
  );
}

type SliderProps = { path: Path; title: string; detail?: ReactNode; min: number; max: number; step?: number; format?: (v: number) => string; fallback: number };

export function SliderSetting({ path, title, detail, min, max, step = 1, format, fallback }: SliderProps) {
  const { resolved, set } = useBars();
  return (
    <SettingRow title={title} detail={detail} path={path}>
      <Slider className="w-64" label={title} value={numberAt(resolved, path) ?? fallback} min={min} max={max} step={step} format={format} onChange={(v) => set(path, tidy(v))} />
    </SettingRow>
  );
}

/** A number that can be "auto": the layout's own default until the file sets it. */
export function GeometrySetting({ path, title, detail, shown, min, max }: { path: Path; title: string; detail?: ReactNode; shown: number; min: number; max: number }) {
  const { set } = useBars();
  return (
    <SettingRow title={title} detail={detail} path={path}>
      <Slider className="w-64" label={title} value={shown} min={min} max={max} format={(v) => `${v}px`} onChange={(v) => set(path, tidy(v))} />
    </SettingRow>
  );
}

type ChoiceProps<T extends string> = { path: Path; title: string; detail?: ReactNode; options: readonly { value: T; label: string; hint?: string }[]; fallback: T; wide?: boolean };

export function ChoiceSetting<T extends string>({ path, title, detail, options, fallback, wide }: ChoiceProps<T>) {
  const { resolved, set } = useBars();
  const current = stringAt(resolved, path);
  const value = options.find((o) => o.value === current)?.value ?? fallback;
  return (
    <SettingRow title={title} detail={detail} path={path}>
      <Segmented<T> className={wide ? "w-96" : "w-72"} label={title} hideLabel value={value} options={options} onChange={(v) => set(path, v)} />
    </SettingRow>
  );
}

export function SelectSetting<T extends string>({ path, title, detail, options, fallback }: Omit<ChoiceProps<T>, "wide">) {
  const { resolved, set } = useBars();
  const current = stringAt(resolved, path);
  const value = options.find((o) => o.value === current)?.value ?? fallback;
  return (
    <SettingRow title={title} detail={detail} path={path}>
      <Select<T> label={title} className="w-56" value={value} options={options} onChange={(v) => set(path, v)} />
    </SettingRow>
  );
}

export function TextSetting({ path, title, detail, placeholder, mono }: { path: Path; title: string; detail?: ReactNode; placeholder?: string; mono?: boolean }) {
  const { resolved, set } = useBars();
  return (
    <SettingRow title={title} detail={detail} path={path}>
      <input
        aria-label={title}
        value={stringAt(resolved, path) ?? ""}
        placeholder={placeholder}
        spellCheck={false}
        // An empty box means "not set": the field leaves the file instead of becoming "".
        onChange={(e) => set(path, e.currentTarget.value === "" ? undefined : e.currentTarget.value)}
        className={`${INPUT_CLASS} w-64 ${mono ? "font-mono text-[13px]" : ""}`}
      />
    </SettingRow>
  );
}

export const px = (value: number) => `${value}px`;
export const percent = (value: number) => `${Math.round(value * 100)}%`;

// --- Compact fields, for the Look and Motion tabs ---

/** A labeled slider for one optional value; it shows the host's value dimmed until the file sets it. */
export function FieldSlider({ path, label, min, max, step = 1, fallback, format }: { path: Path; label: string; min: number; max: number; step?: number; fallback: number; format?: (v: number) => string }) {
  const { resolved, has, set } = useBars();
  const own = has(path);
  return (
    <div className="flex items-center gap-3">
      <span className="w-28 shrink-0 text-[13px] text-ink-2">{label}</span>
      <Slider className={`min-w-0 flex-1 ${own ? "" : "opacity-60"}`} label={label} value={numberAt(resolved, path) ?? fallback} min={min} max={max} step={step} format={format} onChange={(v) => set(path, tidy(v))} />
      <button type="button" disabled={!own} onClick={() => set(path, undefined)} className="w-12 shrink-0 cursor-default text-xs text-ink-3 enabled:hover:text-ink" title="Remove this from your file so the preset's value applies">
        {own ? "Reset" : "Default"}
      </button>
    </div>
  );
}

/** A labeled color for one optional value. */
export function FieldColor({ path, label, placeholder }: { path: Path; label: string; placeholder?: string }) {
  const { resolved, has, set } = useBars();
  return (
    <div className="flex flex-wrap items-center gap-3">
      <span className="w-28 shrink-0 text-[13px] text-ink-2">{label}</span>
      <ColorValue label={label} value={stringAt(resolved, path)} placeholder={placeholder} onChange={(color) => set(path, color)} onClear={has(path) ? () => set(path, undefined) : undefined} />
    </div>
  );
}

const asPaint = (value: unknown): Paint | undefined =>
  typeof value === "string" ? value : typeof value === "object" && value !== null && "stops" in value ? (value as Gradient) : undefined;

/** A labeled paint (color or gradient) for one optional value. */
export function FieldPaint({ path, label, placeholder }: { path: Path; label: string; placeholder?: string }) {
  const { resolved, has, set } = useBars();
  return (
    <div className="flex items-start gap-3">
      <span className="mt-2 w-28 shrink-0 text-[13px] text-ink-2">{label}</span>
      <div className="min-w-0 flex-1">
        <PaintValue label={label} value={asPaint(getIn(resolved, path))} placeholder={placeholder} onChange={(paint) => set(path, paint)} onClear={has(path) ? () => set(path, undefined) : undefined} />
      </div>
    </div>
  );
}
