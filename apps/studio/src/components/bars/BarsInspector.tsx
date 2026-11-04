import { Check, Dock, PanelTop, Plus, RotateCcw, Trash2, X } from "lucide-react";
import { useState } from "react";

import { forBar, useBars, BarsContext } from "../../lib/barsContext";
import type { ZoneId } from "../../lib/barsGeometry";
import { addModule, listOf, setList, ZONE_NAMES } from "../../lib/barsModules";
import { DOCK_DESIGNS, PRESETS, TOP_DESIGNS } from "../../lib/barsNames";
import { BREAK, BUILTIN_MODULES, CUSTOM_ID, isCustom, PRESET_IDS, type BarKind, type BarsDoc, type ModuleRef, type PresetId } from "../../lib/barsSchema";
import { describeModule } from "../../lib/modules";
import type { BarPick } from "../stage/BarsLayer";
import { zonesOf } from "../stage/BarsLayer";
import { Button, IconButton, INPUT_BASE, InspectorHeader, InspectorSection, More, moved, Segmented, SortableList, Switch } from "../ui";
import { OverridesList, TaskbarSettings, TopBarCommon, TopBarMore } from "./BarPanels";
import { ToggleSetting } from "./BarSettings";
import { DesignPicker } from "./DesignPicker";
import { DockCommon, DockMore, DockPinned } from "./DockPanel";
import { IconsPanel } from "./IconsPanel";
import { LookCommon, LookPanel, SharedLook } from "./LookPanel";
import { MenusPanel } from "./MenusPanel";
import { ModuleFields, withoutModule } from "./ModuleOptions";
import { MotionAndPower, MotionPanel } from "./MotionPanel";

const BAR_NAMES: Record<BarKind, string> = { top: "Top bar", dock: "Bottom bar" };
const BAR_ICONS = { top: PanelTop, dock: Dock } as const;

const designName = (bar: BarKind, id: string) => (bar === "top" ? TOP_DESIGNS[id as keyof typeof TOP_DESIGNS] : DOCK_DESIGNS[id as keyof typeof DOCK_DESIGNS])?.name ?? id;

/** Nothing picked: the pairings, the two bars (on or off), and Windows' own taskbar. */
export function BarsOverview({ onPick }: { onPick: (pick: BarPick) => void }) {
  const { resolved, update } = useBars();

  /** A pairing sets both designs and which bars are on, so the file's own choices of those go. */
  function usePairing(id: PresetId) {
    update((doc) => {
      const { design: _top, enabled: _topOn, ...top } = doc.top ?? {};
      const { design: _dock, enabled: _dockOn, ...dock } = doc.dock ?? {};
      return { ...doc, preset: id, top, dock };
    }, `Use the ${PRESETS[id].name} pairing`);
  }

  return (
    <>
      <InspectorHeader icon={PanelTop} title="Bars" detail="Click a bar or a module to change it" />
      <InspectorSection title="Bars">
        {(["top", "dock"] as const).map((bar) => (
          <BarRow key={bar} bar={bar} onOpen={() => onPick({ bar })} />
        ))}
      </InspectorSection>
      <InspectorSection title="Pairings">
        <div role="radiogroup" aria-label="Pairing" data-setting="bars.preset" className="setting-anchor -mx-2 flex flex-col">
          {PRESET_IDS.map((id) => {
            const on = resolved.preset === id;
            return (
              <button key={id} type="button" role="radio" aria-checked={on} onClick={() => usePairing(id)} className="flex cursor-default items-center gap-3 rounded-lg px-2 py-2 text-left transition-colors hover:bg-surface-hi">
                <span className="min-w-0 flex-1">
                  <span className={`block text-[13px] ${on ? "font-medium text-ink" : "text-ink-2"}`}>{PRESETS[id].name}</span>
                  <span className="block truncate text-[12px] text-ink-3">{PRESETS[id].blurb}</span>
                </span>
                {on && <Check className="size-4 shrink-0 text-ink" aria-hidden />}
              </button>
            );
          })}
        </div>
      </InspectorSection>
      <InspectorSection title="Windows taskbar">
        <TaskbarSettings />
      </InspectorSection>
      <More label="Menus and panels">
        <MenusPanel />
      </More>
      <More label="Icon packs">
        <IconsPanel />
      </More>
      <More label="Look for both bars">
        <SharedLook />
      </More>
      <More label="Motion and power">
        <MotionAndPower />
      </More>
      <OverridesSection />
    </>
  );
}

function OverridesSection() {
  const { doc } = useBars();
  if (Object.keys(doc.overrides ?? {}).length === 0) return null;
  return (
    <More label="Per display">
      <OverridesList />
    </More>
  );
}

/** A bar in the overview: what it is, on or off, and the way into it. */
function BarRow({ bar, onOpen }: { bar: BarKind; onOpen: () => void }) {
  const { resolved, set } = useBars();
  const Icon = BAR_ICONS[bar];
  const on = resolved[bar].enabled;
  return (
    <div className="-mx-2 flex items-center gap-1 rounded-lg pr-1 transition-colors hover:bg-surface-hi">
      <button type="button" onClick={onOpen} className="flex min-w-0 flex-1 cursor-default items-center gap-3 px-2 py-1.5 text-left">
        <span className="grid size-8 shrink-0 place-items-center rounded-md bg-surface-hi text-ink-2 ring-1 ring-line-strong">
          <Icon className="size-4" aria-hidden />
        </span>
        <span className="min-w-0">
          <span className="block text-[13px] font-medium text-ink">{BAR_NAMES[bar]}</span>
          <span className="block truncate text-[12px] text-ink-3">{on ? designName(bar, resolved[bar].design) : "Off"}</span>
        </span>
      </button>
      <Switch label={`Show the ${BAR_NAMES[bar].toLowerCase()}`} checked={on} onChange={(next) => set([bar, "enabled"], next, next ? `Show the ${BAR_NAMES[bar].toLowerCase()}` : `Hide the ${BAR_NAMES[bar].toLowerCase()}`)} />
    </div>
  );
}

/** One bar: on or off, its design (live thumbnails), the common few, its modules; the rest one step away. */
export function BarInspector({ bar, wallpaper, onPick, onAdd, onBack }: { bar: BarKind; wallpaper: string | null; onPick: (pick: BarPick) => void; onAdd: (zone: ZoneId) => void; onBack: () => void }) {
  const editor = useBars();
  const { resolved, doc, update } = editor;
  const scoped = forBar(editor, bar);
  const on = resolved[bar].enabled;
  const own = doc[bar];
  const styled = own?.look !== undefined || own?.animation !== undefined;
  const zones = zonesOf(resolved, bar);

  function resetToDesign() {
    update((d) => {
      const strip = <S extends { look?: unknown; animation?: unknown }>({ look: _look, animation: _animation, ...rest }: S) => rest;
      return bar === "top" ? { ...d, top: strip(d.top ?? {}) } : { ...d, dock: strip(d.dock ?? {}) };
    }, `Reset the ${BAR_NAMES[bar].toLowerCase()} to its design`);
  }

  return (
    <>
      <InspectorHeader
        icon={BAR_ICONS[bar]}
        title={BAR_NAMES[bar]}
        detail={on ? designName(bar, resolved[bar].design) : "Off"}
        back={{ label: "Bars", onClick: onBack }}
        actions={styled && <IconButton icon={RotateCcw} variant="ghost" size="sm" label="Back to the design's own look and motion" onClick={resetToDesign} />}
      />
      <InspectorSection>
        <ToggleSetting path={[bar, "enabled"]} title={`Show the ${BAR_NAMES[bar].toLowerCase()}`} />
      </InspectorSection>
      {on && (
        <>
          <InspectorSection title="Design">
            <DesignPicker bar={bar} wallpaper={wallpaper} />
          </InspectorSection>
          <InspectorSection title="Layout">{bar === "top" ? <TopBarCommon /> : <DockCommon />}</InspectorSection>
          <BarsContext value={scoped}>
            <InspectorSection title="Look">
              <LookCommon />
            </InspectorSection>
          </BarsContext>
          {zones.length > 0 && (
            <InspectorSection title="Modules" action={<Button size="sm" variant="ghost" icon={Plus} onClick={() => onAdd(zones.at(-1) ?? "right")}>Add</Button>}>
              <ZoneLists bar={bar} zones={zones} onPick={onPick} />
            </InspectorSection>
          )}
          {bar === "dock" && (
            <InspectorSection title="Pinned apps">
              <DockPinned />
            </InspectorSection>
          )}
          <More label="More layout">{bar === "top" ? <TopBarMore /> : <DockMore />}</More>
          <BarsContext value={scoped}>
            <More label="More look">
              <LookPanel bar={bar} />
            </More>
            <More label="Motion">
              <MotionPanel bar={bar} />
            </More>
          </BarsContext>
        </>
      )}
    </>
  );
}

/** The bar's module lists: drag to reorder, remove, or pick one to set it up. */
function ZoneLists({ bar, zones, onPick }: { bar: BarKind; zones: readonly ZoneId[]; onPick: (pick: BarPick) => void }) {
  const { resolved, update } = useBars();
  return (
    <div data-setting={`bars.${bar}.modules`} className="setting-anchor flex flex-col gap-3">
      {zones.map((zone) => {
        const list = listOf(resolved, bar, zone);
        return (
          <div key={zone}>
            <div className="mb-1.5 text-[12px] text-ink-3">{ZONE_NAMES[zone]}</div>
            {list.length === 0 ? (
              <p className="rounded-lg px-3 py-2 text-[12px] text-ink-4 ring-1 ring-line-strong ring-dashed ring-inset">Empty</p>
            ) : (
              <SortableList
                label={`${BAR_NAMES[bar]}, ${ZONE_NAMES[zone].toLowerCase()}`}
                items={list}
                keyOf={(ref, i) => `${ref}:${i}`}
                onReorder={(from, to) => update((doc) => setList(doc, bar, zone, moved(list, from, to)), "Reorder modules")}
                renderItem={(ref, index) => {
                  const { label, icon: Icon } = describeModule(ref);
                  const gap = ref === BREAK;
                  return (
                    <>
                      <button type="button" disabled={gap} onClick={() => onPick({ bar, module: { zone, ref } })} className="flex min-w-0 flex-1 cursor-default items-center gap-2 px-1 text-left disabled:cursor-default">
                        {gap ? <span className="h-px flex-1 bg-line-hi/70" aria-label="Gap" /> : <Icon className="size-3.5 shrink-0 text-ink-3" aria-hidden />}
                        {!gap && <span className="min-w-0 truncate text-[13px] text-ink">{label}</span>}
                      </button>
                      <IconButton icon={X} variant="ghost" size="sm" label={`Remove ${gap ? "the gap" : label}`} className="opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100" onClick={() => update((doc) => setList(doc, bar, zone, list.toSpliced(index, 1)), `Remove ${gap ? "a gap" : label}`)} />
                    </>
                  );
                }}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

/** One module: what its clicks do, its icon and look; a custom one's command. */
export function ModuleInspector({ bar, zone, moduleRef, onBack }: { bar: BarKind; zone: ZoneId; moduleRef: ModuleRef; onBack: () => void }) {
  const { resolved, update } = useBars();
  const { label, hint, icon } = describeModule(moduleRef);
  const custom = isCustom(moduleRef);

  function removeHere() {
    update((doc) => setList(doc, bar, zone, listOf(resolved, bar, zone).filter((ref) => ref !== moduleRef)), `Remove ${label}`);
    onBack();
  }

  function deleteCustom() {
    update((doc) => withoutModule(doc, moduleRef), `Delete ${label}`);
    onBack();
  }

  return (
    <>
      <InspectorHeader
        icon={icon}
        title={label}
        detail={`${BAR_NAMES[bar]} · ${ZONE_NAMES[zone]}`}
        back={{ label: BAR_NAMES[bar], onClick: onBack }}
        actions={<IconButton icon={Trash2} variant="ghost" size="sm" label={`Remove ${label} from the ${BAR_NAMES[bar].toLowerCase()}`} onClick={removeHere} />}
      />
      <InspectorSection>
        <p className="text-[12px] leading-snug text-ink-3">{hint}.</p>
      </InspectorSection>
      <ModuleFields moduleRef={moduleRef} />
      {custom && (
        <InspectorSection>
          <Button size="sm" icon={Trash2} onClick={deleteCustom} className="self-start">
            Delete this module everywhere
          </Button>
        </InspectorSection>
      )}
    </>
  );
}

/** Adding to a bar: every module it doesn't show yet, a gap, or one of your own. */
export function ModuleCatalog({ bar, zone: initialZone, onAdded, onBack }: { bar: BarKind; zone: ZoneId; onAdded: (zone: ZoneId, ref: ModuleRef) => void; onBack: () => void }) {
  const { resolved, update } = useBars();
  const zones = zonesOf(resolved, bar);
  const [zone, setZone] = useState<ZoneId>(zones.includes(initialZone) ? initialZone : (zones.at(-1) ?? "right"));
  const placed = new Set(zones.flatMap((z) => listOf(resolved, bar, z)));
  const customs = Object.keys(resolved.modules).filter(isCustom);
  const available = [...BUILTIN_MODULES, ...customs].filter((ref) => !placed.has(ref));

  function add(ref: ModuleRef) {
    update((doc) => addModule(doc, resolved, bar, zone, ref), ref === BREAK ? "Add a gap" : `Add ${describeModule(ref).label}`);
    if (ref !== BREAK) onAdded(zone, ref);
  }

  return (
    <>
      <InspectorHeader icon={Plus} title="Add a module" detail={`To the ${BAR_NAMES[bar].toLowerCase()}`} back={{ label: BAR_NAMES[bar], onClick: onBack }} />
      {zones.length > 1 && (
        <InspectorSection>
          <Segmented<ZoneId> label="Where" hideLabel size="sm" className="w-full" value={zone} options={zones.map((z) => ({ value: z, label: ZONE_NAMES[z] }))} onChange={setZone} />
        </InspectorSection>
      )}
      <InspectorSection>
        <ul className="-mx-2 flex flex-col">
          {available.map((ref) => {
            const { label, hint, icon: Icon } = describeModule(ref);
            return (
              <li key={ref}>
                <button type="button" onClick={() => add(ref)} className="group flex w-full cursor-default items-center gap-3 rounded-lg px-2 py-2 text-left transition-colors hover:bg-surface-hi">
                  <span className="grid size-8 shrink-0 place-items-center rounded-md bg-surface-hi text-ink-2 ring-1 ring-line-strong">
                    <Icon className="size-4" aria-hidden />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] font-medium text-ink">{label}</span>
                    <span className="block truncate text-[12px] text-ink-3">{hint}</span>
                  </span>
                  <Plus className="size-4 shrink-0 text-ink-4 group-hover:text-ink" aria-hidden />
                </button>
              </li>
            );
          })}
          <li>
            <button type="button" onClick={() => add(BREAK)} className="group flex w-full cursor-default items-center gap-3 rounded-lg px-2 py-2 text-left transition-colors hover:bg-surface-hi">
              <span className="grid size-8 shrink-0 place-items-center rounded-md bg-surface-hi text-ink-3 ring-1 ring-line-strong">|</span>
              <span className="min-w-0 flex-1">
                <span className="block text-[13px] font-medium text-ink">Gap</span>
                <span className="block text-[12px] text-ink-3">A separator, or a new island</span>
              </span>
              <Plus className="size-4 shrink-0 text-ink-4 group-hover:text-ink" aria-hidden />
            </button>
          </li>
        </ul>
      </InspectorSection>
      <More label="Your own module">
        <CustomModuleForm onCreate={(ref, command) => {
          update((doc: BarsDoc) => addModule({ ...doc, modules: { ...doc.modules, [ref]: { command } } }, resolved, bar, zone, ref), `Add ${ref}`);
          onAdded(zone, ref);
        }} />
      </More>
    </>
  );
}

/** A module that shows a program's output: its name and the program (the host needs both). */
function CustomModuleForm({ onCreate }: { onCreate: (ref: ModuleRef, command: string) => void }) {
  const { resolved } = useBars();
  const [id, setId] = useState("");
  const [command, setCommand] = useState("");
  const ref = `custom:${id}`;
  const taken = ref in resolved.modules;
  const valid = CUSTOM_ID.test(id) && command.trim() !== "" && !taken;
  return (
    <div className="flex flex-col gap-2">
      <input aria-label="Name" value={id} placeholder="Name, like weather" spellCheck={false} maxLength={32} onChange={(e) => setId(e.currentTarget.value.toLowerCase())} className={`${INPUT_BASE} ui-input-sm w-full font-mono text-[12px]`} />
      <input aria-label="Program" value={command} placeholder="Program, like curl.exe" spellCheck={false} onChange={(e) => setCommand(e.currentTarget.value)} className={`${INPUT_BASE} ui-input-sm w-full font-mono text-[12px]`} />
      {id !== "" && !CUSTOM_ID.test(id) && <p className="text-[12px] text-ink-3">Use lowercase letters, digits, - and _.</p>}
      {taken && <p className="text-[12px] text-ink-3">There's already a module called {id}.</p>}
      <Button size="sm" variant="primary" icon={Plus} className="self-start" disabled={!valid} onClick={() => {
        onCreate(ref, command.trim());
        setId("");
        setCommand("");
      }}>
        Add it
      </Button>
      <p className="text-[12px] leading-snug text-ink-3">Its first line of output becomes its text. Set its arguments and how often it runs once it's added.</p>
    </div>
  );
}
