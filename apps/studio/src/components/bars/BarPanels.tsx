import { X } from "lucide-react";

import { topGeometry } from "../../lib/barsDoc";
import { useBars } from "../../lib/barsContext";
import { OVERRIDE_SECTIONS, type Monitors, type TaskbarAlignment, type TaskbarMode, type TopLayout, type TopPosition } from "../../lib/barsSchema";
import { Block, Group, IconButton, INPUT_CLASS, Row, Segmented, Slider } from "../ui";
import { ChoiceSetting, GeometrySetting, SettingRow, SliderSetting, ToggleSetting, boolAt, px, stringAt } from "./BarSettings";
import { MonitorPicker } from "./MonitorPicker";

const LAYOUTS: readonly { value: TopLayout; label: string; hint?: string }[] = [
  { value: "strip", label: "Strip", hint: "Edge to edge, square" },
  { value: "floating", label: "Floating", hint: "Detached from the edges, rounded" },
  { value: "islands", label: "Islands", hint: "A transparent rail with one pill per group" },
];

const POSITIONS: readonly { value: TopPosition; label: string }[] = [
  { value: "top", label: "Top" },
  { value: "bottom", label: "Bottom" },
];

export function TopBarPanel() {
  const { resolved, catalog, set } = useBars();
  const { top } = resolved;
  const geometry = topGeometry(resolved, catalog);
  const clockPlaced = [...top.left, ...top.center, ...top.right].includes("clock");

  return (
    <div className="flex flex-col gap-6">
      <Group>
        <ToggleSetting path={["top", "enabled"]} title="Top bar" detail="A bar along a screen edge. Maximized windows stop below it." fallback />
        <Block title="Monitors" detail="Where the top bar appears.">
          <MonitorPicker label="Top bar monitors" value={top.monitors} onChange={(monitors: Monitors) => set(["top", "monitors"], monitors)} />
        </Block>
      </Group>

      <Group title="Shape">
        <ChoiceSetting<TopLayout> path={["top", "layout"]} title="Layout" detail={LAYOUTS.find((l) => l.value === top.layout)?.hint} options={LAYOUTS} fallback="strip" wide />
        <ChoiceSetting<TopPosition> path={["top", "position"]} title="Edge" detail="Which screen edge it sits on." options={POSITIONS} fallback="top" />
        <SliderSetting path={["top", "height"]} title="Height" detail="The bar itself, in device-independent pixels." min={20} max={80} fallback={32} format={px} />
        <GeometrySetting path={["top", "margin"]} title="Margin" detail={top.layout === "strip" ? "Keeps it off the screen edges. A strip has none by default." : "Keeps it off the screen edges, and sets the gap between islands."} shown={geometry.margin} min={0} max={64} />
        <GeometrySetting path={["top", "padding"]} title="Padding" detail="Inside the bar, or inside each island." shown={geometry.padding} min={0} max={48} />
        <GeometrySetting path={["top", "spacing"]} title="Spacing" detail="Between modules." shown={geometry.spacing} min={0} max={32} />
        {top.layout === "floating" && <WidthSetting />}
      </Group>

      <Group title="Clock">
        <ToggleSetting
          path={["top", "clock", "seconds"]}
          title="Show seconds"
          detail={clockPlaced ? "The clock then ticks every second instead of every minute." : "Add the Clock module to a bar to show it."}
        />
        <SettingRow title="Format" detail="Day, date, month and 24-hour time look like the placeholder. A \n makes two lines. At most 64 characters." path={["top", "clock", "format"]}>
          <input
            aria-label="Clock format"
            value={stringAt(resolved, ["top", "clock", "format"]) ?? ""}
            placeholder={boolAt(resolved, ["top", "clock", "seconds"]) ? "ddd d MMM  HH:mm:ss" : "ddd d MMM  HH:mm"}
            maxLength={64}
            spellCheck={false}
            onChange={(e) => set(["top", "clock", "format"], e.currentTarget.value === "" ? undefined : e.currentTarget.value)}
            className={`${INPUT_CLASS} w-56 font-mono text-[13px]`}
          />
        </SettingRow>
      </Group>

      <OverridesList />
    </div>
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
    <SettingRow title="Width" detail="How much of the edge a floating bar takes; it stays centered." path={["top", "width"]}>
      <div className="flex w-96 flex-col gap-2">
        <Segmented<WidthMode>
          label="Width"
          hideLabel
          value={mode}
          onChange={pick}
          options={[
            { value: "full", label: "Full" },
            { value: "auto", label: "Hug modules" },
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

/** The file's per-monitor overrides: what each sets, and a way to drop one. */
function OverridesList() {
  const { doc, update } = useBars();
  const entries = Object.entries(doc.overrides ?? {});
  if (entries.length === 0) return null;
  return (
    <Group title="Per-monitor overrides">
      <Block title="Set in bars.json" detail="Each override is a partial config merged over the rest for one monitor (a device name, primary or secondary). Edit them in the file; here you can drop one.">
        <ul className="divide-y divide-line overflow-hidden rounded-xl bg-canvas ring-1 ring-line">
          {entries.map(([monitor, override]) => (
            <li key={monitor} className="flex items-center gap-3 px-3 py-2">
              <span className="font-mono text-[13px] text-ink">{monitor}</span>
              <span className="min-w-0 flex-1 truncate text-xs text-ink-3">
                {OVERRIDE_SECTIONS.filter((section) => override[section] !== undefined).join(", ") || "empty"}
              </span>
              <IconButton
                icon={X}
                label={`Remove the override for ${monitor}`}
                onClick={() =>
                  update((d) => {
                    const { [monitor]: _gone, ...rest } = d.overrides ?? {};
                    const { overrides: _old, ...others } = d;
                    return Object.keys(rest).length > 0 ? { ...others, overrides: rest } : others;
                  })
                }
              />
            </li>
          ))}
        </ul>
      </Block>
    </Group>
  );
}

const MODES: readonly { value: TaskbarMode; label: string }[] = [
  { value: "keep", label: "Keep" },
  { value: "autohide", label: "Auto-hide" },
  { value: "hide", label: "Hide" },
];

const MODE_HELP: Record<TaskbarMode, string> = {
  keep: "The Windows taskbar stays exactly as it is.",
  autohide: "The Windows taskbar hides until you push the pointer against the screen edge.",
  hide: "The Windows taskbar is hidden completely, on every monitor, and its space goes to your bars. Turn the dock on to replace it (the Windows 11 preset is a complete replacement).",
};

type AlignmentChoice = TaskbarAlignment | "default";

const ALIGNMENTS: readonly { value: AlignmentChoice; label: string }[] = [
  { value: "default", label: "Windows' choice" },
  { value: "center", label: "Center" },
  { value: "left", label: "Left" },
];

export function TaskbarPanel() {
  const { resolved, set } = useBars();
  const { taskbar, dock } = resolved;

  return (
    <div className="flex flex-col gap-6">
      <Group>
        <Block title="Windows taskbar" detail="What mypaper does to the native taskbar. Presets never change this: replacing it is always your choice.">
          <Segmented<TaskbarMode> label="Taskbar mode" hideLabel value={taskbar.mode} options={MODES} onChange={(mode) => set(["taskbar", "mode"], mode === "keep" ? undefined : mode)} />
          <p className="mt-3 text-[13px] text-ink-2">{MODE_HELP[taskbar.mode]}</p>
          <p className="mt-2 text-xs text-ink-3">
            Nothing is ever left changed: mypaper puts the taskbar back exactly as it was when the bars quit, on the panic hotkey, and at the next start after a crash.
          </p>
          {taskbar.mode === "hide" && !dock.enabled && (
            <p className="mt-3 rounded-lg bg-canvas px-3 py-2 text-[13px] text-ink ring-1 ring-line-hi">
              The dock is off, so nothing replaces the taskbar. Turn the dock on in the Dock tab, or open apps from the Start menu and the top bar.
            </p>
          )}
        </Block>
        <Row title="Alignment" detail="Where Windows 11 puts the taskbar icons. Leave it to Windows' choice to change nothing; the original is restored when you do.">
          <Segmented<AlignmentChoice>
            className="w-72"
            label="Taskbar alignment"
            hideLabel
            value={taskbar.alignment ?? "default"}
            options={ALIGNMENTS}
            onChange={(choice) => set(["taskbar", "alignment"], choice === "default" ? undefined : choice)}
          />
        </Row>
      </Group>
    </div>
  );
}
