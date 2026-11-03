import { AppWindow, GripVertical, Plus, X } from "lucide-react";
import { useState, type KeyboardEvent } from "react";

import { pickDockApp, type PinnedItem } from "../../lib/api";
import { useDockIcon } from "../../lib/hooks";
import { useStudio } from "../../lib/studio";
import { Button, IconButton, INPUT_CLASS } from "../ui";

/** `C:\Apps\Notepad++.exe` -> `Notepad++`. */
function stem(path: string): string {
  const file = path.split(/[\\/]/).pop() ?? path;
  const dot = file.lastIndexOf(".");
  return dot > 0 ? file.slice(0, dot) : file;
}

/** `list` with the item at `from` moved so it ends up at `to`. */
export function reorder<T>(list: readonly T[], from: number, to: number): T[] {
  const item = list[from];
  if (item === undefined || from === to) return [...list];
  return list.toSpliced(from, 1).toSpliced(to, 0, item);
}

/**
 * The dock's pinned items: add with a file picker (.exe or .lnk), rename, drag
 * to reorder (or focus the grip and use the up and down arrows) and remove.
 */
export function PinnedList({ items, onChange }: { items: PinnedItem[]; onChange: (items: PinnedItem[]) => void }) {
  const { notify } = useStudio();
  // Where the dragged row would land, as an index into `items`.
  const [dragFrom, setDragFrom] = useState<number | null>(null);
  const [dropAt, setDropAt] = useState<number | null>(null);

  async function add() {
    const path = await pickDockApp();
    if (!path) return;
    if (items.some((item) => item.path.toLowerCase() === path.toLowerCase())) {
      notify(`${stem(path)} is already pinned`);
      return;
    }
    onChange([...items, { path, name: stem(path) }]);
  }

  function rename(index: number, name: string) {
    onChange(items.map((item, i) => (i === index ? withName(item, name) : item)));
  }

  function endDrag() {
    setDragFrom(null);
    setDropAt(null);
  }

  function drop() {
    if (dragFrom !== null && dropAt !== null) {
      // `dropAt` counts the row being dragged, which leaves its old place first.
      const to = dropAt > dragFrom ? dropAt - 1 : dropAt;
      onChange(reorder(items, dragFrom, to));
    }
    endDrag();
  }

  function gripKey(event: KeyboardEvent, index: number) {
    const to = event.key === "ArrowUp" ? index - 1 : event.key === "ArrowDown" ? index + 1 : null;
    if (to === null || to < 0 || to >= items.length) return;
    event.preventDefault();
    onChange(reorder(items, index, to));
    // The grip is re-created in its new row; focus follows it.
    requestAnimationFrame(() => document.querySelectorAll<HTMLElement>("[data-grip]")[to]?.focus());
  }

  return (
    <div>
      {items.length === 0 ? (
        <p className="rounded-xl border border-dashed border-line-strong px-4 py-6 text-center text-[13px] text-ink-3">
          Nothing pinned yet. Add a program or a shortcut and it stays in the dock whether or not it's running.
        </p>
      ) : (
        <ul
          aria-label="Pinned items"
          className="divide-y divide-line overflow-hidden rounded-xl bg-canvas ring-1 ring-line"
          onDragOver={(e) => {
            if (dragFrom === null) return;
            e.preventDefault();
            setDropAt(items.length);
          }}
          onDrop={(e) => {
            e.preventDefault();
            drop();
          }}
        >
          {items.map((item, index) => (
            <li
              key={item.path}
              onDragOver={(e) => {
                if (dragFrom === null) return;
                e.preventDefault();
                e.stopPropagation();
                const box = e.currentTarget.getBoundingClientRect();
                setDropAt(index + (e.clientY > box.top + box.height / 2 ? 1 : 0));
              }}
              className={`relative flex items-center gap-3 px-3 py-2 ${dragFrom === index ? "opacity-40" : ""}`}
            >
              {dragFrom !== null && dropAt === index && <DropLine at="top" />}
              {dragFrom !== null && dropAt === items.length && index === items.length - 1 && <DropLine at="bottom" />}
              <button
                type="button"
                draggable
                data-grip
                aria-label={`Reorder ${item.name ?? stem(item.path)}. Use the up and down arrow keys.`}
                onKeyDown={(e) => gripKey(e, index)}
                onDragStart={(e) => {
                  const row = e.currentTarget.closest("li");
                  if (row) e.dataTransfer.setDragImage(row, 16, 20);
                  e.dataTransfer.effectAllowed = "move";
                  e.dataTransfer.setData("text/plain", item.path);
                  setDragFrom(index);
                }}
                onDragEnd={endDrag}
                className="grid h-8 w-6 shrink-0 cursor-grab place-items-center rounded-md text-ink-3 hover:text-ink active:cursor-grabbing"
              >
                <GripVertical className="size-4" aria-hidden />
              </button>
              <AppIcon path={item.path} />
              <div className="min-w-0 flex-1">
                <input
                  aria-label={`Name of ${item.name ?? stem(item.path)}`}
                  value={item.name ?? ""}
                  placeholder={stem(item.path)}
                  onChange={(e) => rename(index, e.currentTarget.value)}
                  className={`${INPUT_CLASS} h-8 px-2 text-[13.5px] font-medium`}
                />
                <div className="selectable mt-0.5 truncate px-2 font-mono text-[11.5px] text-ink-3" title={item.path}>
                  {item.path}
                </div>
              </div>
              <IconButton icon={X} label={`Unpin ${item.name ?? stem(item.path)}`} onClick={() => onChange(items.filter((_, i) => i !== index))} />
            </li>
          ))}
        </ul>
      )}
      <Button icon={Plus} className="mt-3" onClick={() => void add()}>
        Add program or shortcut
      </Button>
    </div>
  );
}

/** The item with a new name; an empty name is removed so the dock falls back to the file's own. */
function withName(item: PinnedItem, name: string): PinnedItem {
  const { name: _old, ...rest } = item;
  return name === "" ? rest : { ...rest, name };
}

function DropLine({ at }: { at: "top" | "bottom" }) {
  return <span aria-hidden className={`absolute inset-x-0 h-0.5 bg-ink ${at === "top" ? "top-0" : "bottom-0"}`} />;
}

function AppIcon({ path }: { path: string }) {
  const url = useDockIcon(path);
  return (
    <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-surface-hi">
      {url ? <img src={url} alt="" draggable={false} className="size-7 object-contain" /> : <AppWindow className={`size-4 text-ink-3 ${url === undefined ? "animate-pulse" : ""}`} aria-hidden />}
    </span>
  );
}
