import { Palette, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { fileUrl, type ThemeEntry } from "../lib/api";
import { KINDS, placeholderBackground } from "../lib/kinds";
import { useStudio } from "../lib/studio";
import { Button, IconButton } from "./ui";

const ORIGIN_LABEL = { user: "Yours", bundled: "Built in", external: "Custom location" } as const;

type Props = {
  theme: ThemeEntry;
  index: number;
  /** Opens the Windows styling sheet for this theme. */
  onCustomize: (theme: ThemeEntry) => void;
};

export function ThemeCard({ theme, index, onCustomize }: Props) {
  const { applyTheme, busy, pendingPreviews } = useStudio();
  const applying = busy.has(`apply:${theme.id}`);
  const broken = theme.error !== null;

  return (
    <article
      aria-label={theme.name}
      style={{ animationDelay: `${Math.min(index, 12) * 35}ms` }}
      className={`group animate-[rise_380ms_ease-out_backwards] rounded-2xl bg-surface p-1.5 shadow-card ring-1 transition duration-200 focus-within:ring-line-hi hover:-translate-y-0.5 focus-within:-translate-y-0.5 ${
        theme.active ? "ring-line-hi" : "ring-line hover:ring-line-strong"
      }`}
    >
      <div className="relative aspect-[16/10] overflow-hidden rounded-xl bg-surface-hi">
        <Preview theme={theme} loading={pendingPreviews.has(theme.id)} />
        <div className="absolute inset-x-0 top-0 flex items-start justify-between gap-2 bg-gradient-to-b from-black/55 to-transparent p-2.5 pb-8">
          <KindBadge theme={theme} />
          {theme.active && (
            <span className="flex items-center gap-1.5 rounded-full bg-white px-2.5 py-1 text-[11px] font-semibold text-black">
              <span className="size-1.5 rounded-full bg-black" aria-hidden />
              Active
            </span>
          )}
        </div>
      </div>

      <div className="flex items-center gap-2 px-2.5 pt-3 pb-2">
        <div className="min-w-0 flex-1">
          <h3 className="truncate font-display text-[15px] font-semibold tracking-tight text-ink">{theme.name}</h3>
          <p className="flex items-center gap-1.5 truncate text-xs text-ink-3" title={theme.dir}>
            {ORIGIN_LABEL[theme.origin]}
            {theme.styled && (
              <>
                <span aria-hidden>·</span>
                <span title="Has Windows styling">Styled</span>
              </>
            )}
          </p>
        </div>
        <IconButton icon={Palette} label={`Windows styling for ${theme.name}`} disabled={broken} onClick={() => onCustomize(theme)} />
        {theme.active ? (
          <span className="px-2 text-[13px] font-medium text-ink-2">In use</span>
        ) : (
          <Button
            busy={applying}
            disabled={broken}
            onClick={() => void applyTheme(theme)}
            aria-label={`Apply ${theme.name}`}
            className="group-focus-within:bg-solid group-focus-within:text-solid-ink group-focus-within:ring-transparent group-hover:bg-solid group-hover:text-solid-ink group-hover:ring-transparent"
          >
            Apply
          </Button>
        )}
      </div>
      {theme.error && (
        <p className="selectable mx-2.5 mb-2 flex items-start gap-1.5 text-xs text-ink-2" title={theme.error}>
          <TriangleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          <span className="line-clamp-2">{errorSummary(theme.error)}</span>
        </p>
      )}
    </article>
  );
}

/** The reason without the leading "in <path>\theme.json:" context, which is long and not useful on a card. */
function errorSummary(error: string): string {
  return error.replace(/^in .*?theme\.json:\s*/i, "") || error;
}

function Preview({ theme, loading }: { theme: ThemeEntry; loading: boolean }) {
  const [failed, setFailed] = useState<string | null>(null);
  const zoom = "transition-transform duration-500 ease-out group-hover:scale-[1.045]";
  if (theme.preview && failed !== theme.preview) {
    const preview = theme.preview;
    return (
      <img
        src={fileUrl(preview)}
        alt=""
        loading="lazy"
        decoding="async"
        draggable={false}
        onError={() => setFailed(preview)}
        className={`size-full object-cover ${zoom}`}
      />
    );
  }
  const Icon = theme.kind ? KINDS[theme.kind].icon : TriangleAlert;
  return (
    <div className={`relative grid size-full place-items-center ${zoom}`} style={{ background: placeholderBackground(theme.name) }}>
      <div className="grain absolute inset-0" aria-hidden />
      <Icon className={`relative size-9 text-white/60 ${loading ? "animate-pulse" : ""}`} strokeWidth={1.4} aria-hidden />
    </div>
  );
}

function KindBadge({ theme }: { theme: ThemeEntry }) {
  const kind = theme.kind ? KINDS[theme.kind] : null;
  const Icon = kind?.icon ?? TriangleAlert;
  return (
    <span
      className="flex items-center gap-1.5 rounded-full bg-black/60 px-2.5 py-1 text-[11px] font-medium text-white backdrop-blur-md"
      title={kind?.hint ?? "theme.json could not be loaded"}
    >
      <Icon className="size-3" aria-hidden />
      {kind?.label ?? "Invalid"}
    </span>
  );
}
