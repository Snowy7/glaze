import { FolderPlus, Plus, Search, SearchX } from "lucide-react";
import { useMemo, useState } from "react";

import { ThemeCard } from "../components/ThemeCard";
import { Banner, Button, EmptyState, PageBar, Segmented, Skeleton, TextInput } from "../components/ui";
import type { ThemeEntry } from "../lib/api";
import { KINDS } from "../lib/kinds";
import { useNav } from "../lib/nav";
import { useStudio } from "../lib/studio";
import { NewTheme } from "./NewTheme";
import { ThemeEditor } from "./ThemeEditor";

type Filter = "all" | "user" | "bundled";

const GRID = "grid grid-cols-[repeat(auto-fill,minmax(15.5rem,1fr))] gap-x-4 gap-y-6 2xl:grid-cols-[repeat(auto-fill,minmax(19rem,1fr))] 2xl:gap-x-5";

/** Themes: the gallery (click to use), New theme, and the theme editor. */
export function Themes() {
  const { route } = useNav();
  if (route.page === "themes" && route.view === "new") return <NewTheme />;
  if (route.page === "themes" && route.view === "edit") return <ThemeEditor themeId={route.theme} />;
  return <Gallery />;
}

function Gallery() {
  const { themes, roots, themesLoaded, configError, resetConfig, busy, duplicateTheme } = useStudio();
  const { go } = useNav();
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");

  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const matches = (t: ThemeEntry) =>
      (filter === "all" || (filter === "user" ? t.origin !== "bundled" : t.origin === "bundled")) &&
      (needle === "" || `${t.name} ${t.kind ? KINDS[t.kind].label : ""}`.toLowerCase().includes(needle));
    // Broken themes sink to the end so the usable ones lead.
    return themes.filter(matches).toSorted((a, b) => Number(a.error !== null) - Number(b.error !== null));
  }, [themes, filter, query]);

  async function edit(theme: ThemeEntry) {
    if (theme.origin === "user") return go({ page: "themes", view: "edit", theme: theme.id });
    const copy = await duplicateTheme(theme);
    if (copy) go({ page: "themes", view: "edit", theme: copy.id });
  }

  const newTheme = (
    <Button variant="primary" icon={Plus} onClick={() => go({ page: "themes", view: "new" })}>
      New theme
    </Button>
  );

  return (
    <>
      <PageBar title="Themes" detail={themesLoaded ? `${themes.length}` : undefined}>
        <div className="relative max-md:hidden">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-ink-3" aria-hidden />
          <TextInput compact type="search" aria-label="Search themes" placeholder="Search themes" value={query} onChange={(e) => setQuery(e.currentTarget.value)} className="w-52 pl-8" />
        </div>
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
        {newTheme}
      </PageBar>
      <div className="min-h-0 flex-1 overflow-y-auto px-(--gutter) pt-6 pb-12 [scrollbar-gutter:stable]">
        <div>
          {configError && (
            <Banner>
              <span>Your settings file can't be read: {configError}</span>
              <Button busy={busy.has("config")} onClick={() => void resetConfig()}>
                Reset to defaults
              </Button>
            </Banner>
          )}
          {!themesLoaded ? (
            <div aria-hidden className={GRID}>
              {Array.from({ length: 8 }, (_, i) => (
                <div key={i}>
                  <Skeleton className="aspect-[16/10] rounded-xl" />
                  <Skeleton className="mt-3 h-3.5 w-1/2 rounded" />
                </div>
              ))}
            </div>
          ) : themes.length === 0 ? (
            <EmptyState icon={FolderPlus} title="No themes yet" action={newTheme}>
              Studio looks in {roots.length > 0 ? roots.join(" and ") : "your themes folder"}.
            </EmptyState>
          ) : shown.length === 0 ? (
            <EmptyState icon={SearchX} title="Nothing matches" action={newTheme}>
              {query.trim() ? `No theme called “${query.trim()}”.` : filter === "user" ? "You haven't made a theme yet. Make one from a picture, a video or a page." : "No built-in themes were found."}
            </EmptyState>
          ) : (
            <div className={GRID}>
              {shown.map((theme, index) => (
                <ThemeCard key={theme.id} theme={theme} index={index} onEdit={(t) => void edit(t)} />
              ))}
            </div>
          )}
        </div>
      </div>
    </>
  );
}
