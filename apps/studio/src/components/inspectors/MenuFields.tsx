import { Plus, RotateCcw, X } from "lucide-react";
import { useState } from "react";

import { MENU_BUILTINS, type Config, type MenuBuiltin, type MenuEntry } from "../../lib/api";
import { useStudio } from "../../lib/studio";
import { MENU_LABELS } from "../stage/WindowsLayer";
import { Button, IconButton, InspectorSection, More, moved, Prop, Select, SortableList, Switch, TextInput } from "../ui";

const labelOf = (entry: MenuEntry) => (typeof entry === "string" ? MENU_LABELS[entry] : entry.label);

/**
 * The desktop's right-click menu (docs/desktop-extras.md): on or off, its items in order (drag to
 * reorder, remove to hide), built-ins to add back, your own commands, and Explorer's full menus.
 */
export function MenuFields({ config }: { config: Config }) {
  const { updateConfig } = useStudio();
  const { enabled, items } = config.desktop_menu;
  const setItems = (next: MenuEntry[], label: string) => void updateConfig({ desktop_menu: { items: next } }, { label });
  const hidden = MENU_BUILTINS.filter((b) => b !== "separator" && !items.includes(b));
  const isDefault = JSON.stringify(items) === JSON.stringify(config.desktop_menu_default);

  return (
    <>
      <InspectorSection>
        <Prop id="menu.enabled" label="Use mypaper's menu" hint={enabled ? "On the bare desktop. Icons, windows and Shift+right-click keep Windows' menu." : "Windows' own menu shows when you right-click the desktop."}>
          <Switch label="Use mypaper's menu" checked={enabled} onChange={(on) => void updateConfig({ desktop_menu: { enabled: on } }, { label: on ? "Use mypaper's menu" : "Use Windows' menu" })} />
        </Prop>
      </InspectorSection>
      {enabled && (
        <InspectorSection
          title="Items"
          action={
            <Button size="sm" variant="ghost" icon={RotateCcw} disabled={isDefault} onClick={() => setItems(config.desktop_menu_default, "Reset the menu")}>
              Reset
            </Button>
          }
        >
          <div data-setting="menu.items" className="setting-anchor">
            <SortableList
              label="Menu items"
              items={items}
              keyOf={(entry, i) => `${labelOf(entry)}:${i}`}
              onReorder={(from, to) => setItems(moved(items, from, to), "Reorder the menu")}
              renderItem={(entry, index) => (
                <>
                  {entry === "separator" ? (
                    <span className="flex min-w-0 flex-1 items-center px-1" aria-label="Separator">
                      <span className="h-px flex-1 bg-line-hi/70" />
                    </span>
                  ) : (
                    <span className="min-w-0 flex-1 truncate px-1 text-[13px] text-ink">
                      {labelOf(entry)}
                      {typeof entry === "object" && <span className="ml-2 font-mono text-[12px] text-ink-3">{entry.command}</span>}
                    </span>
                  )}
                  <IconButton icon={X} variant="ghost" size="sm" label={`Remove ${labelOf(entry)}`} className="opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100" onClick={() => setItems(items.toSpliced(index, 1), `Remove ${labelOf(entry)}`)} />
                </>
              )}
            />
          </div>
          <div className="flex items-center gap-2">
            <Select<MenuBuiltin | "">
              label="Add an item"
              size="sm"
              className="min-w-0 flex-1"
              value=""
              options={[{ value: "", label: hidden.length > 0 ? "Add an item…" : "Every item is in" }, ...hidden.map((b) => ({ value: b, label: MENU_LABELS[b] }))]}
              onChange={(next) => next !== "" && setItems([...items, next], `Add ${MENU_LABELS[next]}`)}
            />
            <Button size="sm" icon={Plus} onClick={() => setItems([...items, "separator"], "Add a separator")}>
              Separator
            </Button>
          </div>
        </InspectorSection>
      )}
      <More>
        {enabled && <CustomItem onAdd={(entry) => setItems([...items, entry], `Add ${entry.label}`)} />}
        <Prop id="menu.classic" label="Full menus in Explorer" hint="Windows 11's right-click menus show everything, without “Show more options”. Takes effect after you sign out, or restart Explorer.">
          <Switch label="Full menus in Explorer" checked={config.classic_context_menu} onChange={(on) => void updateConfig({ classic_context_menu: on }, { label: on ? "Use full menus in Explorer" : "Use Windows 11's menus" })} />
        </Prop>
      </More>
    </>
  );
}

/** A menu item that runs a program of your own. */
function CustomItem({ onAdd }: { onAdd: (entry: { label: string; command: string; args: string[] }) => void }) {
  const [label, setLabel] = useState("");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  return (
    <div data-setting="menu.custom" className="setting-anchor">
      <div className="mb-1.5 text-[13px] text-ink-2">Your own item</div>
      <div className="flex flex-col gap-2">
        <TextInput compact aria-label="Item name" value={label} placeholder="Name, e.g. Notes" onChange={(e) => setLabel(e.currentTarget.value)} className="w-full" />
        <TextInput compact mono aria-label="Program" value={command} placeholder="notepad.exe" onChange={(e) => setCommand(e.currentTarget.value)} className="w-full" />
        <div className="flex gap-2">
          <TextInput compact mono aria-label="Arguments" value={args} placeholder="Arguments (optional)" onChange={(e) => setArgs(e.currentTarget.value)} className="min-w-0 flex-1" />
          <Button
            size="sm"
            className="h-8"
            icon={Plus}
            disabled={label.trim() === "" || command.trim() === ""}
            onClick={() => {
              onAdd({ label: label.trim(), command: command.trim(), args: args.split(" ").filter((a) => a !== "") });
              setLabel("");
              setCommand("");
              setArgs("");
            }}
          >
            Add
          </Button>
        </div>
      </div>
    </div>
  );
}
