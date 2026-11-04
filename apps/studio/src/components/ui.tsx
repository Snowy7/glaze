import { Check, ChevronDown, ChevronLeft, ChevronRight, GripVertical, LoaderCircle, Minus, Plus, Redo2, RotateCcw, TriangleAlert, Undo2, type LucideIcon } from "lucide-react";
import {
  createContext,
  use,
  useEffect,
  useId,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type ComponentProps,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";

import { useHistory } from "../lib/history";

// The control kit. Colors come only from the tokens in index.css; primary is the inverse fill,
// selection is a raised gray with a bright edge, never a hue. Control bases (`ui-btn`,
// `ui-input`, `ui-range`) live in index.css's components layer, so a caller's `className`
// utilities always win over them.

/** Control sizes: `sm` 30 px, `md` 36 px (the default), `lg` 40 px tall. */
export type Size = "sm" | "md" | "lg";

const SIZE_CLASS: Record<Size, string> = { sm: "ui-btn-sm", md: "", lg: "ui-btn-lg" };

const VARIANTS = {
  primary: "bg-solid text-solid-ink shadow-[0_1px_2px_rgb(0_0_0/0.35)] enabled:hover:bg-solid-hover",
  secondary: "bg-surface-hi text-ink ring-1 ring-line-strong ring-inset enabled:hover:bg-surface-top enabled:hover:ring-line-hi",
  ghost: "text-ink-2 enabled:hover:bg-surface-hi enabled:hover:text-ink",
} as const;

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: keyof typeof VARIANTS;
  size?: Size;
  icon?: LucideIcon;
  /** Shows a spinner in place of the icon and disables the button. */
  busy?: boolean;
};

export function Button({ variant = "secondary", size = "md", icon: Icon, busy = false, disabled, className = "", children, ...rest }: ButtonProps) {
  return (
    <button type="button" disabled={disabled || busy} className={`ui-btn cursor-default ${SIZE_CLASS[size]} ${VARIANTS[variant]} ${className}`} {...rest}>
      {busy ? <LoaderCircle className="size-4 animate-spin" aria-hidden /> : Icon && <Icon className={size === "sm" ? "size-3.5" : "size-4"} aria-hidden />}
      {children}
    </button>
  );
}

const ICON_VARIANTS = {
  outline: "text-ink-2 ring-1 ring-line-strong ring-inset enabled:hover:bg-surface-hi enabled:hover:text-ink enabled:hover:ring-line-hi",
  ghost: "text-ink-3 enabled:hover:bg-surface-hi enabled:hover:text-ink",
} as const;

/** A square icon-only button; `label` is its accessible name and tooltip. */
export function IconButton({
  icon: Icon,
  label,
  size = "md",
  variant = "outline",
  className = "",
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { icon: LucideIcon; label: string; size?: Exclude<Size, "lg">; variant?: keyof typeof ICON_VARIANTS }) {
  return (
    <button type="button" aria-label={label} title={label} className={`ui-btn ui-icon-btn cursor-default ${SIZE_CLASS[size]} ${ICON_VARIANTS[variant]} ${className}`} {...rest}>
      <Icon className={size === "sm" ? "size-3.5" : "size-4"} aria-hidden />
    </button>
  );
}

type SwitchProps = {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
  /** For a `<label htmlFor>` elsewhere (see `ToggleRow`). */
  id?: string;
  /** The id of the text that explains it. */
  describedBy?: string;
  size?: "sm" | "md";
};

/** An accessible on/off switch; the visible label lives in the row around it. Space and Enter toggle it. */
export function Switch({ checked, onChange, label, disabled, id, describedBy, size = "md" }: SwitchProps) {
  const small = size === "sm";
  return (
    <button
      type="button"
      role="switch"
      id={id}
      aria-checked={checked}
      aria-label={label}
      aria-describedby={describedBy}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex shrink-0 cursor-default items-center rounded-full p-0.5 transition-colors duration-200 disabled:opacity-40 ${small ? "h-4 w-7" : "h-5 w-9"} ${
        checked ? "bg-solid enabled:hover:bg-solid-hover" : "bg-surface-top ring-1 ring-line-hi/50 ring-inset enabled:hover:ring-line-hi"
      }`}
    >
      <span
        aria-hidden
        className={`rounded-full shadow-[0_1px_2px_rgb(0_0_0/0.4)] transition-transform duration-250 ease-[cubic-bezier(0.3,1.35,0.5,1)] ${small ? "size-3" : "size-4"} ${
          checked ? `bg-solid-ink ${small ? "translate-x-3" : "translate-x-4"}` : "bg-ink-2"
        }`}
      />
    </button>
  );
}

type SegmentedOption<T extends string> = { value: T; label: string; hint?: string; disabled?: boolean; icon?: LucideIcon };

type SegmentedProps<T extends string> = {
  label: string;
  value: T;
  options: readonly SegmentedOption<T>[];
  onChange: (value: T) => void;
  /** Hide the legend visually (it stays for screen readers). */
  hideLabel?: boolean;
  /** As wide as its content by default; give it a width (`w-72`, `w-full`) and the segments share it. */
  className?: string;
  size?: "sm" | "md";
};

/**
 * A radio group drawn as a segmented control: arrow keys move the choice. The selected
 * segment is raised with a bright edge.
 */
export function Segmented<T extends string>({ label, value, options, onChange, hideLabel, className = "", size = "md" }: SegmentedProps<T>) {
  const name = useId();
  return (
    <fieldset className={`ui-segmented ${className}`}>
      <legend className={hideLabel ? "sr-only" : "mb-1.5 text-[13px] font-medium text-ink-2"}>{label}</legend>
      <div className="flex flex-wrap gap-0.5 rounded-lg bg-canvas p-0.5 ring-1 ring-line-strong ring-inset">
        {options.map((option) => (
          <label
            key={option.value}
            title={option.hint}
            className={`relative flex flex-1 cursor-default items-center justify-center gap-1.5 rounded-md text-center font-medium whitespace-nowrap text-ink-3 transition-[color,background-color,box-shadow] duration-150 hover:bg-surface-hi hover:text-ink has-checked:bg-surface-top has-checked:text-ink has-checked:shadow-[inset_0_0_0_1px_var(--line-hi),0_1px_2px_rgb(0_0_0/0.4)] has-focus-visible:outline-2 has-focus-visible:outline-offset-1 has-focus-visible:outline-ink ${
              size === "sm" ? "h-[26px] px-2.5 text-[12px]" : "h-[calc(var(--control-h,2.25rem)-4px)] px-3 text-[13px]"
            } ${option.disabled ? "pointer-events-none opacity-35" : ""}`}
          >
            <input
              type="radio"
              name={name}
              value={option.value}
              disabled={option.disabled}
              checked={value === option.value}
              onChange={() => onChange(option.value)}
              className="sr-only"
            />
            {option.icon && <option.icon className="size-3.5 shrink-0" aria-hidden />}
            {option.label}
          </label>
        ))}
      </div>
    </fieldset>
  );
}

/** A native dropdown (the platform's keyboard and list), drawn like a button with our chevron. Widths go in `className`. */
export function Select<T extends string>({
  label,
  value,
  options,
  onChange,
  className = "",
  size = "md",
  disabled,
}: {
  label: string;
  value: T;
  options: readonly { value: T; label: string; disabled?: boolean }[];
  onChange: (value: T) => void;
  className?: string;
  size?: "sm" | "md";
  disabled?: boolean;
}) {
  return (
    <span className={`relative inline-flex min-w-0 ${className}`}>
      <select
        aria-label={label}
        value={value}
        disabled={disabled}
        onChange={(e) => {
          const picked = options.find((o) => o.value === e.currentTarget.value);
          if (picked) onChange(picked.value);
        }}
        className={`ui-input w-full cursor-default truncate bg-surface-hi pl-3 hover:bg-surface-top ${size === "sm" ? "ui-input-sm text-[12px]" : ""}`}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value} disabled={o.disabled}>
            {o.label}
          </option>
        ))}
      </select>
      <ChevronDown className="pointer-events-none absolute top-1/2 right-2.5 size-4 -translate-y-1/2 text-ink-3" aria-hidden />
    </span>
  );
}

/** Text input styled for the monochrome surfaces, without a width: add `w-…` or `flex-1` yourself. */
export const INPUT_BASE = "selectable ui-input";

/** The same, as wide as its container. */
export const INPUT_CLASS = `w-full ${INPUT_BASE}`;

type TextInputProps = Omit<ComponentProps<"input">, "size"> & {
  /** Paths, commands, ids: monospace, no spell check. */
  mono?: boolean;
  /** The small control size (30 px), as in the inspector. */
  compact?: boolean;
};

/** A styled `<input>`; widths go in `className` (it has none of its own). */
export function TextInput({ mono = false, compact = false, className = "", ...rest }: TextInputProps) {
  return (
    <input
      spellCheck={mono ? false : undefined}
      className={`${INPUT_BASE} ${compact ? "ui-input-sm" : ""} ${mono ? "font-mono text-[12px]" : ""} ${className}`}
      {...rest}
    />
  );
}

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

/** `value` on the `step` grid from `min`, without float noise (0.1 + 0.2 stays 0.3). */
function snap(value: number, min: number, step: number): number {
  const decimals = (String(step).split(".")[1] ?? "").length;
  return Number((Math.round((value - min) / step) * step + min).toFixed(decimals));
}

/**
 * Typed text in a number box: shown formatted, edited as text, committed on Enter or blur.
 * Escape gives up. Arrow keys step by `step` (Shift: ten steps).
 */
function useNumberDraft(value: number, commit: (next: number) => void, format: (v: number) => string, parse: (text: string) => number, step: number) {
  const [draft, setDraft] = useState<string | null>(null);
  const finish = () => {
    // Only a changed text commits: focusing and leaving must not "set" an inherited value.
    if (draft !== null && draft !== format(value)) {
      const parsed = parse(draft);
      if (Number.isFinite(parsed)) commit(parsed);
    }
    setDraft(null);
  };
  return {
    value: draft ?? format(value),
    onFocus: (e: { currentTarget: HTMLInputElement }) => {
      setDraft(format(value));
      e.currentTarget.select();
    },
    onChange: (e: { currentTarget: HTMLInputElement }) => setDraft(e.currentTarget.value),
    onBlur: finish,
    onKeyDown: (e: KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Enter") e.currentTarget.blur();
      else if (e.key === "Escape") {
        setDraft(null);
        e.currentTarget.blur();
      } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
        e.preventDefault();
        const next = value + (e.key === "ArrowUp" ? 1 : -1) * step * (e.shiftKey ? 10 : 1);
        commit(next);
        setDraft(null);
      }
    },
  };
}

/** How a number reads in a value box, and how typed text turns back into one. Spread into `Slider`. */
export const units = {
  px: { format: (v: number) => `${v}px`, parse: Number.parseFloat },
  ms: { format: (v: number) => `${v}ms`, parse: Number.parseFloat },
  percent: { format: (v: number) => `${Math.round(v * 100)}%`, parse: (text: string) => Number.parseFloat(text) / 100 },
  times: { format: (v: number) => `${v.toFixed(2)}x`, parse: Number.parseFloat },
} satisfies Record<string, { format: (v: number) => string; parse: (text: string) => number }>;

type SliderProps = {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  /** How the value reads next to the track, e.g. `(v) => `${v}px``. */
  format?: (value: number) => string;
  /**
   * Turns typed text back into a value, which makes the value box editable. Without `format`
   * the box is always editable; with `format` alone it only shows the value. See `units`.
   */
  parse?: (text: string) => number;
  onChange: (value: number) => void;
  className?: string;
  disabled?: boolean;
};

/** A range input with its value in a box beside it. Arrow keys work on both. */
export function Slider({ label, value, min, max, step = 1, format, parse, onChange, className = "", disabled }: SliderProps) {
  const show = format ?? String;
  const read = parse ?? (format ? undefined : Number.parseFloat);
  const commit = (next: number) => onChange(clamp(snap(next, min, step), min, max));
  const draft = useNumberDraft(value, commit, show, read ?? Number.parseFloat, step);
  const pct = max > min ? ((clamp(value, min, max) - min) / (max - min)) * 100 : 0;
  const fill: CSSProperties & Record<"--pct", string> = { "--pct": `${pct}%` };
  return (
    <div className={`flex min-w-0 items-center gap-3 ${className}`}>
      <input
        type="range"
        aria-label={label}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        style={fill}
        onChange={(e) => onChange(e.currentTarget.valueAsNumber)}
        className="ui-range min-w-0 flex-1 cursor-default"
      />
      {read ? (
        <input
          aria-label={`${label} value`}
          inputMode="decimal"
          disabled={disabled}
          {...draft}
          className="ui-input h-[26px] w-16 shrink-0 rounded-md px-2 text-right font-mono text-[12px] tabular-nums"
        />
      ) : (
        <output className="w-16 shrink-0 pr-2 text-right font-mono text-[12px] text-ink-2 tabular-nums">{show(value)}</output>
      )}
    </div>
  );
}

/** A number box with − and + beside it. Typing, arrow keys and the buttons all clamp to `min`…`max`. */
export function NumberField({
  label,
  value,
  onChange,
  min = -Infinity,
  max = Infinity,
  step = 1,
  unit,
  className = "",
  disabled,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  /** Shown after the number, e.g. `px`. */
  unit?: string;
  className?: string;
  disabled?: boolean;
}) {
  const commit = (next: number) => onChange(clamp(snap(next, Number.isFinite(min) ? min : 0, step), min, max));
  const draft = useNumberDraft(value, commit, String, Number.parseFloat, step);
  const stepButton = "grid h-full w-7 shrink-0 cursor-default place-items-center text-ink-3 transition-colors enabled:hover:bg-surface-hi enabled:hover:text-ink disabled:opacity-30";
  return (
    <div className={`ui-input ui-number inline-flex items-center overflow-hidden px-0 ${className}`}>
      <button type="button" tabIndex={-1} aria-label={`Decrease ${label}`} disabled={disabled || value <= min} onClick={() => commit(value - step)} className={stepButton}>
        <Minus className="size-3.5" aria-hidden />
      </button>
      <input aria-label={label} inputMode="decimal" disabled={disabled} {...draft} className="h-full w-full flex-1 text-center font-mono text-[12px] tabular-nums" />
      {unit && <span className="pr-1 text-[12px] text-ink-3">{unit}</span>}
      <button type="button" tabIndex={-1} aria-label={`Increase ${label}`} disabled={disabled || value >= max} onClick={() => commit(value + step)} className={stepButton}>
        <Plus className="size-3.5" aria-hidden />
      </button>
    </div>
  );
}

const HEX = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i;

/** `"abc"`, `"#ABC"`, `"#aabbcc"` -> `"#aabbcc"`; anything else -> null. */
export function normalizeHex(text: string): string | null {
  const digits = HEX.exec(text.trim())?.[1];
  if (!digits) return null;
  return `#${digits.length === 3 ? [...digits].map((c) => c + c).join("") : digits}`.toLowerCase();
}

/** A color swatch (opens the native picker) and its hex value, which can be typed. */
export function ColorField({ label, value, onChange, className = "" }: { label: string; value: string; onChange: (value: string) => void; className?: string }) {
  const [draft, setDraft] = useState<string | null>(null);
  const hex = normalizeHex(value);
  return (
    <div className={`ui-input ui-color inline-flex shrink-0 items-center gap-2 pr-2 pl-1 ${className}`}>
      <label className="relative size-[calc(var(--control-h,2.25rem)-10px)] shrink-0 cursor-default overflow-hidden rounded-[5px] shadow-[inset_0_0_0_1px_rgb(255_255_255/0.12),0_0_0_1px_rgb(0_0_0/0.3)]" style={{ background: hex ?? "repeating-conic-gradient(#777 0 25%, #444 0 50%) 0 0 / 8px 8px" }}>
        <input type="color" aria-label={`${label}: pick a color`} value={hex ?? "#000000"} onChange={(e) => onChange(e.currentTarget.value)} className="absolute inset-0 size-full cursor-default opacity-0" />
      </label>
      <input
        aria-label={label}
        value={draft ?? value}
        spellCheck={false}
        maxLength={7}
        onFocus={(e) => {
          setDraft(value);
          e.currentTarget.select();
        }}
        onChange={(e) => {
          const text = e.currentTarget.value;
          setDraft(text);
          const next = normalizeHex(text);
          if (next && text.replace("#", "").length === 6) onChange(next);
        }}
        onBlur={() => {
          const next = draft === null ? null : normalizeHex(draft);
          if (next && next !== hex) onChange(next);
          setDraft(null);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape") {
            setDraft(null);
            e.currentTarget.blur();
          }
        }}
        className="selectable w-full font-mono text-[12px] text-ink-2 uppercase"
      />
    </div>
  );
}

type ComboOption = { value: string; label?: string; hint?: string };

/**
 * Free text with suggestions (ARIA combobox): type anything, or pick from the list with the
 * arrow keys and Enter, or the mouse. The list is a popover, so no card ever clips it.
 */
export function Combobox({
  label,
  value,
  onChange,
  options,
  placeholder,
  mono = false,
  className = "",
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  options: readonly ComboOption[];
  placeholder?: string;
  mono?: boolean;
  className?: string;
}) {
  const id = useId();
  const listId = `${id}-list`;
  const anchor: CSSProperties & Record<"--anchor", string> = { "--anchor": `--combo-${id.replace(/[^a-z0-9]/gi, "")}` };
  const list = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  // What the user typed since opening: filters the list. Null shows everything.
  const [query, setQuery] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  const needle = query?.trim().toLowerCase() ?? "";
  const shown = needle === "" ? options : options.filter((o) => `${o.label ?? ""} ${o.value}`.toLowerCase().includes(needle));
  const visible = open && shown.length > 0;

  useEffect(() => {
    const popover = list.current;
    if (!popover) return;
    if (visible && !popover.matches(":popover-open")) popover.showPopover();
    if (!visible && popover.matches(":popover-open")) popover.hidePopover();
  }, [visible]);

  function close() {
    setOpen(false);
    setQuery(null);
  }

  function pick(option: ComboOption) {
    onChange(option.value);
    close();
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) {
        setOpen(true);
        setActive(Math.max(0, options.findIndex((o) => o.value === value)));
        return;
      }
      setActive((i) => (i + (e.key === "ArrowDown" ? 1 : -1) + shown.length) % Math.max(1, shown.length));
    } else if (e.key === "Enter" && visible) {
      e.preventDefault();
      const option = shown[active];
      if (option) pick(option);
    } else if (e.key === "Escape" && open) {
      e.preventDefault();
      close();
    }
  }

  return (
    <div className={`ui-input ui-anchor relative inline-flex min-w-0 items-center pr-1 ${className}`} style={anchor}>
      <input
        ref={input}
        role="combobox"
        aria-label={label}
        aria-expanded={visible}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={visible ? `${listId}-${active}` : undefined}
        value={value}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(e) => {
          onChange(e.currentTarget.value);
          setQuery(e.currentTarget.value);
          setActive(0);
          setOpen(true);
        }}
        onKeyDown={onKeyDown}
        onBlur={close}
        className={`selectable h-full flex-1 ${mono ? "font-mono text-[12px]" : "text-[13px]"}`}
      />
      <button
        type="button"
        tabIndex={-1}
        aria-label={`Show ${label} suggestions`}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => {
          input.current?.focus();
          setQuery(null);
          setActive(Math.max(0, options.findIndex((o) => o.value === value)));
          setOpen((was) => !was);
        }}
        className="grid size-6 shrink-0 cursor-default place-items-center rounded-md text-ink-3 hover:bg-surface-hi hover:text-ink"
      >
        <ChevronDown className={`size-4 transition-transform ${visible ? "rotate-180" : ""}`} aria-hidden />
      </button>
      <div
        ref={list}
        id={listId}
        role="listbox"
        aria-label={label}
        popover="manual"
        style={anchor}
        className="ui-popover max-h-64 overflow-y-auto rounded-lg bg-surface-hi p-1 text-ink shadow-pop ring-1 ring-line-strong"
      >
        {shown.map((option, index) => (
          <div
            key={option.value}
            id={`${listId}-${index}`}
            role="option"
            aria-selected={index === active}
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => setActive(index)}
            onClick={() => pick(option)}
            className="flex cursor-default items-center gap-2 rounded-md px-2.5 py-1.5 text-[13px] aria-selected:bg-surface-top"
          >
            <Check className={`size-3.5 shrink-0 ${option.value === value ? "" : "invisible"}`} aria-hidden />
            <span className="min-w-0 flex-1 truncate">{option.label ?? option.value}</span>
            {option.hint && <span className="shrink-0 truncate text-[12px] text-ink-3">{option.hint}</span>}
          </div>
        ))}
      </div>
    </div>
  );
}

/** A short explanation under a control, in the muted tone. */
export function Note({ children }: { children: ReactNode }) {
  return <p className="mt-2 text-[12px] leading-snug text-ink-3">{children}</p>;
}

/** A callout for things that replace native behavior or need a restart: bordered, not colored. */
export function Notice({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="rounded-xl bg-surface px-4 py-3 text-[13px] text-ink-2 shadow-[inset_3px_0_0_var(--ink-2)] ring-1 ring-line-strong">
      <div className="mb-0.5 text-[13px] font-semibold text-ink">{title}</div>
      {children}
    </div>
  );
}

const BADGE_TONES = {
  neutral: "bg-surface-hi text-ink-2 ring-1 ring-line-strong ring-inset",
  outline: "text-ink-2 ring-1 ring-line-hi ring-inset",
  solid: "bg-solid text-solid-ink",
  /** On images: dark glass, readable on any picture. */
  overlay: "bg-black/60 text-white ring-1 ring-white/10 ring-inset backdrop-blur-md",
} as const;

/** A small label: a kind, a state, a count. */
export function Badge({ children, icon: Icon, tone = "neutral", title, className = "" }: { children: ReactNode; icon?: LucideIcon; tone?: keyof typeof BADGE_TONES; title?: string; className?: string }) {
  return (
    <span title={title} className={`inline-flex h-5 shrink-0 items-center gap-1 rounded-md px-1.5 text-[11px] leading-none font-medium whitespace-nowrap ${BADGE_TONES[tone]} ${className}`}>
      {Icon && <Icon className="size-3" aria-hidden />}
      {children}
    </span>
  );
}

export function Keycap({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex h-5 min-w-5 items-center justify-center rounded-[5px] bg-surface-hi px-1.5 font-sans text-[11px] font-medium text-ink-2 shadow-[inset_0_-1px_0_var(--line-hi),inset_0_0_0_1px_var(--line-strong)]">
      {children}
    </kbd>
  );
}

/** An error or warning strip. Meaning comes from the icon and wording, not from color. */
export function Banner({ children }: { children: ReactNode }) {
  return (
    <div role="alert" className="selectable mb-6 flex items-center gap-3 rounded-xl bg-surface px-4 py-3 text-[13px] text-ink ring-1 ring-line-hi">
      <TriangleAlert className="size-4 shrink-0" aria-hidden />
      <div className="flex min-w-0 flex-1 flex-wrap items-center justify-between gap-x-4 gap-y-2">{children}</div>
    </div>
  );
}

/** Centered placeholder for lists with nothing in them (or nothing that matches). */
export function EmptyState({
  icon: Icon,
  title,
  children,
  action,
  compact = false,
}: {
  icon: LucideIcon;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
  /** Less padding, for a panel rather than a page. */
  compact?: boolean;
}) {
  return (
    <div className={`grid place-items-center rounded-xl border border-dashed border-line-strong bg-surface/40 px-8 text-center ${compact ? "py-10" : "py-20"}`}>
      <span className="mb-4 grid size-11 place-items-center rounded-xl bg-surface-hi text-ink-2 ring-1 ring-line-strong">
        <Icon className="size-5" strokeWidth={1.6} aria-hidden />
      </span>
      <p className="font-display text-[15px] font-semibold text-ink">{title}</p>
      {children && <div className="selectable mt-1 max-w-md text-[13px] leading-relaxed text-ink-3">{children}</div>}
      {action && <div className="mt-5">{action}</div>}
    </div>
  );
}

/** A shimmering placeholder block while something loads. */
export function Skeleton({ className = "" }: { className?: string }) {
  return (
    <div
      aria-hidden
      className={`animate-[shimmer_1.6s_linear_infinite] bg-[linear-gradient(110deg,var(--surface-hi)_30%,var(--surface-top)_50%,var(--surface-hi)_70%)] bg-[length:200%_100%] ${className}`}
    />
  );
}

/** Engine / activity indicator that does not rely on color: filled = on, ring = off. */
export function StatusDot({ state }: { state: "on" | "off" | "unknown" }) {
  return (
    <span className="relative flex size-2 shrink-0" aria-hidden>
      {state === "on" && <span className="absolute inset-0 animate-pulse-ring rounded-full bg-ink" />}
      <span className={`relative size-2 rounded-full ${state === "on" ? "bg-ink" : state === "off" ? "ring-[1.5px] ring-ink-3 ring-inset" : "bg-line-hi"}`} />
    </span>
  );
}

/**
 * Whether the kit is inside an inspector: there, `Group`, `Row` and `Block` drop their cards and
 * lay out like inspector properties, so settings written for a page read right in a panel too.
 */
const InspectorScope = createContext(false);

/** A section card of rows (settings-style): a title and description over it, an action beside them. */
export function Group({
  title,
  description,
  children,
  action,
  className = "",
}: {
  title?: string;
  description?: ReactNode;
  children: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  if (use(InspectorScope))
    return (
      <section aria-label={title} className={`flex flex-col gap-3.5 not-first:pt-3 ${className}`}>
        {(title || action || description) && (
          <div className="flex items-center justify-between gap-2">
            <div className="min-w-0">
              {title && <h3 className="ui-eyebrow">{title}</h3>}
              {description && <p className="mt-1 text-[12px] leading-snug text-ink-3">{description}</p>}
            </div>
            {action}
          </div>
        )}
        {children}
      </section>
    );
  return (
    <section aria-label={title} className={className}>
      {(title || action) && (
        <div className="mb-2.5 flex items-end justify-between gap-3 px-0.5">
          <div className="min-w-0">
            {title && <h2 className="ui-eyebrow">{title}</h2>}
            {description && <p className="mt-1 text-[12px] text-ink-3">{description}</p>}
          </div>
          {action}
        </div>
      )}
      <div className="divide-y divide-line overflow-hidden rounded-xl bg-surface shadow-card ring-1 ring-line-strong">{children}</div>
    </section>
  );
}

type RowProps = {
  title: string;
  detail?: ReactNode;
  mono?: boolean;
  children?: ReactNode;
  /** The control's id: the title becomes its label (a click on it toggles a switch). */
  htmlFor?: string;
  /** The id to give the detail, for the control's `aria-describedby`. */
  detailId?: string;
};

/** A setting row: title and detail on the left, the control on the right (under them when the row is narrow). */
export function Row({ title, detail, mono, children, htmlFor, detailId }: RowProps) {
  const inInspector = use(InspectorScope);
  const titleClass = inInspector ? "block text-[13px] text-ink-2" : "block text-[13px] font-medium text-ink";
  if (inInspector)
    return (
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
        <div className="min-w-[9rem] flex-1">
          {htmlFor ? (
            <label htmlFor={htmlFor} className={titleClass}>
              {title}
            </label>
          ) : (
            <div className={titleClass}>{title}</div>
          )}
          {detail && (
            <div id={detailId} className={`mt-0.5 text-[12px] leading-snug text-ink-3 ${mono ? "selectable truncate font-mono" : ""}`}>
              {detail}
            </div>
          )}
        </div>
        {children && <div className="inspector-control flex max-w-full min-w-0 items-center gap-2">{children}</div>}
      </div>
    );
  return (
    <div className="flex flex-wrap items-center gap-x-6 gap-y-3 px-4 py-3.5">
      <div className="min-w-0 flex-[1_1_15rem]">
        {htmlFor ? (
          <label htmlFor={htmlFor} className={titleClass}>
            {title}
          </label>
        ) : (
          <div className={titleClass}>{title}</div>
        )}
        {detail && (
          <div
            id={detailId}
            className={`mt-0.5 max-w-[72ch] text-[12px] leading-relaxed text-ink-3 ${mono ? "selectable truncate font-mono" : ""}`}
            title={mono && typeof detail === "string" ? detail : undefined}
          >
            {detail}
          </div>
        )}
      </div>
      {children && <div className="ml-auto flex max-w-full min-w-0 shrink-0 items-center gap-2">{children}</div>}
    </div>
  );
}

/** Like `Row`, but the control gets the full width under the title. */
export function Block({ title, detail, children }: { title: string; detail?: ReactNode; children: ReactNode }) {
  if (use(InspectorScope))
    return (
      <div>
        <div className="text-[13px] text-ink-2">{title}</div>
        {detail && <div className="mt-0.5 text-[12px] leading-snug text-ink-3">{detail}</div>}
        <div className="inspector-control mt-2">{children}</div>
      </div>
    );
  return (
    <div className="px-4 py-3.5">
      <div className="text-[13px] font-medium text-ink">{title}</div>
      {detail && <div className="mt-0.5 max-w-[72ch] text-[12px] leading-relaxed text-ink-3">{detail}</div>}
      <div className="mt-3">{children}</div>
    </div>
  );
}

/** A `Row` with a switch; clicking the title toggles it too. */
export function ToggleRow({
  title,
  detail,
  mono,
  checked,
  disabled = false,
  onChange,
}: {
  title: string;
  detail?: ReactNode;
  mono?: boolean;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  const id = useId();
  return (
    <Row title={title} detail={detail} mono={mono} htmlFor={id} detailId={detail ? `${id}-detail` : undefined}>
      <Switch id={id} label={title} describedBy={detail ? `${id}-detail` : undefined} checked={checked} disabled={disabled} onChange={onChange} />
    </Row>
  );
}

/** A destructive action that asks once: the first click arms it, the second does it. */
export function ConfirmButton({
  children,
  onConfirm,
  label,
  icon,
  disabled,
  size,
}: {
  children: ReactNode;
  onConfirm: () => void;
  label: string;
  icon?: LucideIcon;
  disabled?: boolean;
  size?: Size;
}) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const timer = window.setTimeout(() => setArmed(false), 3000);
    return () => window.clearTimeout(timer);
  }, [armed]);
  return (
    <Button
      variant={armed ? "primary" : "secondary"}
      size={size}
      icon={icon}
      disabled={disabled}
      aria-label={armed ? `Confirm: ${label}` : label}
      onClick={() => {
        if (armed) {
          setArmed(false);
          onConfirm();
        } else setArmed(true);
      }}
    >
      {armed ? "Sure?" : children}
    </Button>
  );
}

// --- Page and inspector patterns (docs/studio-ux.md, "Few patterns") ---

/**
 * The bar on top of every page: the title (with a way back, for sub-pages), what's selected, and the
 * page's actions on the right.
 */
export function PageBar({ title, back, detail, children }: { title: string; back?: { label: string; onClick: () => void }; detail?: ReactNode; children?: ReactNode }) {
  return (
    <header className="flex h-[3.25rem] shrink-0 items-center gap-3 border-b border-line px-(--gutter)">
      <div className="flex min-w-0 flex-1 items-center gap-1.5">
        {back && (
          <>
            <button
              type="button"
              onClick={back.onClick}
              className="-ml-1.5 flex h-8 cursor-default items-center gap-1 rounded-md pr-2 pl-1 text-[13px] font-medium text-ink-3 transition-colors hover:bg-surface-hi hover:text-ink"
            >
              <ChevronLeft className="size-4" aria-hidden />
              {back.label}
            </button>
            <span className="text-ink-4" aria-hidden>
              /
            </span>
          </>
        )}
        <h1 className={`truncate font-display text-[15px] font-semibold tracking-tight text-ink ${back ? "pl-1" : ""}`}>{title}</h1>
        {detail && <span className="ml-1.5 min-w-0 truncate text-[13px] text-ink-3">{detail}</span>}
      </div>
      {children && <div className="flex shrink-0 items-center gap-2">{children}</div>}
    </header>
  );
}

/** Saving / Saved, with Undo and Redo (history.tsx). Goes in the page bar of every editing page. */
export function SaveState({ saving = false }: { saving?: boolean }) {
  const { undo, redo, undoLabel, redoLabel } = useHistory();
  return (
    <div className="flex items-center gap-0.5">
      <span className="mr-2 flex items-center gap-1.5 text-[12px] text-ink-3 max-md:hidden" aria-live="polite">
        {saving ? <LoaderCircle className="size-3.5 animate-spin" aria-hidden /> : <Check className="size-3.5" aria-hidden />}
        {saving ? "Saving" : "Saved"}
      </span>
      <IconButton icon={Undo2} variant="ghost" size="sm" label={undoLabel ? `Undo: ${lower(undoLabel)}` : "Nothing to undo"} disabled={!undoLabel} onClick={undo} />
      <IconButton icon={Redo2} variant="ghost" size="sm" label={redoLabel ? `Redo: ${lower(redoLabel)}` : "Nothing to redo"} disabled={!redoLabel} onClick={redo} />
    </div>
  );
}

const lower = (text: string) => text.charAt(0).toLowerCase() + text.slice(1);

/** An editing page: the stage (left, live) and the inspector (right). Under 1100 px they stack. */
export function EditorLayout({ stage, inspector }: { stage: ReactNode; inspector: ReactNode }) {
  return (
    <div className="flex min-h-0 flex-1 max-[1100px]:flex-col max-[1100px]:overflow-y-auto">
      <div className="relative flex min-h-0 min-w-0 flex-1 flex-col max-[1100px]:min-h-[24rem] max-[1100px]:flex-none">{stage}</div>
      {inspector}
    </div>
  );
}

/** The right-hand panel of an editing page: a header, presets, the common settings, More options. */
export function Inspector({ label, children }: { label: string; children: ReactNode }) {
  return (
    <InspectorScope value>
    <aside
      aria-label={label}
      className="flex w-[21rem] shrink-0 flex-col overflow-y-auto border-l border-line bg-sidebar [--control-h:1.875rem] [scrollbar-gutter:stable] max-[1100px]:w-full max-[1100px]:overflow-visible max-[1100px]:border-t max-[1100px]:border-l-0 2xl:w-[25rem]"
    >
      {children}
    </aside>
    </InspectorScope>
  );
}

/** What the inspector shows: an icon, a name, one line of context, a way back, and actions (Reset). */
export function InspectorHeader({
  icon: Icon,
  title,
  detail,
  back,
  actions,
}: {
  icon: LucideIcon;
  title: string;
  detail?: ReactNode;
  back?: { label: string; onClick: () => void };
  actions?: ReactNode;
}) {
  return (
    <div className="border-b border-line px-4 py-3.5">
      {back && (
        <button
          type="button"
          onClick={back.onClick}
          className="-ml-1.5 mb-2 flex h-6 cursor-default items-center gap-0.5 rounded-md pr-1.5 pl-1 text-[12px] font-medium text-ink-3 hover:bg-surface-hi hover:text-ink"
        >
          <ChevronLeft className="size-3.5" aria-hidden />
          {back.label}
        </button>
      )}
      <div className="flex items-center gap-3">
        <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-surface-hi text-ink ring-1 ring-line-strong">
          <Icon className="size-[18px]" strokeWidth={1.75} aria-hidden />
        </span>
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-[14px] font-semibold tracking-tight text-ink">{title}</h2>
          {detail && <div className="truncate text-[12px] text-ink-3">{detail}</div>}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-1">{actions}</div>}
      </div>
    </div>
  );
}

/** A run of settings in the inspector, with an optional small heading and an action beside it. */
export function InspectorSection({ title, action, children }: { title?: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section aria-label={title} className="border-b border-line px-4 py-4 last:border-b-0">
      {(title || action) && (
        <div className="mb-3 flex min-h-6 items-center justify-between gap-2">
          {title && <h3 className="ui-eyebrow">{title}</h3>}
          {action}
        </div>
      )}
      <div className="flex flex-col gap-3.5">{children}</div>
    </section>
  );
}

type PropProps = {
  /** The search anchor (`data-setting`, see settingsIndex.ts). */
  id?: string;
  label: string;
  /** One short line under the control. */
  hint?: ReactNode;
  /** Where an unset value comes from ("Theme", "Default"). Absent with `onReset`: the user's own. */
  inherited?: string;
  /** Hands the value back to the layer below; offered while the value is the user's own. */
  onReset?: () => void;
  /** The control under the label (full width) instead of beside it. */
  stacked?: boolean;
  children: ReactNode;
};

/**
 * One setting in the inspector: its label, the control, and where the value comes from. A value
 * from below is marked with its source; touching the control makes it the user's own (marked with a
 * dot), and the reset arrow hands it back.
 */
export function Prop({ id, label, hint, inherited, onReset, stacked = false, children }: PropProps) {
  const own = inherited === undefined && onReset !== undefined;
  const labelRow = (
    <div className="group/prop flex min-h-7 min-w-0 items-center gap-1.5">
      <span className={`truncate text-[13px] ${own ? "font-medium text-ink" : "text-ink-2"}`}>{label}</span>
      {own && <span title="Changed" className="size-1.5 shrink-0 rounded-full bg-ink" />}
      {inherited && <span className="shrink-0 text-[11px] text-ink-3">{inherited}</span>}
      {own && (
        <button
          type="button"
          aria-label={`Reset ${lower(label)}`}
          title="Reset"
          onClick={onReset}
          className="-ml-0.5 grid size-6 shrink-0 cursor-default place-items-center rounded-md text-ink-3 transition-colors hover:bg-surface-hi hover:text-ink"
        >
          <RotateCcw className="size-3.5" aria-hidden />
        </button>
      )}
    </div>
  );
  return (
    <div data-setting={id} className="setting-anchor">
      {stacked ? (
        <>
          {labelRow}
          <div className="mt-1.5 min-w-0">{children}</div>
        </>
      ) : (
        <div className="flex min-h-8 items-center gap-3">
          <div className="min-w-0 flex-1">{labelRow}</div>
          <div className="flex shrink-0 items-center justify-end">{children}</div>
        </div>
      )}
      {hint && <p className="mt-1.5 text-[12px] leading-snug text-ink-3">{hint}</p>}
    </div>
  );
}

/** "More options": the rest of a section, one step away. A native disclosure, so search can open it. */
export function More({ label = "More options", children }: { label?: string; children: ReactNode }) {
  return (
    <details className="group/more border-b border-line last:border-b-0">
      <summary className="flex h-11 cursor-default list-none items-center gap-2 px-4 text-[13px] font-medium text-ink-2 transition-colors select-none hover:bg-surface-hi/60 hover:text-ink [&::-webkit-details-marker]:hidden">
        <ChevronRight className="size-4 text-ink-3 transition-transform duration-200 group-open/more:rotate-90" aria-hidden />
        {label}
      </summary>
      <div className="flex flex-col gap-3.5 px-4 pt-1 pb-4">{children}</div>
    </details>
  );
}

/** A row that leads somewhere: another selection, or another page. */
export function NavRow({ icon: Icon, title, detail, trailing, onClick }: { icon: LucideIcon; title: string; detail?: ReactNode; trailing?: ReactNode; onClick: () => void }) {
  return (
    <button type="button" onClick={onClick} className="group -mx-2 flex min-h-11 cursor-default items-center gap-3 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-surface-hi">
      <span className="grid size-8 shrink-0 place-items-center rounded-md bg-surface-hi text-ink-2 ring-1 ring-line-strong transition-colors group-hover:bg-surface-top group-hover:text-ink">
        <Icon className="size-4" aria-hidden />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px] font-medium text-ink">{title}</span>
        {detail && <span className="block truncate text-[12px] text-ink-3">{detail}</span>}
      </span>
      {trailing && <span className="shrink-0 text-[12px] text-ink-3">{trailing}</span>}
      <ChevronRight className="size-4 shrink-0 text-ink-4 transition-colors group-hover:text-ink-2" aria-hidden />
    </button>
  );
}

export type Tile<T extends string> = { value: T; label: string; preview: ReactNode; hint?: string };

/**
 * The one picker: visual tiles, one chosen (a radio group; arrow keys move the choice). Themes,
 * presets, styles, surfaces, fits, corners and pointers all use it.
 */
export function TilePicker<T extends string>({
  label,
  value,
  options,
  onChange,
  min = "5.25rem",
  columns,
  aspect = "aspect-[4/3]",
}: {
  label: string;
  /** Null: none of the tiles matches (a mix of their own). */
  value: T | null;
  options: readonly Tile<T>[];
  onChange: (value: T) => void;
  /** The smallest tile width; tiles fill the row. */
  min?: string;
  /** A fixed number of tiles per row instead (so a set of five stays one row). */
  columns?: number;
  /** The preview's shape. */
  aspect?: string;
}) {
  const group = useRef<HTMLDivElement>(null);
  const focusable = value !== null && options.some((o) => o.value === value) ? value : options[0]?.value;

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[event.key];
    if (step === undefined) return;
    event.preventDefault();
    const index = options.findIndex((o) => o.value === focusable);
    const next = options[(index + step + options.length) % options.length];
    if (!next) return;
    onChange(next.value);
    group.current?.querySelector<HTMLElement>(`[data-tile="${CSS.escape(next.value)}"]`)?.focus();
  }

  const grid: CSSProperties = { gridTemplateColumns: columns ? `repeat(${columns}, minmax(0, 1fr))` : `repeat(auto-fill, minmax(${min}, 1fr))` };
  return (
    <div ref={group} role="radiogroup" aria-label={label} onKeyDown={onKeyDown} className="grid gap-2" style={grid}>
      {options.map((option) => {
        const on = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={on}
            tabIndex={option.value === focusable ? 0 : -1}
            data-tile={option.value}
            title={option.hint}
            onClick={() => onChange(option.value)}
            className={`relative min-w-0 cursor-default rounded-[10px] p-1 text-left transition-[box-shadow,background-color,color] duration-150 ${
              on ? "bg-surface-top text-ink ring-2 ring-ink" : "bg-surface text-ink-3 ring-1 ring-line-strong hover:bg-surface-hi hover:text-ink hover:ring-line-hi"
            }`}
          >
            <span className={`relative block overflow-hidden rounded-md bg-canvas ${aspect}`} aria-hidden>
              {option.preview}
            </span>
            <span className="flex items-center gap-1 px-1 pt-1.5 pb-0.5 text-[12px] leading-tight font-medium">
              <span className="min-w-0 flex-1 truncate">{option.label}</span>
              {on && <Check className="size-3.5 shrink-0" aria-hidden />}
            </span>
          </button>
        );
      })}
    </div>
  );
}

/**
 * The one list: rows with a grip to drag into a new order (the grip also takes the up and down
 * arrow keys), then the row's own content and actions.
 */
export function SortableList<T>({
  label,
  items,
  keyOf,
  renderItem,
  onReorder,
}: {
  label: string;
  items: readonly T[];
  keyOf: (item: T, index: number) => string;
  /** A row's content after the grip. */
  renderItem: (item: T, index: number) => ReactNode;
  /** Moves the item at `from` so it ends up at `to`. */
  onReorder: (from: number, to: number) => void;
}) {
  const list = useRef<HTMLOListElement>(null);
  const hint = useId();
  const [drag, setDrag] = useState<{ from: number; to: number; dy: number; height: number } | null>(null);

  function start(event: ReactPointerEvent<HTMLButtonElement>, index: number) {
    if (event.button !== 0) return;
    const rows = [...(list.current?.children ?? [])].map((row) => row.getBoundingClientRect());
    const row = rows[index];
    if (!row) return;
    event.preventDefault();
    const startY = event.clientY;
    let last = { from: index, to: index, dy: 0, height: row.height };
    setDrag(last);
    const move = (e: PointerEvent) => {
      const dy = e.clientY - startY;
      const middle = row.top + row.height / 2 + dy;
      const to = rows.filter((r, i) => i !== index && r.top + r.height / 2 < middle).length;
      last = { from: index, to, dy, height: row.height };
      setDrag(last);
    };
    const end = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      setDrag(null);
      if (last.to !== last.from) onReorder(last.from, last.to);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  }

  function onKey(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    const to = event.key === "ArrowUp" ? index - 1 : event.key === "ArrowDown" ? index + 1 : null;
    if (to === null || to < 0 || to >= items.length) return;
    event.preventDefault();
    onReorder(index, to);
    requestAnimationFrame(() => list.current?.querySelectorAll<HTMLElement>("[data-grip]")[to]?.focus());
  }

  const shift = (index: number): number => {
    if (!drag || index === drag.from) return 0;
    if (drag.from < index && index <= drag.to) return -drag.height;
    if (drag.to <= index && index < drag.from) return drag.height;
    return 0;
  };

  return (
    <>
      <span id={hint} className="sr-only">
        Drag, or use the up and down arrow keys, to move it.
      </span>
      <ol ref={list} aria-label={label} className="flex flex-col rounded-lg bg-canvas p-1 ring-1 ring-line-strong ring-inset">
        {items.map((item, index) => {
          const dragging = drag?.from === index;
          const style: CSSProperties = { transform: `translateY(${dragging ? drag.dy : shift(index)}px)` };
          return (
            <li
              key={keyOf(item, index)}
              style={style}
              className={`group/row relative flex min-h-9 items-center gap-1 rounded-md pr-1 ${dragging ? "z-10 bg-surface-top shadow-pop ring-1 ring-line-hi" : "transition-transform duration-150 hover:bg-surface-hi"}`}
            >
              <button
                type="button"
                data-grip
                aria-label={`Move item ${index + 1}`}
                aria-describedby={hint}
                onPointerDown={(e) => start(e, index)}
                onKeyDown={(e) => onKey(e, index)}
                className="grid h-8 w-6 shrink-0 cursor-grab touch-none place-items-center rounded text-ink-4 transition-colors group-hover/row:text-ink-3 hover:text-ink active:cursor-grabbing"
              >
                <GripVertical className="size-3.5" aria-hidden />
              </button>
              {renderItem(item, index)}
            </li>
          );
        })}
      </ol>
    </>
  );
}

/** `list` with the item at `from` moved so it ends up at `to`. */
export function moved<T>(list: readonly T[], from: number, to: number): T[] {
  const item = list[from];
  if (item === undefined || from === to) return [...list];
  return list.toSpliced(from, 1).toSpliced(to, 0, item);
}
