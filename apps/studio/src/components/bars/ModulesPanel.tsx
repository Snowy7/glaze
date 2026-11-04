import { Plus } from "lucide-react";
import { useState } from "react";

import { setIn } from "../../lib/barsDoc";
import { useBars } from "../../lib/barsContext";
import { CUSTOM_ID, isCustom, type ModuleRef } from "../../lib/barsSchema";
import { Block, Button, Group, INPUT_BASE, Note, Segmented } from "../ui";
import { ModuleLayout, type ZoneInfo, type Zones } from "./ModuleLayout";
import { ModuleOptionsEditor } from "./ModuleOptions";

type Bar = "top" | "dock";

const TOP_ZONES: readonly ZoneInfo[] = [
  { zone: "left", label: "Left", hint: "Starts at the bar's start" },
  { zone: "center", label: "Center", hint: "Middle of the bar" },
  { zone: "right", label: "Right", hint: "Ends at the bar's end" },
];

const DOCK_ZONES: readonly ZoneInfo[] = [
  { zone: "left", label: "Before the task buttons", hint: "The taskbar style's modules before the apps" },
  { zone: "right", label: "After the task buttons", hint: "The taskbar style's modules after the apps (tray, clock)" },
];

/** What each bar shows, where, and how each module behaves (docs/bars.md, "Modules"). */
export function ModulesPanel() {
  const { resolved, update } = useBars();
  const [bar, setBar] = useState<Bar>("top");
  const [selected, setSelected] = useState<ModuleRef | null>(null);
  const info = bar === "top" ? TOP_ZONES : DOCK_ZONES;
  const zones: Zones = bar === "top" ? { left: resolved.top.left, center: resolved.top.center, right: resolved.top.right } : { left: resolved.dock.left, right: resolved.dock.right };
  const customs = Object.keys(resolved.modules).filter(isCustom);

  function change(next: Zones) {
    // Only the lists that moved are written; the rest keep following the preset.
    update((doc) => info.reduce((acc, { zone }) => (JSON.stringify(next[zone]) === JSON.stringify(zones[zone]) ? acc : setIn(acc, [bar, zone], next[zone])), doc));
  }

  return (
    <div className="flex flex-col gap-6">
      <Group>
        <Block title="Layout" detail="Drag modules to place them, drag a break between groups, click one to set its options.">
          <Segmented<Bar>
            className="mb-4 w-72"
            label="Bar"
            hideLabel
            value={bar}
            onChange={(next) => {
              setBar(next);
              setSelected(null);
            }}
            options={[{ value: "top", label: "Top bar" }, { value: "dock", label: "Dock taskbar" }]}
          />
          {bar === "dock" && resolved.dock.style !== "taskbar" && (
            <p className="mb-3 rounded-lg bg-canvas px-3 py-2 text-[13px] text-ink ring-1 ring-line-hi">The dock is a floating pill, which has no modules. These show once its style is Taskbar (Dock tab).</p>
          )}
          {bar === "top" && resolved.top.layout === "islands" && <Note>In the islands layout, each group between breaks is its own pill. Elsewhere a break is a thin separator.</Note>}
          <div className="mt-3">
            <ModuleLayout zones={zones} info={info} customs={customs} selected={selected} onSelect={setSelected} onChange={change} />
          </div>
        </Block>
      </Group>

      <AddCustom bar={bar} onAdded={(ref) => setSelected(ref)} />

      {selected && <ModuleOptionsEditor key={selected} moduleRef={selected} onRemoved={() => setSelected(null)} />}
      {!selected && <p className="text-[13px] text-ink-3">Select a module above to set what its clicks do, restyle it, change its icon, or edit a custom module's command.</p>}
    </div>
  );
}

/** Creates a `custom:<id>` module: it needs its command from the start, since the host refuses one without. */
function AddCustom({ bar, onAdded }: { bar: Bar; onAdded: (ref: ModuleRef) => void }) {
  const { resolved, update } = useBars();
  const [id, setId] = useState("");
  const [command, setCommand] = useState("");
  const ref = `custom:${id}`;
  const taken = ref in resolved.modules;
  const valid = CUSTOM_ID.test(id) && command.trim() !== "" && !taken;

  function add() {
    update((doc) => {
      const list = bar === "top" ? resolved.top.right : resolved.dock.right;
      const withModule = setIn(doc, ["modules", ref], { command: command.trim() });
      return setIn(withModule, [bar, "right"], [...list, ref]);
    });
    onAdded(ref);
    setId("");
    setCommand("");
  }

  return (
    <Group title="Custom modules">
      <Block title="Add a module that shows a command's output" detail="Its first line of output becomes the module's text, or a JSON object { text, icon, tooltip, class }. The id is 1 to 32 of a-z, 0-9, - and _.">
        <div className="flex flex-wrap gap-2">
          <input aria-label="Custom module id" value={id} placeholder="weather" spellCheck={false} maxLength={32} onChange={(e) => setId(e.currentTarget.value.toLowerCase())} className={`${INPUT_BASE} w-40 font-mono text-[13px]`} />
          <input aria-label="Custom module command" value={command} placeholder="curl.exe, powershell.exe, …" spellCheck={false} onChange={(e) => setCommand(e.currentTarget.value)} className={`${INPUT_BASE} min-w-56 flex-1 font-mono text-[13px]`} />
          <Button icon={Plus} variant="primary" disabled={!valid} onClick={add}>
            Add to the {bar === "top" ? "top bar" : "dock"}
          </Button>
        </div>
        {id !== "" && !CUSTOM_ID.test(id) && <Note>Ids use lowercase letters, digits, - and _ only.</Note>}
        {taken && <Note>There is already a module called custom:{id}.</Note>}
        <Note>Set its arguments and how often it runs after adding it, below.</Note>
      </Block>
    </Group>
  );
}
