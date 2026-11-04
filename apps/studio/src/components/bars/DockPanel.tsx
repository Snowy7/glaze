import { dockGeometry, magnifyOf, previewOf, tidy } from "../../lib/barsDoc";
import { useBars } from "../../lib/barsContext";
import type { Attention, DockPosition, DockStyle, Extra, Indicator, Running } from "../../lib/barsSchema";
import { Button, Group, Prop, Segmented, Slider, Switch } from "../ui";
import { ClockSettings } from "./BarPanels";
import { ChoiceSetting, GeometrySetting, SettingRow, SliderSetting, ToggleSetting, px } from "./BarSettings";
import { MonitorPicker } from "./MonitorPicker";
import { PinnedList } from "./PinnedList";

// The bottom bar (the `dock` section): its common few, the rest, and its pinned apps. A setting
// that doesn't apply to the dock's style or edge isn't shown.

const STYLES: readonly { value: DockStyle; label: string; hint?: string }[] = [
  { value: "floating", label: "Floating", hint: "A pill of icons on its edge" },
  { value: "taskbar", label: "Taskbar", hint: "Edge to edge: modules, apps, tray and clock" },
];

const POSITIONS: readonly { value: DockPosition; label: string }[] = [
  { value: "bottom", label: "Bottom" },
  { value: "top", label: "Top" },
  { value: "left", label: "Left" },
  { value: "right", label: "Right" },
];

const INDICATORS: readonly { value: Indicator; label: string; hint?: string }[] = [
  { value: "dot", label: "Dot", hint: "A dot, wider in front" },
  { value: "bar", label: "Bar", hint: "A line under the icon" },
  { value: "glow", label: "Glow", hint: "A soft light" },
  { value: "pill", label: "Pill", hint: "A short bar, longer in front" },
  { value: "none", label: "None" },
];

const ATTENTIONS: readonly { value: Attention; label: string; hint?: string }[] = [
  { value: "pulse", label: "Pulse" },
  { value: "bounce", label: "Bounce" },
  { value: "none", label: "None" },
];

type RunningChoice = "all" | "monitor" | "pinned";
const RUNNING_CHOICES: readonly { value: RunningChoice; label: string; hint?: string }[] = [
  { value: "all", label: "All", hint: "Every running app, pinned or not" },
  { value: "monitor", label: "This display", hint: "Apps with a window on this display" },
  { value: "pinned", label: "Pinned only", hint: "Running apps that aren't pinned stay out" },
];
const toRunning = (choice: RunningChoice): Running => (choice === "all" ? true : choice === "monitor" ? "monitor" : false);
const fromRunning = (running: Running): RunningChoice => (running === true ? "all" : running === "monitor" ? "monitor" : "pinned");

/** The bottom bar's common few: its style, edge, size, auto-hide and (floating) magnify. */
export function DockCommon() {
  const { resolved, catalog, set } = useBars();
  const { dock } = resolved;
  const magnify = magnifyOf(dock.magnify);
  return (
    <>
      <ChoiceSetting<DockStyle> path={["dock", "style"]} title="Style" detail={STYLES.find((s) => s.value === dock.style)?.hint} options={STYLES} fallback="floating" />
      <ChoiceSetting<DockPosition> path={["dock", "position"]} title="Edge" options={POSITIONS} fallback="bottom" wide />
      <SliderSetting path={["dock", "size"]} title="Icon size" min={16} max={128} fallback={dockGeometry(resolved, catalog).size} format={px} />
      <ToggleSetting path={["dock", "autohide"]} title="Auto-hide" detail={dock.autohide ? "Touch the screen edge to bring it back." : undefined} />
      {dock.style === "floating" && (
        <SettingRow title="Magnify" path={["dock", "magnify"]}>
          <Switch label="Magnify" checked={magnify.enabled} onChange={(on) => set(["dock", "magnify"], { ...magnify, enabled: on })} />
        </SettingRow>
      )}
    </>
  );
}

/** The rest of the bottom bar: room, magnify's strength, indicators, windows, extras, displays. */
export function DockMore() {
  const { resolved, catalog, set } = useBars();
  const { dock } = resolved;
  const geometry = dockGeometry(resolved, catalog);
  const taskbar = dock.style === "taskbar";
  const vertical = dock.position === "left" || dock.position === "right";
  const magnify = magnifyOf(dock.magnify);
  const preview = previewOf(dock.preview);
  // Written whole: a bare `true` would replace the design's scale and reach.
  const setMagnify = (patch: Partial<typeof magnify>) => set(["dock", "magnify"], { ...magnify, ...patch });
  const setPreview = (patch: Partial<typeof preview>) => set(["dock", "preview"], { ...preview, ...patch });

  return (
    <>
      <Group title="Room">
        <GeometrySetting path={["dock", "margin"]} title="Margin" shown={geometry.margin} min={0} max={64} />
        <GeometrySetting path={["dock", "padding"]} title="Padding" shown={geometry.padding} min={0} max={48} />
        <GeometrySetting path={["dock", "spacing"]} title="Spacing" shown={geometry.spacing} min={0} max={32} />
        {!taskbar && magnify.enabled && (
          <>
            <Prop id="bars.dock.magnify.scale" label="Magnify by">
              <Slider className="w-[10.5rem] 2xl:w-[13rem]" label="Magnify scale" value={magnify.scale} min={1} max={3} step={0.05} format={(v) => `${v.toFixed(2)}x`} onChange={(v) => setMagnify({ scale: tidy(v) })} />
            </Prop>
            <Prop id="bars.dock.magnify.range" label="Magnify reach">
              <Slider className="w-[10.5rem] 2xl:w-[13rem]" label="Magnify reach" value={magnify.range} min={1} max={8} step={0.5} format={(v) => `${v} icons`} onChange={(v) => setMagnify({ range: tidy(v) })} />
            </Prop>
          </>
        )}
      </Group>
      {taskbar && (
        <Group title="Taskbar">
          <ChoiceSetting<"start" | "center"> path={["dock", "align"]} title="Apps" options={[{ value: "center", label: "Centered" }, { value: "start", label: "At the start" }]} fallback="center" />
          {!vertical && <ToggleSetting path={["dock", "titles"]} title="Window titles" />}
          <ClockSettings bar="dock" />
        </Group>
      )}
      <Group title="Apps">
        <SettingRow title="Running apps" detail={RUNNING_CHOICES.find((c) => c.value === fromRunning(dock.running))?.hint} path={["dock", "running"]} stacked>
          <Segmented<RunningChoice> className="w-full" size="sm" label="Running apps" hideLabel value={fromRunning(dock.running)} options={RUNNING_CHOICES} onChange={(v) => set(["dock", "running"], toRunning(v))} />
        </SettingRow>
        <ToggleSetting path={["dock", "split"]} title="One item per window" />
        <ToggleSetting path={["dock", "counter"]} title="Window count badge" fallback />
        <SettingRow title="Window previews" path={["dock", "preview"]}>
          <Switch label="Window previews" checked={preview.enabled} onChange={(on) => setPreview({ enabled: on })} />
        </SettingRow>
        {preview.enabled && (
          <Prop id="bars.dock.preview.delay" label="Preview after">
            <Slider className="w-[10.5rem] 2xl:w-[13rem]" label="Preview delay" value={preview.delay} min={0} max={3000} step={50} format={(v) => `${v} ms`} onChange={(v) => setPreview({ delay: v })} />
          </Prop>
        )}
        <ToggleSetting path={["dock", "bounce"]} title="Bounce while opening" fallback />
        <ChoiceSetting<Attention> path={["dock", "attention"]} title="Asking for attention" options={ATTENTIONS} fallback="pulse" />
        <ChoiceSetting<Indicator> path={["dock", "indicator"]} title="Running indicator" detail={INDICATORS.find((i) => i.value === dock.indicator)?.hint} options={INDICATORS} fallback="dot" />
        <SettingRow title="After the apps" path={["dock", "extras"]} stacked>
          <Segmented<Extra | "both" | "none">
            className="w-full"
            size="sm"
            label="After the apps"
            hideLabel
            value={dock.extras.length === 2 ? "both" : (dock.extras[0] ?? "none")}
            options={[
              { value: "none", label: "None" },
              { value: "desktop", label: "Desktop" },
              { value: "trash", label: "Bin" },
              { value: "both", label: "Both" },
            ]}
            onChange={(v) => set(["dock", "extras"], v === "none" ? [] : v === "both" ? ["desktop", "trash"] : [v])}
          />
        </SettingRow>
      </Group>
      <Group title="Displays">
        <Prop id="bars.dock.monitors" label="Show on" stacked>
          <MonitorPicker label="Bottom bar displays" value={dock.monitors} onChange={(monitors) => set(["dock", "monitors"], monitors)} />
        </Prop>
      </Group>
    </>
  );
}

/** The bottom bar's pinned apps: Windows' own pins, or a list of your own. */
export function DockPinned() {
  const { resolved, set } = useBars();
  const pinned = resolved.dock.pinned;
  if (pinned === null)
    return (
      <div data-setting="bars.dock.pinned" className="setting-anchor flex flex-col items-start gap-2.5">
        <p className="text-[12px] leading-snug text-ink-3">The Windows taskbar's pinned apps.</p>
        <Button size="sm" onClick={() => set(["dock", "pinned"], [], "Choose your own pinned apps")}>
          Choose my own
        </Button>
      </div>
    );
  return (
    <div data-setting="bars.dock.pinned" className="setting-anchor flex flex-col gap-2">
      <PinnedList items={pinned} onChange={(next) => set(["dock", "pinned"], next, "Change pinned apps")} />
      <button type="button" onClick={() => set(["dock", "pinned"], undefined, "Use the Windows taskbar's pins")} className="cursor-default self-start text-[12px] text-ink-3 underline decoration-line-hi underline-offset-2 hover:text-ink">
        Use the Windows taskbar's pins
      </button>
    </div>
  );
}
