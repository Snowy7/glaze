import type { ReactNode } from "react";

import { getIn, tidy, type Path } from "../../lib/barsDoc";
import { useBars } from "../../lib/barsContext";
import type { Gradient, Paint } from "../../lib/barsSchema";
import { INPUT_BASE, Prop, Segmented, Select, Slider, Switch } from "../ui";
import { ColorValue, PaintValue } from "./PaintEditor";

// Controls bound to a path of the `bars.json` being edited, as inspector properties. Each shows the
// value the host will use (the file's, else the design's or preset's, else the default), marked
// "Design" until the file sets it; then the reset arrow takes it out of the file again.

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

/** Sliders beside their label share one width, so their tracks line up (as in the look fields). */
const SLIDER = "w-[10.5rem] 2xl:w-[13rem]";

/** The id search reveals: the path, dotted. */
const idOf = (path: Path) => `bars.${path.join(".")}`;

/** One setting at `path`: its label, where its value comes from, a reset while the file sets it. */
export function SettingRow({ title, detail, path, stacked = false, children }: { title: string; detail?: ReactNode; path?: Path; stacked?: boolean; children: ReactNode }) {
  const { has, set } = useBars();
  const own = path !== undefined && has(path);
  return (
    <Prop id={path && idOf(path)} label={title} hint={detail} stacked={stacked} inherited={path && !own ? "Design" : undefined} onReset={own ? () => set(path, undefined) : undefined}>
      {children}
    </Prop>
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
      <Slider className={SLIDER} label={title} value={numberAt(resolved, path) ?? fallback} min={min} max={max} step={step} format={format} onChange={(v) => set(path, tidy(v))} />
    </SettingRow>
  );
}

/** A number the layout or design decides until the file sets it. */
export function GeometrySetting({ path, title, detail, shown, min, max }: { path: Path; title: string; detail?: ReactNode; shown: number; min: number; max: number }) {
  const { set } = useBars();
  return (
    <SettingRow title={title} detail={detail} path={path}>
      <Slider className={SLIDER} label={title} value={shown} min={min} max={max} format={(v) => `${v}px`} onChange={(v) => set(path, tidy(v))} />
    </SettingRow>
  );
}

type ChoiceProps<T extends string> = { path: Path; title: string; detail?: ReactNode; options: readonly { value: T; label: string; hint?: string }[]; fallback: T; wide?: boolean };

/** A few choices as a segmented control; `wide` (or more than three) puts it under the label. */
export function ChoiceSetting<T extends string>({ path, title, detail, options, fallback, wide = false }: ChoiceProps<T>) {
  const { resolved, set } = useBars();
  const current = stringAt(resolved, path);
  const value = options.find((o) => o.value === current)?.value ?? fallback;
  const stacked = wide || options.length > 3;
  return (
    <SettingRow title={title} detail={detail} path={path} stacked={stacked}>
      <Segmented<T> className={stacked ? "w-full" : ""} size="sm" label={title} hideLabel value={value} options={options} onChange={(v) => set(path, v)} />
    </SettingRow>
  );
}

export function SelectSetting<T extends string>({ path, title, detail, options, fallback }: Omit<ChoiceProps<T>, "wide">) {
  const { resolved, set } = useBars();
  const current = stringAt(resolved, path);
  const value = options.find((o) => o.value === current)?.value ?? fallback;
  return (
    <SettingRow title={title} detail={detail} path={path}>
      <Select<T> label={title} size="sm" className="w-44" value={value} options={options} onChange={(v) => set(path, v)} />
    </SettingRow>
  );
}

export function TextSetting({ path, title, detail, placeholder, mono }: { path: Path; title: string; detail?: ReactNode; placeholder?: string; mono?: boolean }) {
  const { resolved, set } = useBars();
  return (
    <SettingRow title={title} detail={detail} path={path} stacked>
      <input
        aria-label={title}
        value={stringAt(resolved, path) ?? ""}
        placeholder={placeholder}
        spellCheck={false}
        // An empty box means "not set": the field leaves the file instead of becoming "".
        onChange={(e) => set(path, e.currentTarget.value === "" ? undefined : e.currentTarget.value)}
        className={`${INPUT_BASE} ui-input-sm w-full ${mono ? "font-mono text-[12px]" : ""}`}
      />
    </SettingRow>
  );
}

export const px = (value: number) => `${value}px`;
export const percent = (value: number) => `${Math.round(value * 100)}%`;

// --- Compact fields, for the Look and Motion panels ---

/** A slider for one optional value; it shows the host's value until the file sets it. */
export function FieldSlider({ path, label, min, max, step = 1, fallback, format }: { path: Path; label: string; min: number; max: number; step?: number; fallback: number; format?: (v: number) => string }) {
  const { resolved, set } = useBars();
  return (
    <SettingRow title={label} path={path}>
      <Slider className={SLIDER} label={label} value={numberAt(resolved, path) ?? fallback} min={min} max={max} step={step} format={format} onChange={(v) => set(path, tidy(v))} />
    </SettingRow>
  );
}

/** A color for one optional value. */
export function FieldColor({ path, label, placeholder }: { path: Path; label: string; placeholder?: string }) {
  const { resolved, set } = useBars();
  return (
    <SettingRow title={label} path={path} stacked>
      <ColorValue label={label} value={stringAt(resolved, path)} placeholder={placeholder} onChange={(color) => set(path, color)} />
    </SettingRow>
  );
}

const asPaint = (value: unknown): Paint | undefined =>
  typeof value === "string" ? value : typeof value === "object" && value !== null && "stops" in value ? (value as Gradient) : undefined;

/** A paint (color or gradient) for one optional value. */
export function FieldPaint({ path, label, placeholder }: { path: Path; label: string; placeholder?: string }) {
  const { resolved, set } = useBars();
  return (
    <SettingRow title={label} path={path} stacked>
      <PaintValue label={label} value={asPaint(getIn(resolved, path))} placeholder={placeholder} onChange={(paint) => set(path, paint)} />
    </SettingRow>
  );
}
