import { Check, ChevronDown, LoaderCircle, Minus, Plus, TriangleAlert, type LucideIcon } from "lucide-react";
import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type ComponentProps,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";

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
              size === "sm" ? "h-6 px-2 text-[11.5px]" : "h-7 px-3 text-[12.5px]"
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

type TabOption<T extends string> = { value: T; label: string; count?: number; icon?: LucideIcon };

/**
 * Page-level tabs: text with an underline under the current one. Arrow keys, Home and End move
 * between them (and select, like the platform's tabs). Put them on a bottom border.
 */
export function Tabs<T extends string>({
  label,
  value,
  options,
  onChange,
  className = "",
}: {
  label: string;
  value: T;
  options: readonly TabOption<T>[];
  onChange: (value: T) => void;
  className?: string;
}) {
  const list = useRef<HTMLDivElement>(null);

  function onKeyDown(event: KeyboardEvent) {
    const index = options.findIndex((o) => o.value === value);
    const next = { ArrowRight: index + 1, ArrowLeft: index - 1, Home: 0, End: options.length - 1 }[event.key];
    if (next === undefined) return;
    event.preventDefault();
    const target = options[(next + options.length) % options.length];
    if (!target) return;
    onChange(target.value);
    list.current?.querySelector<HTMLElement>(`[data-tab="${target.value}"]`)?.focus();
  }

  return (
    <div ref={list} role="tablist" aria-label={label} onKeyDown={onKeyDown} className={`flex min-w-0 gap-1 overflow-x-auto [scrollbar-width:none] ${className}`}>
      {options.map(({ value: tab, label: text, count, icon: Icon }) => {
        const selected = tab === value;
        return (
          <button
            key={tab}
            type="button"
            role="tab"
            data-tab={tab}
            aria-selected={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(tab)}
            className={`group relative flex h-10 shrink-0 cursor-default items-center pb-px text-[13px] font-medium transition-colors focus-visible:outline-offset-[-2px] ${
              selected ? "text-ink" : "text-ink-3 hover:text-ink"
            }`}
          >
            <span className="flex items-center gap-2 rounded-md px-2.5 py-1 transition-colors group-hover:bg-surface-hi">
              {Icon && <Icon className="size-4" aria-hidden />}
              {text}
              {count !== undefined && (
                <span className={`rounded-[5px] px-1.5 text-[11px] leading-[18px] tabular-nums transition-colors ${selected ? "bg-surface-top text-ink" : "bg-surface-hi text-ink-3"}`}>{count}</span>
              )}
            </span>
            <span
              aria-hidden
              className={`absolute inset-x-1.5 -bottom-px h-0.5 rounded-full bg-ink transition-[opacity,transform] duration-200 ${selected ? "scale-x-100 opacity-100" : "scale-x-50 opacity-0"}`}
            />
          </button>
        );
      })}
    </div>
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
        className={`ui-input w-full cursor-default truncate bg-surface-hi pl-3 hover:bg-surface-top ${size === "sm" ? "h-[30px] text-[12.5px]" : "text-[13px]"}`}
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
  /** 32 px tall instead of 36. */
  compact?: boolean;
};

/** A styled `<input>`; widths go in `className` (it has none of its own). */
export function TextInput({ mono = false, compact = false, className = "", ...rest }: TextInputProps) {
  return (
    <input
      spellCheck={mono ? false : undefined}
      className={`${INPUT_BASE} ${compact ? "h-8 px-2.5 text-[13px]" : ""} ${mono ? "font-mono text-[12.5px]" : ""} ${className}`}
      {...rest}
    />
  );
}

/** A label over one control, with an optional description under it. */
export function Field({ label, description, children, className = "" }: { label: string; description?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <label className={`block min-w-0 ${className}`}>
      <span className="mb-1.5 block text-[13px] font-medium text-ink">{label}</span>
      {children}
      {description && <span className="mt-1.5 block text-[12px] leading-snug text-ink-3">{description}</span>}
    </label>
  );
}

/** `Field` around a full-width `TextInput`. */
export function TextField({ label, description, className = "", ...input }: TextInputProps & { label: string; description?: ReactNode }) {
  return (
    <Field label={label} description={description} className={className}>
      <TextInput className="w-full" {...input} />
    </Field>
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
          className="ui-input h-7 w-[4.25rem] shrink-0 rounded-md px-2 text-right font-mono text-[12px] tabular-nums"
        />
      ) : (
        <output className="w-[4.25rem] shrink-0 pr-2 text-right font-mono text-[12px] text-ink-2 tabular-nums">{show(value)}</output>
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
      <input aria-label={label} inputMode="decimal" disabled={disabled} {...draft} className="h-full w-full flex-1 text-center font-mono text-[12.5px] tabular-nums" />
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
      <label className="relative size-7 shrink-0 cursor-default overflow-hidden rounded-md shadow-[inset_0_0_0_1px_rgb(255_255_255/0.12),0_0_0_1px_rgb(0_0_0/0.3)]" style={{ background: hex ?? "repeating-conic-gradient(#777 0 25%, #444 0 50%) 0 0 / 8px 8px" }}>
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
        className={`selectable h-full flex-1 ${mono ? "font-mono text-[12.5px]" : "text-[13px]"}`}
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
        className="grid size-7 shrink-0 cursor-default place-items-center rounded-md text-ink-3 hover:bg-surface-hi hover:text-ink"
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
            {option.hint && <span className="shrink-0 truncate text-xs text-ink-3">{option.hint}</span>}
          </div>
        ))}
      </div>
    </div>
  );
}

/** A short explanation under a control, in the muted tone. */
export function Note({ children }: { children: ReactNode }) {
  return <p className="mt-2 text-[12.5px] leading-snug text-ink-3">{children}</p>;
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

const HEADER_TITLE = "font-display text-[22px] leading-tight font-semibold tracking-tight text-ink";

/** A page title with its actions, in the page's flow. Inside `Page`, use its props instead. */
export function PageHeader({ title, subtitle, actions }: { title: string; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="mb-6 flex flex-wrap items-end justify-between gap-x-6 gap-y-3 pt-7">
      <div className="min-w-0">
        <h1 className={HEADER_TITLE}>{title}</h1>
        {subtitle && <p className="mt-1 truncate text-[13px] text-ink-3">{subtitle}</p>}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </header>
  );
}

const PAGE_WIDTHS = { full: "", wide: "max-w-[100rem]", narrow: "max-w-[68rem]" } as const;

type PageProps = {
  title: string;
  subtitle?: ReactNode;
  actions?: ReactNode;
  /** `Tabs`, shown on the header's bottom edge. */
  tabs?: ReactNode;
  /** `full` fills the window (grids); `wide` and `narrow` center a block. Header and content share it. */
  width?: keyof typeof PAGE_WIDTHS;
  children: ReactNode;
};

/**
 * A page: a sticky header (title, actions, tabs) over its content. The header's height is
 * `--header-h` for anything else that sticks (`top-[calc(var(--header-h)+1rem)]`).
 * Expects the shell's scroller, which sets `--page-px`.
 */
export function Page({ title, subtitle, actions, tabs, width = "full", children }: PageProps) {
  const header = useRef<HTMLElement>(null);
  const [height, setHeight] = useState(0);
  useLayoutEffect(() => {
    const element = header.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => entry && setHeight(entry.borderBoxSize[0]?.blockSize ?? element.offsetHeight));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const max = PAGE_WIDTHS[width];
  const vars: CSSProperties & Record<"--header-h", string> = { "--header-h": `${height}px` };
  return (
    <div style={vars}>
      <header
        ref={header}
        className={`page-header sticky top-0 z-20 -mx-(--page-px) border-b bg-canvas px-(--page-px) backdrop-blur-xl ${tabs ? "border-line" : "border-transparent"}`}
      >
        <div className={`mx-auto ${max}`}>
          <div className={`flex flex-wrap items-end justify-between gap-x-6 gap-y-3 pt-6 ${tabs ? "pb-2" : "pb-4"}`}>
            <div className="min-w-0">
              <h1 className={HEADER_TITLE}>{title}</h1>
              {subtitle && <p className="mt-1 truncate text-[13px] text-ink-3">{subtitle}</p>}
            </div>
            {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
          </div>
          {tabs && <div className="-mx-2.5">{tabs}</div>}
        </div>
      </header>
      <div className={`mx-auto pt-6 ${max}`}>{children}</div>
    </div>
  );
}

/** A heading for a run of content inside a page. */
export function SectionLabel({ children, id }: { children: ReactNode; id?: string }) {
  return (
    <h2 id={id} className="mb-3 text-[13.5px] font-semibold text-ink">
      {children}
    </h2>
  );
}

export function Keycap({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex h-6 min-w-6 items-center justify-center rounded-md bg-surface-hi px-1.5 font-sans text-[11.5px] font-medium text-ink-2 shadow-[inset_0_-1px_0_var(--line-hi),inset_0_0_0_1px_var(--line-strong)]">
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
      <p className="font-display text-base font-semibold text-ink">{title}</p>
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

/** "Saving" while an edit is on its way to disk, otherwise a calm confirmation. */
export function SaveStatus({ saving }: { saving: boolean }) {
  return (
    <span className="flex items-center gap-1.5 text-xs text-ink-3" aria-live="polite">
      {saving ? <LoaderCircle className="size-3.5 animate-spin" aria-hidden /> : <Check className="size-3.5" aria-hidden />}
      {saving ? "Saving" : "Saved as you go"}
    </span>
  );
}

/** A plain card: the surface, its edge and padding (`flush`: none, for cards with their own sections). */
export function Card({ children, className = "", flush = false }: { children: ReactNode; className?: string; flush?: boolean }) {
  return <div className={`rounded-xl bg-surface shadow-card ring-1 ring-line-strong ${flush ? "" : "p-5"} ${className}`}>{children}</div>;
}

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
  return (
    <section aria-label={title} className={className}>
      {(title || action) && (
        <div className="mb-2.5 flex items-end justify-between gap-3 px-0.5">
          <div className="min-w-0">
            {title && <h2 className="text-[13.5px] font-semibold text-ink">{title}</h2>}
            {description && <p className="mt-0.5 text-[12.5px] text-ink-3">{description}</p>}
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
  const titleClass = "block text-[13.5px] font-medium text-ink";
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
            className={`mt-0.5 max-w-[72ch] text-[12.5px] leading-relaxed text-ink-3 ${mono ? "selectable truncate font-mono text-xs" : ""}`}
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
  return (
    <div className="px-4 py-3.5">
      <div className="text-[13.5px] font-medium text-ink">{title}</div>
      {detail && <div className="mt-0.5 max-w-[72ch] text-[12.5px] leading-relaxed text-ink-3">{detail}</div>}
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
