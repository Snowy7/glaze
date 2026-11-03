import { LoaderCircle, type LucideIcon } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode } from "react";

const VARIANTS = {
  primary: "bg-accent text-accent-ink hover:brightness-110 active:brightness-95",
  secondary: "bg-surface-hi text-ink ring-1 ring-line-strong hover:ring-ink-3",
  ghost: "text-ink-2 hover:bg-surface-hi hover:text-ink",
  danger: "bg-surface-hi text-danger ring-1 ring-line-strong hover:ring-danger",
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
      className={`inline-flex h-9 cursor-default items-center justify-center gap-2 rounded-lg px-3.5 text-[13px] font-medium whitespace-nowrap transition duration-150 disabled:opacity-45 ${VARIANTS[variant]} ${className}`}
      {...rest}
    >
      {busy ? <LoaderCircle className="size-4 animate-spin" aria-hidden /> : Icon && <Icon className="size-4" aria-hidden />}
      {children}
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
      className={`relative h-6 w-11 shrink-0 cursor-default rounded-full transition-colors duration-200 disabled:opacity-45 ${
        checked ? "bg-accent" : "bg-surface-hi ring-1 ring-inset ring-line-strong"
      }`}
    >
      <span
        className={`absolute top-1 left-1 size-4 rounded-full shadow-sm transition-transform duration-200 ease-out ${
          checked ? "translate-x-5 bg-accent-ink" : "bg-ink-2"
        }`}
      />
    </button>
  );
}

export function PageHeader({ title, subtitle, actions }: { title: string; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="mb-7 flex items-end justify-between gap-6">
      <div className="min-w-0">
        <h1 className="font-display text-[28px] leading-tight font-semibold tracking-tight">{title}</h1>
        {subtitle && <p className="mt-1 truncate text-[13px] text-ink-2">{subtitle}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </header>
  );
}

export function Keycap({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex h-6 min-w-6 items-center justify-center rounded-md border border-line-strong border-b-2 bg-surface-hi px-1.5 font-sans text-xs font-medium text-ink-2">
      {children}
    </kbd>
  );
}

export function Banner({ tone, children }: { tone: "warn" | "danger"; children: ReactNode }) {
  const color = tone === "danger" ? "text-danger" : "text-warn";
  return (
    <div
      role="alert"
      className={`selectable mb-6 flex items-center justify-between gap-4 rounded-xl border border-line-strong bg-surface px-4 py-3 text-[13px] ${color}`}
    >
      {children}
    </div>
  );
}
