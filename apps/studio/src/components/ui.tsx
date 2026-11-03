import { LoaderCircle, TriangleAlert, type LucideIcon } from "lucide-react";
import { useId, type ButtonHTMLAttributes, type ReactNode } from "react";

// Shared primitives. Colors come only from the tokens in index.css; selected
// and primary states are always the inverse fill (`solid`), never a hue.

const VARIANTS = {
  primary: "bg-solid text-solid-ink hover:opacity-85 active:opacity-70",
  secondary: "bg-surface-hi text-ink ring-1 ring-line-strong hover:ring-line-hi",
  ghost: "text-ink-2 hover:bg-surface-hi hover:text-ink",
} as const;

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: keyof typeof VARIANTS;
  icon?: LucideIcon;
  /** Shows a spinner in place of the icon and disables the button. */
  busy?: boolean;
};

export function Button({
  variant = "secondary",
  icon: Icon,
  busy = false,
  disabled,
  className = "",
  children,
  ...rest
}: ButtonProps) {
  return (
    <button
      type="button"
      disabled={disabled || busy}
      className={`inline-flex h-9 cursor-default items-center justify-center gap-2 rounded-lg px-3.5 text-[13px] font-medium whitespace-nowrap transition duration-150 disabled:opacity-40 ${VARIANTS[variant]} ${className}`}
      {...rest}
    >
      {busy ? <LoaderCircle className="size-4 animate-spin" aria-hidden /> : Icon && <Icon className="size-4" aria-hidden />}
      {children}
    </button>
  );
}

/** A square icon-only button; `label` is its accessible name and tooltip. */
export function IconButton({
  icon: Icon,
  label,
  className = "",
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { icon: LucideIcon; label: string }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={`grid size-9 shrink-0 cursor-default place-items-center rounded-lg text-ink-2 ring-1 ring-line-strong transition duration-150 hover:bg-surface-hi hover:text-ink hover:ring-line-hi disabled:opacity-40 ${className}`}
      {...rest}
    >
      <Icon className="size-4" aria-hidden />
    </button>
  );
}

type SwitchProps = {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
};

/** An accessible on/off switch; the visible label lives in the row around it. */
export function Switch({ checked, onChange, label, disabled }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-6 w-11 shrink-0 cursor-default rounded-full transition-colors duration-200 disabled:opacity-40 ${
        checked ? "bg-solid" : "bg-surface-hi ring-1 ring-line-strong ring-inset"
      }`}
    >
      <span
        className={`absolute top-1 left-1 size-4 rounded-full transition-transform duration-200 ease-out ${
          checked ? "translate-x-5 bg-solid-ink" : "bg-ink-3"
        }`}
      />
    </button>
  );
}

type SegmentedProps<T extends string> = {
  label: string;
  value: T;
  options: readonly { value: T; label: string; hint?: string }[];
  onChange: (value: T) => void;
  /** Hide the legend visually (it stays for screen readers). */
  hideLabel?: boolean;
  className?: string;
};

/** A radio group drawn as a segmented control; the selected segment is the inverse fill. */
export function Segmented<T extends string>({ label, value, options, onChange, hideLabel, className = "" }: SegmentedProps<T>) {
  const name = useId();
  return (
    <fieldset className={className}>
      <legend className={hideLabel ? "sr-only" : "mb-1.5 text-[13px] font-medium text-ink-2"}>{label}</legend>
      <div className="flex flex-wrap gap-1 rounded-xl bg-canvas p-1 ring-1 ring-line">
        {options.map((option) => (
          <label
            key={option.value}
            title={option.hint}
            className="relative flex-1 cursor-default rounded-lg px-3 py-1.5 text-center text-[13px] font-medium whitespace-nowrap text-ink-3 transition-colors hover:text-ink has-checked:bg-solid has-checked:text-solid-ink has-focus-visible:outline-2 has-focus-visible:outline-offset-2 has-focus-visible:outline-ink"
          >
            <input
              type="radio"
              name={name}
              value={option.value}
              checked={value === option.value}
              onChange={() => onChange(option.value)}
              className="sr-only"
            />
            {option.label}
          </label>
        ))}
      </div>
    </fieldset>
  );
}

/** Text input styled for the monochrome surfaces. */
export const INPUT_CLASS =
  "selectable h-10 w-full rounded-lg bg-canvas px-3 text-[14px] text-ink ring-1 ring-line-strong transition placeholder:text-ink-3 hover:ring-line-hi focus-visible:ring-2 focus-visible:ring-ink focus-visible:outline-none";

export function PageHeader({ title, subtitle, actions }: { title: string; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="mb-7 flex items-end justify-between gap-6">
      <div className="min-w-0">
        <h1 className="font-display text-[28px] leading-tight font-semibold tracking-tight text-ink">{title}</h1>
        {subtitle && <p className="mt-1 truncate text-[13px] text-ink-3">{subtitle}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </header>
  );
}

export function SectionLabel({ children, id }: { children: ReactNode; id?: string }) {
  return (
    <h2 id={id} className="mb-3 text-xs font-semibold tracking-[0.12em] text-ink-3 uppercase">
      {children}
    </h2>
  );
}

export function Keycap({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex h-6 min-w-6 items-center justify-center rounded-md border border-line-strong border-b-2 bg-surface-hi px-1.5 font-sans text-xs font-medium text-ink-2">
      {children}
    </kbd>
  );
}

/** An error or warning strip. Meaning comes from the icon and wording, not from color. */
export function Banner({ children }: { children: ReactNode }) {
  return (
    <div
      role="alert"
      className="selectable mb-6 flex items-center gap-3 rounded-xl border border-line-hi bg-surface px-4 py-3 text-[13px] text-ink"
    >
      <TriangleAlert className="size-4 shrink-0" aria-hidden />
      <div className="flex min-w-0 flex-1 items-center justify-between gap-4">{children}</div>
    </div>
  );
}

/** Centered placeholder for lists with nothing in them (or nothing that matches). */
export function EmptyState({
  icon: Icon,
  title,
  children,
  action,
}: {
  icon: LucideIcon;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="grid place-items-center rounded-2xl border border-dashed border-line-strong px-8 py-20 text-center">
      <span className="mb-4 grid size-12 place-items-center rounded-2xl bg-surface-hi text-ink-2">
        <Icon className="size-6" strokeWidth={1.5} aria-hidden />
      </span>
      <p className="font-display text-lg font-semibold text-ink">{title}</p>
      {children && <div className="selectable mt-1 max-w-md text-[13px] text-ink-3">{children}</div>}
      {action && <div className="mt-5">{action}</div>}
    </div>
  );
}

/** A pulsing placeholder block while something loads. */
export function Skeleton({ className = "" }: { className?: string }) {
  return (
    <div
      aria-hidden
      className={`animate-[shimmer_1.6s_linear_infinite] bg-[linear-gradient(110deg,var(--surface-hi)_30%,var(--line-strong)_50%,var(--surface-hi)_70%)] bg-[length:200%_100%] ${className}`}
    />
  );
}

/** Engine / activity indicator that does not rely on color: filled = on, ring = off. */
export function StatusDot({ state }: { state: "on" | "off" | "unknown" }) {
  return (
    <span className="relative flex size-2.5 shrink-0" aria-hidden>
      {state === "on" && <span className="absolute inset-0 animate-pulse-ring rounded-full bg-ink" />}
      <span
        className={`relative size-2.5 rounded-full ${
          state === "on" ? "bg-ink" : state === "off" ? "ring-[1.5px] ring-ink-3 ring-inset" : "bg-line-strong"
        }`}
      />
    </span>
  );
}
