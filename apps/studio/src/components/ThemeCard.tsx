import { Check, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { fileUrl, type ThemeEntry } from "../lib/api";
import { KINDS, placeholderBackground } from "../lib/kinds";
import { useStudio } from "../lib/studio";
import { Button } from "./ui";

const ORIGIN_LABEL = { user: "Yours", bundled: "Built in", external: "Custom location" } as const;

export function ThemeCard({ theme, index }: { theme: ThemeEntry; index: number }) {
  const { applyTheme, busy } = useStudio();
  const applying = busy.has(`apply:${theme.id}`);
  const broken = theme.error !== null;

  return (
    <article
      aria-label={theme.name}
      style={{ animationDelay: `${Math.min(index, 12) * 35}ms` }}
      className={`group animate-[rise_380ms_ease-out_backwards] rounded-2xl bg-surface p-1.5 shadow-card ring-1 transition duration-200 hover:-translate-y-0.5 focus-within:-translate-y-0.5 ${
        theme.active ? "ring-2 ring-accent" : "ring-line hover:ring-line-strong"
      }`}
    >
      <div className="relative aspect-[16/10] overflow-hidden rounded-xl bg-surface-hi">
        <Preview key={theme.preview ?? theme.id} theme={theme} />
        <div className="absolute inset-x-0 top-0 flex items-start justify-between gap-2 bg-gradient-to-b from-black/45 to-transparent p-2.5 pb-8">
          <KindBadge theme={theme} />
          {theme.active && (
            <span className="flex items-center gap-1 rounded-full bg-accent px-2 py-1 text-[11px] font-semibold text-accent-ink">
              <Check className="size-3" strokeWidth={3} aria-hidden />
              Active
            </span>
          )}
        </div>
      </div>

      <div className="flex items-center gap-3 px-2.5 pt-3 pb-2">
        <div className="min-w-0 flex-1">
          <h3 className="truncate font-display text-[15px] font-semibold tracking-tight">{theme.name}</h3>
          <p className="truncate text-xs text-ink-3" title={theme.dir}>
            {ORIGIN_LABEL[theme.origin]}
          </p>
        </div>
        {theme.active ? (
          <span className="px-2 text-[13px] font-medium text-accent">In use</span>
        ) : (
          <Button
            busy={applying}
            disabled={broken}
            onClick={() => void applyTheme(theme)}
            aria-label={`Apply ${theme.name}`}
            className="group-hover:bg-accent group-hover:text-accent-ink group-hover:ring-transparent group-focus-within:bg-accent group-focus-within:text-accent-ink group-focus-within:ring-transparent"
          >
            Apply
          </Button>
        )}
      </div>
      {theme.error && (
        <p className="selectable mx-2.5 mb-2 line-clamp-2 text-xs text-danger" title={theme.error}>
          {theme.error}
        </p>
      )}
    </article>
  );
}

function Preview({ theme }: { theme: ThemeEntry }) {
  const [failed, setFailed] = useState(false);
  if (theme.preview && !failed) {
    return (
      <img
        src={fileUrl(theme.preview)}
        alt=""
        loading="lazy"
        decoding="async"
        draggable={false}
        onError={() => setFailed(true)}
        className="size-full object-cover transition-transform duration-500 ease-out group-hover:scale-[1.045]"
      />
    );
  }
  const Icon = theme.kind ? KINDS[theme.kind].icon : TriangleAlert;
  return (
    <div className="grid size-full place-items-center transition-transform duration-500 ease-out group-hover:scale-[1.045]" style={{ background: placeholderBackground(theme.name) }}>
      <Icon className="size-9 text-white/55" strokeWidth={1.4} aria-hidden />
    </div>
  );
}

function KindBadge({ theme }: { theme: ThemeEntry }) {
  const kind = theme.kind ? KINDS[theme.kind] : null;
  const Icon = kind?.icon ?? TriangleAlert;
  return (
    <span
      className="flex items-center gap-1.5 rounded-full bg-black/45 px-2.5 py-1 text-[11px] font-medium text-white backdrop-blur-md"
      title={kind?.hint ?? "theme.json could not be loaded"}
    >
      <Icon className="size-3" aria-hidden />
      {kind?.label ?? "Invalid"}
    </span>
  );
}
