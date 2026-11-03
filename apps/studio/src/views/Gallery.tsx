import { Pause, Play } from "lucide-react";

import { Banner, Button, PageHeader } from "../components/ui";
import { ThemeCard } from "../components/ThemeCard";
import type { View } from "../components/Sidebar";
import { useStudio } from "../lib/studio";

export function Gallery({ onNavigate }: { onNavigate: (view: View) => void }) {
  const { themes, roots, themesLoaded, config, configError, updateConfig, resetConfig, busy } = useStudio();
  const active = themes.find((t) => t.active);

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
        <Banner tone="danger">
          <span>config.json can't be read: {configError}</span>
          <Button variant="danger" busy={busy.has("config")} onClick={() => void resetConfig()}>
            Reset to defaults
          </Button>
        </Banner>
      )}

      {themesLoaded && themes.length === 0 ? (
        <div className="grid place-items-center rounded-2xl border border-dashed border-line-strong px-8 py-20 text-center">
          <p className="font-display text-lg font-semibold">No themes yet</p>
          <p className="selectable mt-1 max-w-md text-[13px] text-ink-2">
            Studio looks in {roots.length > 0 ? roots.join(" and ") : "your themes folder and the themes folder beside the app"}.
          </p>
          <Button variant="primary" className="mt-5" onClick={() => onNavigate("new")}>
            Create a theme
          </Button>
        </div>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(250px,1fr))] gap-5 pb-4">
          {themes.map((theme, index) => (
            <ThemeCard key={theme.id} theme={theme} index={index} />
          ))}
        </div>
      )}
    </>
  );
}
