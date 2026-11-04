import { ArrowRight, Check, LoaderCircle } from "lucide-react";

import { useNav } from "../lib/nav";
import { useStudio } from "../lib/studio";
import { ThemePreview } from "./ThemeCard";

/** A row of themes under the desktop: one click puts one on (with an Undo), the last opens them all. */
export function ThemeStrip() {
  const { themes, applyTheme, busy } = useStudio();
  const { go } = useNav();
  const usable = themes.filter((t) => t.error === null);
  if (usable.length === 0) return null;
  return (
    <div className="flex items-center gap-2.5">
      <div role="list" aria-label="Themes" className="flex min-w-0 flex-1 gap-2 overflow-x-auto px-0.5 py-1 [scrollbar-width:none]">
        {usable.map((theme) => {
          const applying = busy.has(`apply:${theme.id}`);
          return (
            <button
              key={theme.id}
              type="button"
              role="listitem"
              aria-current={theme.active ? "true" : undefined}
              title={theme.active ? `${theme.name} (on now)` : `Use ${theme.name}`}
              disabled={applying}
              onClick={() => !theme.active && void applyTheme(theme)}
              className={`group relative aspect-[16/10] w-[6.5rem] shrink-0 cursor-default overflow-hidden rounded-lg transition-[box-shadow,transform] duration-150 2xl:w-32 ${
                theme.active ? "ring-2 ring-ink ring-offset-2 ring-offset-canvas" : "ring-1 ring-line-strong hover:-translate-y-0.5 hover:ring-line-hi"
              }`}
            >
              <ThemePreview name={theme.name} kind={theme.kind} preview={theme.preview} zoom />
              <span className="absolute inset-x-0 bottom-0 flex items-center gap-1 bg-linear-to-t from-black/80 to-transparent px-2 pt-4 pb-1.5 text-left text-[11px] font-medium text-white">
                <span className="min-w-0 flex-1 truncate">{theme.name}</span>
                {theme.active && <Check className="size-3 shrink-0" aria-hidden />}
                {applying && <LoaderCircle className="size-3 shrink-0 animate-spin" aria-hidden />}
              </span>
            </button>
          );
        })}
      </div>
      <button
        type="button"
        onClick={() => go({ page: "themes" })}
        className="flex h-[4.1rem] shrink-0 cursor-default flex-col items-center justify-center gap-1 rounded-lg px-3 text-[12px] font-medium text-ink-3 ring-1 ring-line-strong transition-colors hover:bg-surface-hi hover:text-ink 2xl:h-20"
      >
        <ArrowRight className="size-4" aria-hidden />
        All themes
      </button>
    </div>
  );
}
