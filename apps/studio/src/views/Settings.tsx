import { ChevronRight, Play, Square } from "lucide-react";

import { Badge, Button, Group, Keycap, PageBar, Row, StatusDot, ToggleRow } from "../components/ui";
import type { Component, ComponentId } from "../lib/api";
import { useStudio } from "../lib/studio";

const PARTS: Record<ComponentId, { name: string; detail: string }> = {
  core: { name: "mypaper", detail: "Starts the others, and the tray icon" },
  wallpaper: { name: "Wallpaper", detail: "Draws the wallpaper" },
  widgets: { name: "Widgets", detail: "Shows the widgets" },
  bars: { name: "Bars", detail: "The top bar and the dock" },
};

/** How mypaper behaves: when it starts, when it rests, and the engine itself. */
export function Settings() {
  const { config, updateConfig, engine, components, startEngine, stopEngine, autostart, setAutostart, busy } = useStudio();
  const engineBusy = busy.has("engine");
  const toggle = (patch: Parameters<typeof updateConfig>[0], label: string) => void updateConfig(patch, { label });

  return (
    <>
      <PageBar title="Settings" />
      <div className="min-h-0 flex-1 overflow-y-auto px-5 pt-8 pb-14 [scrollbar-gutter:stable] lg:px-8">
        <div className="mx-auto flex max-w-[46rem] flex-col gap-9">
          <Group title="Startup">
            <div data-setting="settings.autostart" className="setting-anchor">
              <ToggleRow title="Start with Windows" detail="mypaper starts when you sign in." checked={autostart?.enabled ?? false} disabled={!autostart || !engine?.corePath || busy.has("autostart")} onChange={(on) => void setAutostart(on)} />
            </div>
            <div data-setting="settings.snapshot" className="setting-anchor">
              <ToggleRow
                title="Show the theme while Windows starts"
                detail="Uses a still of your theme as the Windows wallpaper, so it's there before mypaper is. Off puts your own wallpaper back."
                checked={config?.static_snapshot ?? true}
                disabled={!config}
                onChange={(on) => toggle({ static_snapshot: on }, on ? "Show the theme at startup" : "Stop showing the theme at startup")}
              />
            </div>
          </Group>

          <Group title="Power">
            <div data-setting="settings.battery" className="setting-anchor">
              <ToggleRow title="Pause on battery" detail="The wallpaper rests while the laptop is unplugged." checked={config?.pause_on_battery ?? false} disabled={!config} onChange={(on) => toggle({ pause_on_battery: on }, on ? "Pause on battery" : "Keep playing on battery")} />
            </div>
            <div data-setting="settings.fullscreen" className="setting-anchor">
              <ToggleRow title="Pause behind fullscreen apps" detail="Games and videos get the whole graphics card." checked={config?.pause_on_fullscreen ?? false} disabled={!config} onChange={(on) => toggle({ pause_on_fullscreen: on }, on ? "Pause behind fullscreen apps" : "Keep playing behind fullscreen apps")} />
            </div>
          </Group>

          <Group title="mypaper">
            <div data-setting="settings.engine" className="setting-anchor">
              <Row title={!engine ? "Checking…" : engine.running ? "Running" : "Stopped"} detail={engine && !engine.corePath ? "mypaper isn't installed next to Studio." : engine?.running ? "Your wallpaper, widgets and bars are on." : "Nothing of mypaper is on your desktop."}>
                {engine?.running ? (
                  <Button icon={Square} busy={engineBusy} onClick={() => void stopEngine()}>
                    Stop
                  </Button>
                ) : (
                  <Button variant="primary" icon={Play} busy={engineBusy} disabled={!engine?.corePath} onClick={() => void startEngine()}>
                    Start
                  </Button>
                )}
              </Row>
            </div>
            <div data-setting="settings.panic" className="setting-anchor">
              <Row title="Emergency stop" detail="Stops mypaper and puts your desktop back, whatever state it's in.">
                <span className="flex items-center gap-1" aria-label="Control, Alt, Shift, Backspace">
                  <Keycap>Ctrl</Keycap>
                  <Keycap>Alt</Keycap>
                  <Keycap>Shift</Keycap>
                  <Keycap>Backspace</Keycap>
                </span>
              </Row>
            </div>
            <details data-setting="settings.parts" className="setting-anchor group/parts">
              <summary className="flex h-12 cursor-default list-none items-center gap-2 px-4 text-[13.5px] font-medium text-ink-2 hover:bg-surface-hi/50 hover:text-ink [&::-webkit-details-marker]:hidden">
                <ChevronRight className="size-4 text-ink-3 transition-transform group-open/parts:rotate-90" aria-hidden />
                Details
              </summary>
              <div className="divide-y divide-line border-t border-line">
                {components === null ? <Row title="Checking…" /> : components.map((component) => <PartRow key={component.id} component={component} />)}
                {autostart?.enabled && <Row title="How it starts" detail={autostart.method === "task" ? "A sign-in task, so it's up before the desktop draws." : "A startup entry, a few seconds after the desktop."} mono={false} />}
                {engine?.corePath && <Row title="Installed at" detail={engine.corePath} mono />}
              </div>
            </details>
          </Group>
        </div>
      </div>
    </>
  );
}

/** One part of mypaper: on, off, or not installed. */
function PartRow({ component }: { component: Component }) {
  const part = PARTS[component.id];
  const state = component.running ? "On" : component.path ? "Off" : "Not installed";
  return (
    <Row title={part.name} detail={part.detail}>
      <Badge tone={component.running ? "neutral" : "outline"}>
        <StatusDot state={component.running ? "on" : "off"} />
        {state}
      </Badge>
    </Row>
  );
}
