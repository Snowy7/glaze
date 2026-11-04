import { Pencil, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { fileUrl, licenseName, type Kind, type ThemeEntry } from "../lib/api";
import { KINDS, placeholderBackground } from "../lib/kinds";
import { useStudio } from "../lib/studio";
import { Badge, IconButton } from "./ui";

export const ORIGIN_LABEL = { user: "Yours", bundled: "Built in", external: "Custom location" } as const;

type Props = {
  theme: ThemeEntry;
  index: number;
  /** Opens the theme editor (a built-in theme is copied there first). */
  onEdit: (theme: ThemeEntry) => void;
};

/** One theme in the gallery: click the picture to put it on (with an Undo); Edit opens the theme editor. */
export function ThemeCard({ theme, index, onEdit }: Props) {
  const { applyTheme, busy, pendingPreviews } = useStudio();
  const applying = busy.has(`apply:${theme.id}`);
  const broken = theme.error !== null;
  const credit = theme.credit;
  // A bundled theme says who made it and under what license; others say where they're from.
  const meta = theme.hidden
    ? ["From an earlier version"]
    : credit?.author
      ? [credit.author, ...(credit.license ? [licenseName(credit.license)] : [])]
      : [ORIGIN_LABEL[theme.origin], ...looks(theme).map((look) => look.label)];

  return (
    <article
      aria-label={theme.name}
      style={{ animationDelay: `${Math.min(index, 12) * 30}ms` }}
      className="group flex animate-[rise_360ms_ease-out_backwards] flex-col"
    >
      <button
        type="button"
        disabled={broken || applying}
        aria-label={theme.active ? `${theme.name}, on your desktop now` : `Use ${theme.name}`}
        onClick={() => !theme.active && void applyTheme(theme)}
        className={`relative aspect-[16/10] cursor-default overflow-hidden rounded-xl bg-surface-hi shadow-card transition-[transform,box-shadow] duration-200 enabled:hover:-translate-y-0.5 disabled:cursor-default ${
          theme.active ? "ring-2 ring-ink ring-offset-[3px] ring-offset-canvas" : "ring-1 ring-line-strong enabled:hover:ring-line-hi"
        }`}
      >
        <ThemePreview name={theme.name} kind={theme.kind} preview={theme.preview} loading={pendingPreviews.has(theme.id)} zoom />
        <span className="absolute inset-x-0 top-0 flex items-start justify-between gap-2 p-2">
          <KindBadge kind={theme.kind} />
          {theme.active && (
            <Badge tone="solid">
              <span className="size-1.5 rounded-full bg-current" aria-hidden />
              On now
            </Badge>
          )}
        </span>
        {credit?.noncommercial && (
          <span className="absolute bottom-2 left-2" title="Free for personal use. Don't sell it, and share changes under the same license.">
            <Badge tone="overlay">Non-commercial · ShareAlike</Badge>
          </span>
        )}
        {!theme.active && !broken && (
          <span className="absolute inset-0 grid place-items-center bg-black/35 opacity-0 transition-opacity duration-150 group-hover:opacity-100" aria-hidden>
            <span className="flex h-8 items-center gap-1.5 rounded-lg bg-white px-3.5 text-[13px] font-semibold text-black shadow-pop">{applying ? "Applying…" : "Use this theme"}</span>
          </span>
        )}
      </button>

      <div className="flex items-center gap-1.5 pt-2.5 pr-0.5 pl-1">
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-[14px] font-semibold tracking-tight text-ink" title={theme.name}>
            {/* The badge on the picture says it, so the title doesn't repeat it. */}
            {credit?.noncommercial ? theme.name.replace(/\s*\(non-commercial\)$/i, "") : theme.name}
          </h3>
          <p className="truncate text-[12px] text-ink-3" title={credit?.source ?? theme.dir}>
            {meta.join(" · ")}
          </p>
        </div>
        <IconButton icon={Pencil} size="sm" variant="ghost" label={theme.origin === "user" ? `Edit ${theme.name}` : `Copy and edit ${theme.name}`} disabled={broken} onClick={() => onEdit(theme)} />
      </div>
      {theme.error && (
        <p className="selectable mx-2 mb-1.5 flex items-start gap-1.5 text-[12px] text-ink-2" title={theme.error}>
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
    theme.styled && { label: "Windows", hint: "Changes window colors, corners and material" },
    theme.widgetsSkin && { label: "Widgets", hint: "Changes how widgets look" },
    theme.barsSkin && { label: "Bars", hint: "Changes how the top bar and dock look" },
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
      <Icon className={`relative size-8 text-white/45 ${loading ? "animate-pulse" : ""}`} strokeWidth={1.5} aria-hidden />
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
