import { Check, RotateCcw } from "lucide-react";

import { Button } from "./ui";

type Props = {
  dirty: boolean;
  saving: boolean;
  /** Shows "Saved" until the next edit. */
  justSaved: boolean;
  error: string | null;
  /** Everything is already unset, so there is nothing to clear. */
  cleared: boolean;
  onClear: () => void;
  onRevert: () => void;
  onSave: () => void;
};

/** The bottom bar of an explicit-save editor sheet: clear, revert, save. */
export function SaveFooter({ dirty, saving, justSaved, error, cleared, onClear, onRevert, onSave }: Props) {
  return (
    <footer className="border-t border-line bg-sidebar px-6 py-4">
      {error && (
        <p role="alert" className="selectable mb-3 text-[13px] text-ink">
          {error}
        </p>
      )}
      <div className="flex items-center gap-2">
        <Button variant="ghost" icon={RotateCcw} disabled={cleared} onClick={onClear}>
          Clear all
        </Button>
        <span className="ml-auto flex items-center gap-1.5 text-xs text-ink-3" aria-live="polite">
          {justSaved && !dirty && (
            <>
              <Check className="size-3.5" aria-hidden /> Saved
            </>
          )}
        </span>
        <Button disabled={!dirty} onClick={onRevert}>
          Revert
        </Button>
        <Button variant="primary" busy={saving} disabled={!dirty} onClick={onSave}>
          Save
        </Button>
      </div>
    </footer>
  );
}
