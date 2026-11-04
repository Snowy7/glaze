import { dockGeometry, magnifyOf, previewOf, tidy } from "../../lib/barsDoc";
import { useBars } from "../../lib/barsContext";
import type { Attention, DockPosition, DockStyle, Extra, Indicator, Running } from "../../lib/barsSchema";
import { Block, Group, Segmented, Slider, Note } from "../ui";
import { ChoiceSetting, GeometrySetting, SettingRow, SliderSetting, ToggleSetting, px } from "./BarSettings";
import { MonitorPicker } from "./MonitorPicker";
import { PinnedList } from "./PinnedList";

const STYLES: readonly { value: DockStyle; label: string; hint?: string }[] = [
  { value: "floating", label: "Floating", hint: "A pill of icons centered on its edge, macOS-style" },
  { value: "taskbar", label: "Taskbar", hint: "A full-width bar: modules, task buttons, tray and clock, Windows-style" },
];

const POSITIONS: readonly { value: DockPosition; label: string }[] = [
  { value: "bottom", label: "Bottom" },
  { value: "top", label: "Top" },
  { value: "left", label: "Left" },
  { value: "right", label: "Right" },
];

const INDICATORS: readonly { value: Indicator; label: string; hint?: string }[] = [
  { value: "dot", label: "Dot", hint: "macOS: a dot, wider in front" },
  { value: "bar", label: "Bar", hint: "A line under the icon" },
  { value: "glow", label: "Glow", hint: "A soft light" },
  { value: "pill", label: "Pill", hint: "Windows 11: a short bar, longer in front" },
  { value: "none", label: "None" },
];

const ATTENTIONS: readonly { value: Attention; label: string; hint?: string }[] = [
  { value: "pulse", label: "Pulse", hint: "A light behind the icon pulses six times" },
  { value: "bounce", label: "Bounce", hint: "Three hops" },
  { value: "none", label: "None" },
];

type RunningChoice = "all" | "monitor" | "pinned";
const RUNNING_CHOICES: readonly { value: RunningChoice; label: string; hint?: string }[] = [
  { value: "all", label: "All apps", hint: "Every running app, pinned or not" },
  { value: "monitor", label: "On this monitor", hint: "Only apps with a window on this dock's monitor" },
  { value: "pinned", label: "Pinned only", hint: "Running apps that aren't pinned stay out" },
];
const toRunning = (choice: RunningChoice): Running => (choice === "all" ? true : choice === "monitor" ? "monitor" : false);
const fromRunning = (running: Running): RunningChoice => (running === true ? "all" : running === "monitor" ? "monitor" : "pinned");

export function DockPanel() {
  const { resolved, catalog, set } = useBars();
  const { dock } = resolved;
  const geometry = dockGeometry(resolved, catalog);
  const taskbar = dock.style === "taskbar";
  const magnify = magnifyOf(dock.magnify);
  const preview = previewOf(dock.preview);
  const vertical = dock.position === "left" || dock.position === "right";

  /** Magnify and previews are written whole: a bare `true` would replace the preset's scale and range. */
  const setMagnify = (patch: Partial<typeof magnify>) => set(["dock", "magnify"], { ...magnify, ...patch });
  const setPreview = (patch: Partial<typeof preview>) => set(["dock", "preview"], { ...preview, ...patch });

  return (
    <div className="flex flex-col gap-6">
      <Group>
        <ToggleSetting path={["dock", "enabled"]} title="Dock" detail="App icons in a floating dock, or a full-width taskbar. It can replace the Windows taskbar." />
        <Block title="Monitors" detail="Where the dock appears.">
          <MonitorPicker label="Dock monitors" value={dock.monitors} onChange={(monitors) => set(["dock", "monitors"], monitors)} />
        </Block>
        <ChoiceSetting<DockStyle> path={["dock", "style"]} title="Style" detail={STYLES.find((s) => s.value === dock.style)?.hint} options={STYLES} fallback="floating" />
        <ChoiceSetting<DockPosition> path={["dock", "position"]} title="Edge" detail="Which screen edge it sits on." options={POSITIONS} fallback="bottom" wide />
        <SliderSetting path={["dock", "size"]} title="Icon size" detail="In device-independent pixels." min={16} max={128} fallback={geometry.size} format={px} />
        <GeometrySetting path={["dock", "margin"]} title="Margin" detail="From the screen edge, and the same room toward the windows. A shadow or glow bigger than this is cut off." shown={geometry.margin} min={0} max={64} />
        <GeometrySetting path={["dock", "padding"]} title="Padding" detail="Around the icons." shown={geometry.padding} min={0} max={48} />
        <GeometrySetting path={["dock", "spacing"]} title="Spacing" detail="Between items." shown={geometry.spacing} min={0} max={32} />
        <ToggleSetting path={["dock", "autohide"]} title="Auto-hide" detail="Slides away after a second without the pointer and takes no screen space. Touch the screen edge to bring it back." />
      </Group>

      <Group title="Icons and indicators">
        <SettingRow title="Magnify" detail={taskbar ? "Off in the taskbar style: it only applies to a floating dock. Also off while Windows is set to reduce motion." : "Icons grow under the pointer. Off while Windows is set to reduce motion."} path={["dock", "magnify"]}>
          <div className="flex w-72 flex-col gap-2">
            <Segmented<"on" | "off"> label="Magnify" hideLabel value={magnify.enabled ? "on" : "off"} options={[{ value: "on", label: "On" }, { value: "off", label: "Off" }]} onChange={(v) => setMagnify({ enabled: v === "on" })} />
            {magnify.enabled && (
              <>
                <Slider label="Magnify scale" value={magnify.scale} min={1} max={3} step={0.05} format={(v) => `${v.toFixed(2)}x`} onChange={(v) => setMagnify({ scale: tidy(v) })} />
                <Slider label="Magnify reach" value={magnify.range} min={1} max={8} step={0.5} format={(v) => `${v} icons`} onChange={(v) => setMagnify({ range: tidy(v) })} />
              </>
            )}
          </div>
        </SettingRow>
        <ChoiceSetting<Indicator> path={["dock", "indicator"]} title="Running indicator" detail={INDICATORS.find((i) => i.value === dock.indicator)?.hint ?? "Colors are set in Look."} options={INDICATORS} fallback="dot" wide />
        <ToggleSetting path={["dock", "counter"]} title="Window count badge" detail="A number on apps that have more than one window." fallback />
        <ToggleSetting path={["dock", "titles"]} title="Window titles" detail={taskbar && !vertical ? "Task buttons show the window's title." : "Only for a horizontal taskbar."} />
        <ChoiceSetting<Attention> path={["dock", "attention"]} title="Asking for attention" detail="How an app with a flashing window shows." options={ATTENTIONS} fallback="pulse" />
        <ToggleSetting path={["dock", "bounce"]} title="Bounce while launching" detail="A launched app hops until it shows a window, at most four seconds." fallback />
      </Group>

      <Group title="Windows">
        <SettingRow title="Show running apps" detail="Apps that aren't pinned appear after the pinned ones while they run." path={["dock", "running"]}>
          <Segmented<RunningChoice> className="w-96" label="Show running apps" hideLabel value={fromRunning(dock.running)} options={RUNNING_CHOICES} onChange={(v) => set(["dock", "running"], toRunning(v))} />
        </SettingRow>
        <ToggleSetting path={["dock", "split"]} title="One item per window" detail="Instead of one per app. An app's other windows follow its first as their own items." />
        <SettingRow title="Window previews" detail="Hovering a running item opens a card per window with a live thumbnail and a close button." path={["dock", "preview"]}>
          <div className="flex w-72 flex-col gap-2">
            <Segmented<"on" | "off"> label="Window previews" hideLabel value={preview.enabled ? "on" : "off"} options={[{ value: "on", label: "On" }, { value: "off", label: "Off" }]} onChange={(v) => setPreview({ enabled: v === "on" })} />
            {preview.enabled && <Slider label="Preview delay" value={preview.delay} min={0} max={3000} step={50} format={(v) => `${v} ms`} onChange={(v) => setPreview({ delay: v })} />}
          </div>
        </SettingRow>
      </Group>

      <Group title="Extra items">
        <SettingRow title="After the apps" detail="Show desktop minimizes everything and back (Win+D). The Recycle Bin opens, and its menu empties it with Windows' own confirmation." path={["dock", "extras"]}>
          <Segmented<Extra | "both" | "none">
            className="w-80"
            label="Extra items"
            hideLabel
            value={dock.extras.length === 2 ? "both" : (dock.extras[0] ?? "none")}
            options={[
              { value: "none", label: "None" },
              { value: "desktop", label: "Show desktop" },
              { value: "trash", label: "Recycle Bin" },
              { value: "both", label: "Both" },
            ]}
            onChange={(v) => set(["dock", "extras"], v === "none" ? [] : v === "both" ? ["desktop", "trash"] : [v])}
          />
        </SettingRow>
      </Group>

      {taskbar && (
        <Group title="Full-width taskbar">
          <ChoiceSetting<"start" | "center"> path={["dock", "align"]} title="Task buttons" detail="Centered like Windows 11, or right after the left modules. Which modules sit before and after them is set in Modules." options={[{ value: "center", label: "Centered" }, { value: "start", label: "At the start" }]} fallback="center" />
        </Group>
      )}

      <Group title="Pinned">
        <Block title="Pinned items" detail="Shown first, in this order. Programs, shortcuts, folders and Store apps all work.">
          {dock.pinned === null ? (
            <div className="rounded-xl border border-dashed border-line-strong px-4 py-5 text-[13px] text-ink-3">
              No pins of your own: the dock uses the pinned shortcuts of the Windows taskbar (File Explorer when there are none).
              <div className="mt-3">
                <button type="button" onClick={() => set(["dock", "pinned"], [])} className="cursor-default rounded-lg px-3 py-1.5 text-[13px] font-medium text-ink ring-1 ring-line-strong hover:bg-surface-hi">
                  Choose my own pins
                </button>
              </div>
            </div>
          ) : (
            <>
              <PinnedList items={dock.pinned} onChange={(pinned) => set(["dock", "pinned"], pinned)} />
              <Note>
                An empty list is a choice and stays empty.{" "}
                <button type="button" onClick={() => set(["dock", "pinned"], undefined)} className="cursor-default underline decoration-line-hi underline-offset-2 hover:text-ink">
                  Go back to the Windows taskbar's pins
                </button>
              </Note>
            </>
          )}
        </Block>
      </Group>
    </div>
  );
}
