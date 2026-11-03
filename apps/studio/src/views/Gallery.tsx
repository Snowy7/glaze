import { FolderPlus, Pause, Play, SearchX } from "lucide-react";
import { useMemo, useState } from "react";

import type { View } from "../components/Sidebar";
import { StyleSheet } from "../components/StyleSheet";
import { ThemeCard } from "../components/ThemeCard";
import { Banner, Button, EmptyState, PageHeader, Segmented, Skeleton } from "../components/ui";
import type { ThemeEntry } from "../lib/api";
import { useStudio } from "../lib/studio";

type Filter = "all" | "user" | "bundled";

export function Gallery({ onNavigate }: { onNavigate: (view: View) => void }) {
  const { themes, roots, themesLoaded, config, configError, updateConfig, resetConfig, busy } = useStudio();
  const [filter, setFilter] = useState<Filter>("all");
  // Held by id so the sheet follows the list when it refreshes.
  const [sheetId, setSheetId] = useState<string | null>(null);

  const active = themes.find((t) => t.active);
  const sheetTheme = themes.find((t) => t.id === sheetId) ?? null;

  const shown = useMemo(() => {
    const matches = (t: ThemeEntry) => filter === "all" || (filter === "user" ? t.origin !== "bundled" : t.origin === "bundled");
    // Broken themes sink to the end so the usable ones lead.
    return themes.filter(matches).toSorted((a, b) => Number(a.error !== null) - Number(b.error !== null));
  }, [themes, filter]);

  return (
    <>
      <PageHeader
        title="Gallery"
        subtitle={
          themesLoaded
            ? `${themes.length} ${themes.length === 1 ? "theme" : "themes"}${active ? ` · ${active.name} is active` : ""}`
            : "Loading themes"
        }
        actions={
          config && (
            <Button
              icon={config.paused ? Play : Pause}
              onClick={() => void updateConfig({ paused: !config.paused })}
              aria-pressed={config.paused}
            >
              {config.paused ? "Resume wallpaper" : "Pause wallpaper"}
            </Button>
          )
        }
      />

      {configError && (
        <Banner>
          <span>config.json can't be read: {configError}</span>
          <Button busy={busy.has("config")} onClick={() => void resetConfig()}>
            Reset to defaults
          </Button>
        </Banner>
      )}

      {!themesLoaded ? (
        <SkeletonGrid />
      ) : themes.length === 0 ? (
        <EmptyState
          icon={FolderPlus}
          title="No themes yet"
          action={
            <Button variant="primary" onClick={() => onNavigate("new")}>
              Create a theme
            </Button>
          }
        >
          Studio looks in {roots.length > 0 ? roots.join(" and ") : "your themes folder and the themes folder beside the app"}.
        </EmptyState>
      ) : (
        <>
          <Segmented<Filter>
            label="Show"
            hideLabel
            className="mb-5 w-fit"
            value={filter}
            onChange={setFilter}
            options={[
              { value: "all", label: "All" },
              { value: "user", label: "Yours" },
              { value: "bundled", label: "Built in" },
            ]}
          />
          {shown.length === 0 ? (
            <EmptyState icon={SearchX} title="Nothing here yet" action={<Button onClick={() => onNavigate("new")}>Create a theme</Button>}>
              You haven't made any themes of your own. Create one, or duplicate a built-in theme to customize it.
            </EmptyState>
          ) : (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(250px,1fr))] gap-5 pb-4">
              {shown.map((theme, index) => (
                <ThemeCard key={theme.id} theme={theme} index={index} onCustomize={(t) => setSheetId(t.id)} />
              ))}
            </div>
          )}
        </>
      )}

      {sheetTheme && <StyleSheet theme={sheetTheme} onClose={() => setSheetId(null)} onDuplicated={(copy) => setSheetId(copy.id)} />}
    </>
  );
}

function SkeletonGrid() {
  return (
    <div aria-hidden className="grid grid-cols-[repeat(auto-fill,minmax(250px,1fr))] gap-5">
      {Array.from({ length: 6 }, (_, i) => (
        <div key={i} className="rounded-2xl bg-surface p-1.5 ring-1 ring-line">
          <Skeleton className="aspect-[16/10] rounded-xl" />
          <div className="space-y-2 px-2.5 py-3">
            <Skeleton className="h-4 w-2/3 rounded-md" />
            <Skeleton className="h-3 w-1/4 rounded-md" />
          </div>
        </div>
      ))}
    </div>
  );
}
