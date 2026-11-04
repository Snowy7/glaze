import { X } from "lucide-react";

import { topGeometry } from "../../lib/barsDoc";
import { useBars } from "../../lib/barsContext";
import { OVERRIDE_SECTIONS, type Monitors, type TaskbarAlignment, type TaskbarMode, type TopLayout, type TopPosition } from "../../lib/barsSchema";
import { displayName } from "../../lib/desktopGrid";
import { Group, IconButton, INPUT_BASE, Prop, Segmented, Slider } from "../ui";
import { ChoiceSetting, GeometrySetting, SettingRow, SliderSetting, ToggleSetting, boolAt, px, stringAt } from "./BarSettings";
import { MonitorPicker } from "./MonitorPicker";

const LAYOUTS: readonly { value: TopLayout; label: string; hint?: string }[] = [
  { value: "strip", label: "Strip", hint: "Edge to edge, square" },
  { value: "floating", label: "Floating", hint: "Away from the edges, rounded" },
  { value: "islands", label: "Islands", hint: "One pill per group of modules" },
];

const POSITIONS: readonly { value: TopPosition; label: string }[] = [
  { value: "top", label: "Top" },
  { value: "bottom", label: "Bottom" },
];

/** The top bar's common few: its edge and height. */
export function TopBarCommon() {
  return (
    <>
      <ChoiceSetting<TopPosition> path={["top", "position"]} title="Edge" options={POSITIONS} fallback="top" />
      <SliderSetting path={["top", "height"]} title="Height" min={20} max={80} fallback={32} format={px} />
    </>
  );
}

/** The rest of the top bar's layout: where it shows, its shape and room, its clock. */
export function TopBarMore() {
  const { resolved, catalog, set } = useBars();
  const { top } = resolved;
  const geometry = topGeometry(resolved, catalog);
  return (
    <>
      <Group title="Shape and room">
        <ChoiceSetting<TopLayout> path={["top", "layout"]} title="Shape" detail={LAYOUTS.find((l) => l.value === top.layout)?.hint} options={LAYOUTS} fallback="strip" />
        {top.layout === "floating" && <WidthSetting />}
        <GeometrySetting path={["top", "margin"]} title="Margin" shown={geometry.margin} min={0} max={64} />
        <GeometrySetting path={["top", "padding"]} title="Padding" shown={geometry.padding} min={0} max={48} />
        <GeometrySetting path={["top", "spacing"]} title="Spacing" shown={geometry.spacing} min={0} max={32} />
      </Group>
      <Group title="Clock">
        <ClockSettings bar="top" />
      </Group>
      <Group title="Displays">
        <Prop id="bars.top.monitors" label="Show on" stacked>
          <MonitorPicker label="Top bar displays" value={top.monitors} onChange={(monitors: Monitors) => set(["top", "monitors"], monitors)} />
        </Prop>
      </Group>
    </>
  );
}

/** A clock's seconds and format (the top bar's, or a taskbar dock's own). */
export function ClockSettings({ bar }: { bar: "top" | "dock" }) {
  const { resolved, set } = useBars();
  const seconds = boolAt(resolved, [bar, "clock", "seconds"]);
  return (
    <>
      <ToggleSetting path={[bar, "clock", "seconds"]} title="Show seconds" />
      <SettingRow title="Format" detail="Letters like the example; \n starts a second line." path={[bar, "clock", "format"]} stacked>
        <input
          aria-label="Clock format"
          value={stringAt(resolved, [bar, "clock", "format"]) ?? ""}
          placeholder={seconds ? "ddd d MMM  HH:mm:ss" : "ddd d MMM  HH:mm"}
          maxLength={64}
          spellCheck={false}
          onChange={(e) => set([bar, "clock", "format"], e.currentTarget.value === "" ? undefined : e.currentTarget.value)}
          className={`${INPUT_BASE} ui-input-sm w-full font-mono text-[12px]`}
        />
      </SettingRow>
    </>
  );
}

type WidthMode = "full" | "auto" | "percent" | "pixels";

/** `top.width` of a floating bar: the whole edge, hugging its modules, a share, or DIPs. */
function WidthSetting() {
  const { resolved, set } = useBars();
  const width = resolved.top.width;
  const mode: WidthMode = width === null || width === "full" ? "full" : width === "auto" ? "auto" : typeof width === "number" ? "pixels" : "percent";
  const percent = typeof width === "string" ? Number.parseFloat(width) || 60 : 60;
  const pixels = typeof width === "number" ? width : 800;

  function pick(next: WidthMode) {
    set(["top", "width"], next === "full" ? "full" : next === "auto" ? "auto" : next === "percent" ? `${percent}%` : pixels);
  }

  return (
    <SettingRow title="Width" path={["top", "width"]} stacked>
      <div className="flex flex-col gap-2">
        <Segmented<WidthMode>
          label="Width"
          hideLabel
          size="sm"
          className="w-full"
          value={mode}
          onChange={pick}
          options={[
            { value: "full", label: "Full" },
            { value: "auto", label: "Fit" },
            { value: "percent", label: "Share" },
            { value: "pixels", label: "Pixels" },
          ]}
        />
        {mode === "percent" && <Slider label="Width share" value={percent} min={10} max={100} step={5} format={(v) => `${v}%`} onChange={(v) => set(["top", "width"], `${v}%`)} />}
        {mode === "pixels" && <Slider label="Width in pixels" value={pixels} min={120} max={3840} step={10} format={px} onChange={(v) => set(["top", "width"], v)} />}
      </div>
    </SettingRow>
  );
}

/** Per-display settings the file holds (`overrides`): which display, what each changes, and a way to drop one. */
export function OverridesList() {
  const { doc, update } = useBars();
  const entries = Object.entries(doc.overrides ?? {});
  if (entries.length === 0) return null;
  return (
    <div className="flex flex-col gap-2">
      <p className="text-[12px] leading-snug text-ink-3">Settings that change on one display only.</p>
      <ul className="flex flex-col rounded-lg bg-canvas p-1 ring-1 ring-line-strong ring-inset">
        {entries.map(([monitor, override]) => (
          <li key={monitor} className="flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-surface-hi">
            <span className="min-w-0 flex-1">
              <span className="block text-[13px] text-ink">{monitor.startsWith("\\\\") ? displayName(monitor) : monitor}</span>
              <span className="block truncate text-[12px] text-ink-3">{OVERRIDE_SECTIONS.filter((section) => override[section] !== undefined).join(", ") || "Nothing"}</span>
            </span>
            <IconButton
              icon={X}
              variant="ghost"
              size="sm"
              label={`Stop changing ${monitor} on its own`}
              onClick={() =>
                update((d) => {
                  const { [monitor]: _gone, ...rest } = d.overrides ?? {};
                  const { overrides: _old, ...others } = d;
                  return Object.keys(rest).length > 0 ? { ...others, overrides: rest } : others;
                }, "Remove a per-display setting")
              }
            />
          </li>
        ))}
      </ul>
    </div>
  );
}

const MODES: readonly { value: TaskbarMode; label: string }[] = [
  { value: "keep", label: "Keep" },
  { value: "autohide", label: "Auto-hide" },
  { value: "hide", label: "Hide" },
];

const MODE_HELP: Record<TaskbarMode, string> = {
  keep: "The Windows taskbar stays as it is.",
  autohide: "It hides until the pointer touches the screen edge.",
  hide: "It's gone on every display; your bars get its space.",
};

type AlignmentChoice = TaskbarAlignment | "default";

const ALIGNMENTS: readonly { value: AlignmentChoice; label: string }[] = [
  { value: "default", label: "As is" },
  { value: "center", label: "Center" },
  { value: "left", label: "Left" },
];

/** What happens to Windows' own taskbar. Always put back as it was when the bars quit. */
export function TaskbarSettings() {
  const { resolved, set } = useBars();
  const { taskbar, dock } = resolved;
  return (
    <>
      <SettingRow title="Windows taskbar" detail={MODE_HELP[taskbar.mode] + (taskbar.mode === "hide" && !dock.enabled ? " Turn the bottom bar on to replace it." : "")} path={["taskbar", "mode"]} stacked>
        <Segmented<TaskbarMode> label="Windows taskbar" hideLabel size="sm" className="w-full" value={taskbar.mode} options={MODES} onChange={(mode) => set(["taskbar", "mode"], mode === "keep" ? undefined : mode)} />
      </SettingRow>
      {taskbar.mode !== "hide" && (
        <SettingRow title="Its icons" path={["taskbar", "alignment"]}>
          <Segmented<AlignmentChoice> label="Taskbar alignment" hideLabel size="sm" value={taskbar.alignment ?? "default"} options={ALIGNMENTS} onChange={(choice) => set(["taskbar", "alignment"], choice === "default" ? undefined : choice)} />
        </SettingRow>
      )}
    </>
  );
}
