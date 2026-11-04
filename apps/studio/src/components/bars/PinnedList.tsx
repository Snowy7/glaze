import { AppWindow, Plus, X } from "lucide-react";

import { pickDockApp } from "../../lib/api";
import type { Pinned as PinnedItem } from "../../lib/barsSchema";
import { useDockIcon } from "../../lib/hooks";
import { useStudio } from "../../lib/studio";
import { Button, IconButton, INPUT_BASE, moved, SortableList } from "../ui";

/** `C:\Apps\Notepad++.exe` -> `Notepad++`. */
function stem(path: string): string {
  const file = path.split(/[\\/]/).pop() ?? path;
  const dot = file.lastIndexOf(".");
  return dot > 0 ? file.slice(0, dot) : file;
}

/** Pinned apps (the bottom bar's, or the Start menu's): add a program or shortcut, rename, reorder, unpin. */
export function PinnedList({ items, onChange }: { items: PinnedItem[]; onChange: (items: PinnedItem[]) => void }) {
  const { notify } = useStudio();

  async function add() {
    const path = await pickDockApp();
    if (!path) return;
    if (items.some((item) => item.path.toLowerCase() === path.toLowerCase())) {
      notify(`${stem(path)} is already pinned`);
      return;
    }
    onChange([...items, { path, name: stem(path) }]);
  }

  return (
    <div className="flex flex-col gap-2">
      {items.length === 0 ? (
        <p className="rounded-lg px-3 py-2.5 text-[12px] text-ink-3 ring-1 ring-line-strong ring-inset">Nothing pinned yet.</p>
      ) : (
        <SortableList
          label="Pinned apps"
          items={items}
          keyOf={(item) => item.path}
          onReorder={(from, to) => onChange(moved(items, from, to))}
          renderItem={(item, index) => {
            const name = item.name ?? stem(item.path);
            return (
              <>
                <AppIcon path={item.path} />
                <input
                  aria-label={`Name of ${name}`}
                  title={item.path}
                  value={item.name ?? ""}
                  placeholder={stem(item.path)}
                  onChange={(e) => onChange(items.map((it, i) => (i === index ? withName(it, e.currentTarget.value) : it)))}
                  className={`${INPUT_BASE} ui-input-sm min-w-0 flex-1 border-transparent bg-transparent px-1.5 shadow-none hover:bg-surface focus:bg-surface`}
                />
                <IconButton icon={X} variant="ghost" size="sm" label={`Unpin ${name}`} onClick={() => onChange(items.filter((_, i) => i !== index))} />
              </>
            );
          }}
        />
      )}
      <Button size="sm" icon={Plus} className="self-start" onClick={() => void add()}>
        Add an app
      </Button>
    </div>
  );
}

/** The item with a new name; an empty name is removed so the bar falls back to the file's own. */
function withName(item: PinnedItem, name: string): PinnedItem {
  const { name: _old, ...rest } = item;
  return name === "" ? rest : { ...rest, name };
}

function AppIcon({ path }: { path: string }) {
  const url = useDockIcon(path);
  return (
    <span className="grid size-6 shrink-0 place-items-center">
      {url ? <img src={url} alt="" draggable={false} className="size-5 object-contain" /> : <AppWindow className={`size-4 text-ink-3 ${url === undefined ? "animate-pulse" : ""}`} aria-hidden />}
    </span>
  );
}
