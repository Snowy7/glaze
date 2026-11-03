import { Copy, Lock, X } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";

import type { ThemeEntry } from "../lib/api";
import { useStudio } from "../lib/studio";
import { SkinEditor } from "./SkinEditor";
import { StyleEditor } from "./StyleEditor";
import { Button, IconButton, Segmented } from "./ui";

type Props = {
  theme: ThemeEntry;
  onClose: () => void;
  /** The user chose to customize a read-only theme: switch the sheet to the copy. */
  onDuplicated: (copy: ThemeEntry) => void;
};

type Tab = "windows" | "widgets" | "bars";

const TABS: readonly { value: Tab; label: string }[] = [
  { value: "windows", label: "Windows" },
  { value: "widgets", label: "Widgets look" },
  { value: "bars", label: "Bars look" },
];

/** A right-hand sheet (a native modal dialog: focus is trapped and Esc closes it). */
export function CustomizeSheet({ theme, onClose, onDuplicated }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const [tab, setTab] = useState<Tab>("windows");

  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);

  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      onClose={onClose}
      onClick={(event) => event.target === event.currentTarget && ref.current?.close()}
      className="m-0 ml-auto h-dvh max-h-none w-[min(36rem,100vw)] max-w-none animate-slide-in overflow-hidden border-l border-line-strong bg-sidebar p-0 text-ink-2 shadow-card open:flex open:flex-col"
    >
      <header className="border-b border-line px-6 pt-5 pb-4">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h2 id={titleId} className="font-display text-xl font-semibold tracking-tight text-ink">
              Customize
            </h2>
            <p className="mt-0.5 truncate text-[13px] text-ink-3">{theme.name}</p>
          </div>
          <IconButton icon={X} label="Close" onClick={() => ref.current?.close()} />
        </div>
        <Segmented<Tab> label="Section" hideLabel className="mt-4" value={tab} options={TABS} onChange={setTab} />
      </header>

      {theme.origin !== "user" ? (
        <ReadOnly theme={theme} onDuplicated={onDuplicated} />
      ) : tab === "windows" ? (
        <StyleEditor key={theme.id} theme={theme} />
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
    <div className="flex flex-1 flex-col items-start px-6 py-6">
      <span className="mb-4 grid size-11 place-items-center rounded-xl bg-surface-hi text-ink-2">
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
