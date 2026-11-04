import { ArrowDown, ArrowUp, Plus, RotateCcw, X } from "lucide-react";
import { useState } from "react";

import { MENU_BUILTINS, type MenuBuiltin, type MenuEntry } from "../lib/api";
import { Button, IconButton, INPUT_CLASS, Select } from "./ui";

const LABELS: Record<MenuBuiltin, string> = {
  separator: "Separator",
  view: "View: icon size, show desktop icons",
  sort: "Sort by: name, size, type, date",
  refresh: "Refresh",
  new: "New: folder, text document",
  paste: "Paste",
  wallpaper: "Wallpaper: opens Studio",
  widgets: "Widgets: toggles edit mode",
  display: "Display settings",
  personalize: "Personalization settings",
  terminal: "Terminal here",
  more: "Show more options: the native menu",
};

const labelOf = (entry: MenuEntry): string => (typeof entry === "string" ? LABELS[entry] : entry.label);

/** `list` with the item at `from` moved by `by` places. */
function moved<T>(list: readonly T[], from: number, by: number): T[] {
  const item = list[from];
  const to = from + by;
  if (item === undefined || to < 0 || to >= list.length) return [...list];
  return list.toSpliced(from, 1).toSpliced(to, 0, item);
}

type Props = { items: MenuEntry[]; defaults: MenuEntry[]; onChange: (items: MenuEntry[]) => void };

/**
 * The menu top to bottom. An item left out is hidden, so removing one hides it and it can be added
 * back; stray separators are tidied by mypaper, so hiding items never leaves a gap.
 */
export function DesktopMenuEditor({ items, defaults, onChange }: Props) {
  const [addition, setAddition] = useState<MenuBuiltin | "">("");
  const [label, setLabel] = useState("");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const hidden = MENU_BUILTINS.filter((b) => b !== "separator" && !items.includes(b));
  const isDefault = JSON.stringify(items) === JSON.stringify(defaults);

  return (
    <div className="space-y-3">
      <ol aria-label="Menu items" className="divide-y divide-line overflow-hidden rounded-xl bg-canvas ring-1 ring-line">
        {items.map((entry, index) => (
          <li key={`${labelOf(entry)}:${index}`} className="flex items-center gap-2 px-3 py-1.5">
            <span className={`min-w-0 flex-1 truncate text-[13px] ${entry === "separator" ? "text-ink-3" : "text-ink"}`}>
              {entry === "separator" ? "────────" : labelOf(entry)}
              {typeof entry === "object" && <span className="selectable ml-2 font-mono text-[11.5px] text-ink-3">{[entry.command, ...entry.args].join(" ")}</span>}
            </span>
            <IconButton icon={ArrowUp} label={`Move ${labelOf(entry)} up`} disabled={index === 0} onClick={() => onChange(moved(items, index, -1))} className="size-8" />
            <IconButton icon={ArrowDown} label={`Move ${labelOf(entry)} down`} disabled={index === items.length - 1} onClick={() => onChange(moved(items, index, 1))} className="size-8" />
            <IconButton icon={X} label={`Hide ${labelOf(entry)}`} onClick={() => onChange(items.filter((_, i) => i !== index))} className="size-8" />
          </li>
        ))}
        {items.length === 0 && <li className="px-3 py-3 text-[13px] text-ink-3">An empty menu. Add something back below.</li>}
      </ol>

      <div className="flex flex-wrap items-center gap-2">
        <Select<MenuBuiltin | "">
          label="Add a built-in item"
          className="w-64"
          value={addition}
          options={[{ value: "", label: "Add a built-in item…" }, ...hidden.map((b) => ({ value: b, label: LABELS[b] }))]}
          onChange={(next) => {
            if (next !== "") onChange([...items, next]);
            setAddition("");
          }}
        />
        <Button icon={Plus} onClick={() => onChange([...items, "separator"])}>
          Separator
        </Button>
        <Button icon={RotateCcw} disabled={isDefault} onClick={() => onChange(defaults)}>
          Reset the menu
        </Button>
      </div>

      <div className="rounded-xl bg-canvas p-3 ring-1 ring-line">
        <div className="mb-2 text-[12.5px] font-medium text-ink-2">Your own command</div>
        <div className="flex flex-wrap gap-2">
          <input aria-label="Custom item label" value={label} placeholder="Label" onChange={(e) => setLabel(e.currentTarget.value)} className={`${INPUT_CLASS} h-9 w-36`} />
          <input aria-label="Custom item command" value={command} placeholder="notepad.exe" spellCheck={false} onChange={(e) => setCommand(e.currentTarget.value)} className={`${INPUT_CLASS} h-9 min-w-40 flex-1 font-mono text-[13px]`} />
          <input aria-label="Custom item arguments" value={args} placeholder="Arguments" spellCheck={false} onChange={(e) => setArgs(e.currentTarget.value)} className={`${INPUT_CLASS} h-9 min-w-40 flex-1 font-mono text-[13px]`} />
          <Button
            icon={Plus}
            disabled={label.trim() === "" || command.trim() === ""}
            onClick={() => {
              onChange([...items, { label: label.trim(), command: command.trim(), args: args.split(" ").filter((a) => a !== "") }]);
              setLabel("");
              setCommand("");
              setArgs("");
            }}
          >
            Add
          </Button>
        </div>
        <p className="mt-2 text-xs text-ink-3">Runs the command with those arguments directly: no shell, no quoting rules to learn.</p>
      </div>
    </div>
  );
}
