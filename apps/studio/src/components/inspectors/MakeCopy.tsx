import { Copy } from "lucide-react";

import type { ThemeEntry } from "../../lib/api";
import { useStudio } from "../../lib/studio";
import { Button } from "../ui";

/**
 * A built-in theme can't be changed in place: one click copies it into your themes (and, when it's
 * the one on screen, puts the copy on), so editing carries on right here.
 */
export function MakeCopy({ theme, apply, onCopied }: { theme: ThemeEntry; apply: boolean; onCopied?: (copy: ThemeEntry) => void }) {
  const { duplicateTheme, applyTheme, busy } = useStudio();
  return (
    <div className="mx-4 mt-4 rounded-lg bg-surface p-3.5 ring-1 ring-line-strong">
      <p className="text-[13px] font-medium text-ink">{theme.origin === "bundled" ? `${theme.name} is built in` : `${theme.name} is outside your themes`}</p>
      <p className="mt-0.5 text-[12px] leading-snug text-ink-3">Make your own copy to change it. The original stays as it is.</p>
      <Button
        size="sm"
        variant="primary"
        icon={Copy}
        className="mt-3"
        busy={busy.has(`duplicate:${theme.id}`)}
        onClick={() =>
          void duplicateTheme(theme).then(async (copy) => {
            if (!copy) return;
            if (apply) await applyTheme(copy);
            onCopied?.(copy);
          })
        }
      >
        {apply ? "Make a copy and use it" : "Make a copy"}
      </Button>
    </div>
  );
}
