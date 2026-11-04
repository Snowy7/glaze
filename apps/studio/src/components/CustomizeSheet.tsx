import { Copy, Lock, X } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";

import type { ThemeEntry } from "../lib/api";
import { useStudio } from "../lib/studio";
import { SkinEditor } from "./SkinEditor";
import { StyleEditor } from "./StyleEditor";
import { WallpaperEditor } from "./WallpaperEditor";
import { ThemePreview } from "./ThemeCard";
import { Button, IconButton, Tabs } from "./ui";

type Props = {
  theme: ThemeEntry;
  onClose: () => void;
  /** The user chose to customize a read-only theme: switch the sheet to the copy. */
  onDuplicated: (copy: ThemeEntry) => void;
};

type Tab = "windows" | "widgets" | "bars" | "wallpaper";

const TABS: readonly { value: Tab; label: string }[] = [
  { value: "windows", label: "Windows" },
  { value: "widgets", label: "Widgets look" },
  { value: "bars", label: "Bars look" },
];

/** Wallpaper kinds with options of their own here (they can take the mouse). */
const TAKES_MOUSE = new Set(["web", "scene"]);

/** A right-hand sheet (a native modal dialog: focus is trapped and Esc closes it). */
export function CustomizeSheet({ theme, onClose, onDuplicated }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const [tab, setTab] = useState<Tab>("windows");
  const tabs: readonly { value: Tab; label: string }[] = theme.kind && TAKES_MOUSE.has(theme.kind) ? [...TABS, { value: "wallpaper", label: "Wallpaper" }] : TABS;

  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);

  return (
    <dialog
      ref={ref}
      // The sheet itself takes focus on open (not the first button); Tab moves into it.
      autoFocus
      aria-labelledby={titleId}
      onClose={onClose}
      onClick={(event) => event.target === event.currentTarget && ref.current?.close()}
      className="m-0 ml-auto h-dvh outline-none max-h-none w-[min(38rem,100vw)] max-w-none animate-slide-in overflow-hidden border-l border-line-strong bg-sidebar p-0 text-ink-2 shadow-pop open:flex open:flex-col 2xl:w-[42rem]"
    >
      <header className="border-b border-line-strong px-6 pt-5">
        <div className="flex items-center gap-3.5">
          <div className="aspect-[16/10] w-16 shrink-0 overflow-hidden rounded-md ring-1 ring-line-strong">
            <ThemePreview name={theme.name} kind={theme.kind} preview={theme.preview} />
          </div>
          <div className="min-w-0 flex-1">
            <p className="text-[12px] font-medium text-ink-3">Customize</p>
            <h2 id={titleId} className="truncate font-display text-lg leading-tight font-semibold tracking-tight text-ink">
              {theme.name}
            </h2>
          </div>
          <IconButton icon={X} variant="ghost" label="Close" onClick={() => ref.current?.close()} />
        </div>
        <Tabs<Tab> label="Section" className="mt-3 -mx-2.5" value={tab} options={tabs} onChange={setTab} />
      </header>

      {theme.origin !== "user" ? (
        <ReadOnly theme={theme} onDuplicated={onDuplicated} />
      ) : tab === "windows" ? (
        <StyleEditor key={theme.id} theme={theme} />
      ) : tab === "wallpaper" ? (
        <WallpaperEditor key={theme.id} theme={theme} />
      ) : (
        <SkinEditor key={`${theme.id}:${tab}`} theme={theme} kind={tab} />
      )}
    </dialog>
  );
}

function ReadOnly({ theme, onDuplicated }: Pick<Props, "theme" | "onDuplicated">) {
  const { duplicateTheme, busy } = useStudio();
  const copying = busy.has(`duplicate:${theme.id}`);
  return (
    <div className="flex flex-1 flex-col items-start px-6 py-8">
      <span className="mb-4 grid size-11 place-items-center rounded-xl bg-surface-hi text-ink-2 ring-1 ring-line-strong">
        <Lock className="size-5" aria-hidden />
      </span>
      <p className="font-display text-base font-semibold text-ink">{theme.origin === "bundled" ? "Built-in themes are read-only" : "This theme lives outside your themes folder"}</p>
      <p className="mt-1 max-w-sm text-[13px] text-ink-3">
        Copy it to your themes and you can change its Windows styling and how widgets and the bars look. Its images and other files come along, and the original stays as it is.
      </p>
      <Button
        variant="primary"
        icon={Copy}
        className="mt-5"
        busy={copying}
        onClick={() => void duplicateTheme(theme).then((copy) => copy && onDuplicated(copy))}
      >
        Duplicate to my themes
      </Button>
    </div>
  );
}
