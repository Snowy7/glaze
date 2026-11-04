import { FolderPlus, Pause, Play, Plus, SearchX, SlidersHorizontal } from "lucide-react";
import { useMemo, useState } from "react";

import type { View } from "../components/Sidebar";
import { CustomizeSheet } from "../components/CustomizeSheet";
import { KindBadge, looks, ORIGIN_LABEL, ThemeCard, ThemePreview } from "../components/ThemeCard";
import { Badge, Banner, Button, EmptyState, Page, Segmented, Skeleton, StatusDot } from "../components/ui";
import { fileUrl, type ThemeEntry } from "../lib/api";
import { KINDS } from "../lib/kinds";
import { useStudio } from "../lib/studio";

type Filter = "all" | "user" | "bundled";

const GRID = "grid grid-cols-[repeat(auto-fill,minmax(17.5rem,1fr))] gap-4 2xl:gap-5";

export function Gallery({ onNavigate }: { onNavigate: (view: View) => void }) {
  const { themes, roots, themesLoaded, config, configError, updateConfig, resetConfig, busy } = useStudio();
  const [filter, setFilter] = useState<Filter>("all");
  // Held by id so the sheet follows the list when it refreshes.
  const [sheetId, setSheetId] = useState<string | null>(null);

  const active = themes.find((t) => t.active);
  const sheetTheme = themes.find((t) => t.id === sheetId) ?? null;
  const yours = themes.filter((t) => t.origin !== "bundled").length;

  const shown = useMemo(() => {
    const matches = (t: ThemeEntry) => filter === "all" || (filter === "user" ? t.origin !== "bundled" : t.origin === "bundled");
    // Broken themes sink to the end so the usable ones lead.
    return themes.filter(matches).toSorted((a, b) => Number(a.error !== null) - Number(b.error !== null));
  }, [themes, filter]);

  const pauseButton = config && (
    <Button icon={config.paused ? Play : Pause} onClick={() => void updateConfig({ paused: !config.paused })} aria-pressed={config.paused}>
      {config.paused ? "Resume wallpaper" : "Pause wallpaper"}
    </Button>
  );

  return (
    <Page
      title="Gallery"
      subtitle={!themesLoaded ? "Loading themes" : `${themes.length} ${themes.length === 1 ? "theme" : "themes"}${yours > 0 ? ` · ${yours} of your own` : ""}`}
      actions={
        <>
          {!active && pauseButton}
          <Button variant="primary" icon={Plus} onClick={() => onNavigate("new")}>
            New theme
          </Button>
        </>
      }
    >
      {configError && (
        <Banner>
          <span>config.json can't be read: {configError}</span>
          <Button busy={busy.has("config")} onClick={() => void resetConfig()}>
            Reset to defaults
          </Button>
        </Banner>
      )}

      {!themesLoaded ? (
        <>
          <Skeleton className="mb-8 h-56 rounded-2xl" />
          <SkeletonGrid />
        </>
      ) : themes.length === 0 ? (
        <EmptyState
          icon={FolderPlus}
          title="No themes yet"
          action={
            <Button variant="primary" icon={Plus} onClick={() => onNavigate("new")}>
              Create a theme
            </Button>
          }
        >
          Studio looks in {roots.length > 0 ? roots.join(" and ") : "your themes folder and the themes folder beside the app"}.
        </EmptyState>
      ) : (
        <>
          {active && config && (
            <NowShowing theme={active} paused={config.paused} onTogglePause={() => void updateConfig({ paused: !config.paused })} onCustomize={() => setSheetId(active.id)} />
          )}

          <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
            <h2 className="text-[13.5px] font-semibold text-ink">
              {filter === "all" ? "All themes" : filter === "user" ? "Your themes" : "Built-in themes"}
              <span className="ml-2 font-normal text-ink-3 tabular-nums">{shown.length}</span>
            </h2>
            <Segmented<Filter>
              label="Show"
              hideLabel
              size="sm"
              value={filter}
              onChange={setFilter}
              options={[
                { value: "all", label: "All" },
                { value: "user", label: "Yours" },
                { value: "bundled", label: "Built in" },
              ]}
            />
          </div>

          {shown.length === 0 ? (
            <EmptyState
              icon={SearchX}
              title={filter === "user" ? "None of your own yet" : "No built-in themes found"}
              action={
                <Button icon={Plus} onClick={() => onNavigate("new")}>
                  Create a theme
                </Button>
              }
            >
              Create one from a picture, video or page, or open a built-in theme's Customize and duplicate it.
            </EmptyState>
          ) : (
            <div className={GRID}>
              {shown.map((theme, index) => (
                <ThemeCard key={theme.id} theme={theme} index={index} onCustomize={(t) => setSheetId(t.id)} />
              ))}
            </div>
          )}
        </>
      )}

      {sheetTheme && <CustomizeSheet theme={sheetTheme} onClose={() => setSheetId(null)} onDuplicated={(copy) => setSheetId(copy.id)} />}
    </Page>
  );
}

/** The theme on the desktop right now, large, with what you do to it: pause and customize. */
function NowShowing({ theme, paused, onTogglePause, onCustomize }: { theme: ThemeEntry; paused: boolean; onTogglePause: () => void; onCustomize: () => void }) {
  return (
    <section aria-label="Active theme" className="@container relative isolate mb-9 overflow-hidden rounded-2xl bg-surface shadow-card ring-1 ring-line-strong">
      {/* The picture again, blurred, as ambient light behind the card. */}
      {theme.preview && <img src={fileUrl(theme.preview)} alt="" aria-hidden className="absolute inset-0 -z-10 size-full scale-125 object-cover opacity-30 blur-3xl saturate-150" />}
      <div className="absolute inset-0 -z-10 bg-linear-to-r from-transparent via-surface/60 to-surface" aria-hidden />

      <div className="flex flex-col gap-5 p-3 @lg:flex-row @lg:items-center @lg:gap-7">
        <div className="group aspect-[16/10] w-full shrink-0 overflow-hidden rounded-xl shadow-card ring-1 ring-white/10 @lg:w-[clamp(15rem,30%,28rem)]">
          <ThemePreview name={theme.name} kind={theme.kind} preview={theme.preview} zoom />
        </div>
        <div className="min-w-0 flex-1 px-2 pb-3 @lg:py-3 @lg:pr-6 @lg:pl-0">
          <p className="flex items-center gap-2 text-[12px] font-medium text-ink-2">
            {paused ? <Pause className="size-2.5 fill-current" aria-hidden /> : <StatusDot state="on" />}
            {paused ? "Paused" : "On your desktop now"}
          </p>
          <h2 className="mt-2 truncate font-display text-[30px] leading-tight font-semibold tracking-tight text-ink">{theme.name}</h2>
          <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
            <KindBadge kind={theme.kind} tone="neutral" />
            <Badge tone="outline">{ORIGIN_LABEL[theme.origin]}</Badge>
            {looks(theme).map((look) => (
              <Badge key={look.label} tone="outline" title={look.hint} className="@5xl:hidden">
                {look.label} look
              </Badge>
            ))}
          </div>
          <div className="mt-6 flex flex-wrap gap-2">
            <Button variant="primary" icon={paused ? Play : Pause} onClick={onTogglePause} aria-pressed={paused}>
              {paused ? "Resume" : "Pause"}
            </Button>
            <Button icon={SlidersHorizontal} onClick={onCustomize}>
              Customize
            </Button>
          </div>
        </div>

        <dl className="hidden w-[min(22rem,30%)] shrink-0 self-stretch border-l border-line py-3 pr-5 pl-6 @5xl:flex @5xl:flex-col @5xl:justify-center">
          {changes(theme).map(([what, how]) => (
            <div key={what} className="flex items-baseline justify-between gap-4 border-b border-line py-2.5 last:border-0">
              <dt className="text-[12.5px] text-ink-3">{what}</dt>
              <dd className={`text-right text-[13px] font-medium ${how ? "text-ink" : "text-ink-4"}`}>{how ?? "Not changed"}</dd>
            </div>
          ))}
        </dl>
      </div>
    </section>
  );
}

/** What applying the theme changes, as label and value (null: left as it is). */
function changes(theme: ThemeEntry): [string, string | null][] {
  return [
    ["Wallpaper", theme.kind ? KINDS[theme.kind].label : "Invalid"],
    ["Windows styling", theme.styled ? "Accent, borders, corners" : null],
    ["Widgets look", theme.widgetsSkin ? "Set by this theme" : null],
    ["Bars look", theme.barsSkin ? "Set by this theme" : null],
  ];
}

function SkeletonGrid() {
  return (
    <div aria-hidden className={GRID}>
      {Array.from({ length: 8 }, (_, i) => (
        <div key={i} className="rounded-xl bg-surface p-1.5 ring-1 ring-line-strong">
          <Skeleton className="aspect-[16/10] rounded-lg" />
          <div className="space-y-2 px-2 py-3">
            <Skeleton className="h-3.5 w-2/3 rounded" />
            <Skeleton className="h-3 w-1/3 rounded" />
          </div>
        </div>
      ))}
    </div>
  );
}
