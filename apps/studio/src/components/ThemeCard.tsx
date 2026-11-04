import { SlidersHorizontal, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { fileUrl, type Kind, type ThemeEntry } from "../lib/api";
import { KINDS, placeholderBackground } from "../lib/kinds";
import { useStudio } from "../lib/studio";
import { Badge, Button, IconButton } from "./ui";

export const ORIGIN_LABEL = { user: "Yours", bundled: "Built in", external: "Custom location" } as const;

type Props = {
  theme: ThemeEntry;
  index: number;
  /** Opens the customize sheet for this theme. */
  onCustomize: (theme: ThemeEntry) => void;
};

/** One theme in the gallery: its preview, what it is, and Apply. */
export function ThemeCard({ theme, index, onCustomize }: Props) {
  const { applyTheme, busy, pendingPreviews } = useStudio();
  const applying = busy.has(`apply:${theme.id}`);
  const broken = theme.error !== null;
  const meta = [ORIGIN_LABEL[theme.origin], ...looks(theme).map((look) => look.label)];

  return (
    <article
      aria-label={theme.name}
      style={{ animationDelay: `${Math.min(index, 12) * 30}ms` }}
      className={`group flex animate-[rise_360ms_ease-out_backwards] flex-col rounded-xl bg-surface p-1.5 shadow-card ring-1 transition-[transform,box-shadow,background-color] duration-200 focus-within:-translate-y-0.5 hover:-translate-y-0.5 hover:bg-surface-hi/60 ${
        theme.active ? "ring-ink-3" : "ring-line-strong hover:ring-line-hi focus-within:ring-line-hi"
      }`}
    >
      <div className="relative aspect-[16/10] overflow-hidden rounded-lg bg-surface-hi">
        <ThemePreview name={theme.name} kind={theme.kind} preview={theme.preview} loading={pendingPreviews.has(theme.id)} zoom />
        <div className="absolute inset-x-0 top-0 flex items-start justify-between gap-2 p-2">
          <KindBadge kind={theme.kind} />
          {theme.active && (
            <Badge tone="solid">
              <span className="size-1.5 rounded-full bg-current" aria-hidden />
              Active
            </Badge>
          )}
        </div>
      </div>

      <div className="flex items-center gap-1.5 pt-2.5 pr-1 pb-1 pl-2">
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-[14px] font-semibold tracking-tight text-ink">{theme.name}</h3>
          <p className="truncate text-[12px] text-ink-3" title={theme.dir}>
            {meta.join(" · ")}
          </p>
        </div>
        <IconButton icon={SlidersHorizontal} size="sm" variant="ghost" label={`Customize ${theme.name}`} disabled={broken} onClick={() => onCustomize(theme)} />
        {theme.active ? (
          <span className="px-2 text-[12.5px] font-medium text-ink-3">In use</span>
        ) : (
          <Button size="sm" busy={applying} disabled={broken} onClick={() => void applyTheme(theme)} aria-label={`Apply ${theme.name}`}>
            Apply
          </Button>
        )}
      </div>
      {theme.error && (
        <p className="selectable mx-2 mb-1.5 flex items-start gap-1.5 text-xs text-ink-2" title={theme.error}>
          <TriangleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          <span className="line-clamp-2">{errorSummary(theme.error)}</span>
        </p>
      )}
    </article>
  );
}

/** What the theme restyles besides the wallpaper. */
export function looks(theme: ThemeEntry): { label: string; hint: string }[] {
  return [
    theme.styled && { label: "Windows", hint: "Styles window colors, corners and backdrop" },
    theme.widgetsSkin && { label: "Widgets", hint: "Sets how desktop widgets look" },
    theme.barsSkin && { label: "Bars", hint: "Sets how the top bar and dock look" },
  ].filter((look) => look !== false);
}

/** The reason without the leading "in <path>\theme.json:" context, which is long and not useful on a card. */
function errorSummary(error: string): string {
  return error.replace(/^in .*?theme\.json:\s*/i, "") || error;
}

/**
 * A theme's picture, or a grayscale placeholder seeded by its name with the kind's icon.
 * `zoom` scales it up a little while its `group` is hovered.
 */
export function ThemePreview({ name, kind, preview, loading = false, zoom = false }: { name: string; kind: Kind | null; preview: string | null; loading?: boolean; zoom?: boolean }) {
  const [failed, setFailed] = useState<string | null>(null);
  const motion = zoom ? "transition-transform duration-500 ease-out group-hover:scale-[1.04]" : "";
  if (preview && failed !== preview) {
    return (
      <img
        src={fileUrl(preview)}
        alt=""
        loading="lazy"
        decoding="async"
        draggable={false}
        onError={() => setFailed(preview)}
        className={`size-full animate-fade-in object-cover ${motion}`}
      />
    );
  }
  const Icon = kind ? KINDS[kind].icon : TriangleAlert;
  return (
    <div className={`relative grid size-full place-items-center ${motion}`} style={{ background: placeholderBackground(name) }}>
      <div className="grain absolute inset-0" aria-hidden />
      <Icon className={`relative size-9 text-white/55 ${loading ? "animate-pulse" : ""}`} strokeWidth={1.4} aria-hidden />
    </div>
  );
}

/** The wallpaper kind, as a badge that reads on any picture. */
export function KindBadge({ kind, tone = "overlay" }: { kind: Kind | null; tone?: "overlay" | "neutral" }) {
  const info = kind ? KINDS[kind] : null;
  return (
    <Badge tone={tone} icon={info?.icon ?? TriangleAlert} title={info?.hint ?? "theme.json could not be loaded"}>
      {info?.label ?? "Invalid"}
    </Badge>
  );
}
