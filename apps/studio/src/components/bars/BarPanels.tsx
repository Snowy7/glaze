import {
  DOCK_SIZE,
  TOP_HEIGHT,
  type BarsConfig,
  type DockPosition,
  type TaskbarAlignment,
  type TaskbarMode,
} from "../../lib/api";
import { Block, Group, INPUT_CLASS, Row, Segmented, Slider, ToggleRow } from "../ui";
import { ModuleLayout } from "./ModuleLayout";
import { MonitorPicker } from "./MonitorPicker";
import { PinnedList } from "./PinnedList";

/** Applies a change to the `bars.json` being edited. */
export type EditBars = (change: (config: BarsConfig) => BarsConfig) => void;

type PanelProps = { config: BarsConfig; edit: EditBars };

const px = (value: number) => `${value}px`;

const DEFAULT_CLOCK_FORMAT = "ddd d MMM  HH:mm";

export function TopBarPanel({ config, edit }: PanelProps) {
  const { top } = config;
  const change = (patch: Partial<BarsConfig["top"]>) => edit((c) => ({ ...c, top: { ...c.top, ...patch } }));
  const clockPlaced = [...top.left, ...top.center, ...top.right].includes("clock");

  function changeClock(patch: { seconds?: boolean; format?: string }) {
    edit((c) => {
      const { format, ...rest } = { seconds: false, ...c.top.clock, ...patch };
      // An empty format means "the default", which is the same as leaving it out.
      return { ...c, top: { ...c.top, clock: format ? { ...rest, format } : rest } };
    });
  }

  return (
    <div className="flex flex-col gap-6">
      <Group>
        <ToggleRow title="Top bar" detail="A macOS-style bar along the top of the screen. Maximized windows stop below it." checked={top.enabled} onChange={(enabled) => change({ enabled })} />
        <Block title="Monitors" detail="Where the top bar appears.">
          <MonitorPicker label="Top bar monitors" value={top.monitors} onChange={(monitors) => change({ monitors })} />
        </Block>
        <Row title="Height" detail="In device-independent pixels.">
          <Slider className="w-64" label="Top bar height" value={top.height} min={TOP_HEIGHT.min} max={TOP_HEIGHT.max} format={px} onChange={(height) => change({ height })} />
        </Row>
      </Group>

      <Group title="Modules">
        <Block title="Layout" detail="What the bar shows, and where.">
          <ModuleLayout value={top} onChange={(placement) => change(placement)} />
        </Block>
      </Group>

      <Group title="Clock">
        <ToggleRow
          title="Show seconds"
          detail={clockPlaced ? "The clock then ticks every second instead of every minute." : "Add the Clock module to the layout to show it."}
          checked={top.clock?.seconds ?? false}
          onChange={(seconds) => changeClock({ seconds })}
        />
        <Row title="Format" detail="Leave empty for the default. Day, date, month and 24-hour time look like the placeholder.">
          <input
            aria-label="Clock format"
            value={top.clock?.format ?? ""}
            placeholder={DEFAULT_CLOCK_FORMAT}
            spellCheck={false}
            onChange={(e) => changeClock({ format: e.currentTarget.value })}
            className={`${INPUT_CLASS} w-56 font-mono text-[13px]`}
          />
        </Row>
      </Group>
    </div>
  );
}

const POSITIONS: readonly { value: DockPosition; label: string }[] = [
  { value: "bottom", label: "Bottom" },
  { value: "left", label: "Left" },
  { value: "right", label: "Right" },
];

export function DockPanel({ config, edit }: PanelProps) {
  const { dock } = config;
  const change = (patch: Partial<BarsConfig["dock"]>) => edit((c) => ({ ...c, dock: { ...c.dock, ...patch } }));

  return (
    <div className="flex flex-col gap-6">
      <Group>
        <ToggleRow title="Dock" detail="App icons in a floating dock. It can replace the Windows taskbar." checked={dock.enabled} onChange={(enabled) => change({ enabled })} />
        <Block title="Monitors" detail="Where the dock appears.">
          <MonitorPicker label="Dock monitors" value={dock.monitors} onChange={(monitors) => change({ monitors })} />
        </Block>
        <Row title="Position" detail="Which screen edge the dock sits on.">
          <Segmented<DockPosition> className="w-64" label="Dock position" hideLabel value={dock.position} options={POSITIONS} onChange={(position) => change({ position })} />
        </Row>
        <Row title="Icon size" detail="In device-independent pixels.">
          <Slider className="w-64" label="Dock icon size" value={dock.size} min={DOCK_SIZE.min} max={DOCK_SIZE.max} format={px} onChange={(size) => change({ size })} />
        </Row>
      </Group>

      <Group>
        <ToggleRow title="Auto-hide" detail="Slides away after a second without the pointer and takes no screen space. Touch the screen edge to bring it back." checked={dock.autohide} onChange={(autohide) => change({ autohide })} />
        <ToggleRow title="Magnify" detail="Icons grow under the pointer. Off while Windows is set to reduce motion." checked={dock.magnify} onChange={(magnify) => change({ magnify })} />
        <ToggleRow title="Show running apps" detail="Apps that aren't pinned appear after the pinned ones while they run." checked={dock.running} onChange={(running) => change({ running })} />
      </Group>

      <Group title="Pinned">
        <Block title="Pinned items" detail="Shown first, in this order. Programs, shortcuts, folders and Store apps all work.">
          <PinnedList items={dock.pinned} onChange={(pinned) => change({ pinned })} />
        </Block>
      </Group>
    </div>
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
  hide: "The Windows taskbar is hidden completely, on every monitor, and its space goes to your bars. Turn the dock on to replace it.",
};

type AlignmentChoice = TaskbarAlignment | "default";

const ALIGNMENTS: readonly { value: AlignmentChoice; label: string }[] = [
  { value: "default", label: "Windows' choice" },
  { value: "center", label: "Center" },
  { value: "left", label: "Left" },
];

export function TaskbarPanel({ config, edit }: PanelProps) {
  const { taskbar, dock } = config;
  const change = (patch: Partial<BarsConfig["taskbar"]>) => edit((c) => ({ ...c, taskbar: { ...c.taskbar, ...patch } }));

  return (
    <div className="flex flex-col gap-6">
      <Group>
        <Block title="Windows taskbar" detail="What mypaper does to the native taskbar.">
          <Segmented<TaskbarMode> label="Taskbar mode" hideLabel value={taskbar.mode} options={MODES} onChange={(mode) => change({ mode })} />
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
        <Row title="Alignment" detail="Where Windows 11 puts the taskbar icons. Only applies while the taskbar is visible.">
          <Segmented<AlignmentChoice>
            className="w-72"
            label="Taskbar alignment"
            hideLabel
            value={taskbar.alignment ?? "default"}
            options={ALIGNMENTS}
            onChange={(choice) =>
              edit((c) => {
                const { alignment: _old, ...rest } = c.taskbar;
                return { ...c, taskbar: choice === "default" ? rest : { ...rest, alignment: choice } };
              })
            }
          />
        </Row>
      </Group>
    </div>
  );
}
