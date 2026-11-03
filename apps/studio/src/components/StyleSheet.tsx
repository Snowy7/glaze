import { Copy, Lock, X } from "lucide-react";
import { useEffect, useId, useRef } from "react";

import type { ThemeEntry } from "../lib/api";
import { useStudio } from "../lib/studio";
import { StyleEditor } from "./StyleEditor";
import { Button, IconButton } from "./ui";

type Props = {
  theme: ThemeEntry;
  onClose: () => void;
  /** The user chose to customize a read-only theme: switch the sheet to the copy. */
  onDuplicated: (copy: ThemeEntry) => void;
};

/** A right-hand sheet (a native modal dialog: focus is trapped and Esc closes it). */
export function StyleSheet({ theme, onClose, onDuplicated }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();

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
      className="m-0 ml-auto h-dvh max-h-none w-[min(30rem,100vw)] max-w-none animate-slide-in overflow-hidden border-l border-line-strong bg-sidebar p-0 text-ink-2 shadow-card open:flex open:flex-col"
    >
      <header className="flex items-start justify-between gap-4 border-b border-line px-6 py-5">
        <div className="min-w-0">
          <h2 id={titleId} className="font-display text-xl font-semibold tracking-tight text-ink">
            Windows styling
          </h2>
          <p className="mt-0.5 truncate text-[13px] text-ink-3">{theme.name}</p>
        </div>
        <IconButton icon={X} label="Close" onClick={() => ref.current?.close()} />
      </header>

      {theme.origin === "user" ? <StyleEditor key={theme.id} theme={theme} /> : <ReadOnly theme={theme} onDuplicated={onDuplicated} />}
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
        Copy it to your themes and you can change its accent, borders, title bars, corners and backdrop. Its images and other files come along, and the original stays as it is.
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
