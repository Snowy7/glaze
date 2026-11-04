import { Cpu, Play, Square } from "lucide-react";

import { DesktopMenuEditor } from "../components/DesktopMenuEditor";
import { Badge, Block, Button, Group, Keycap, Page, Row, StatusDot, ToggleRow } from "../components/ui";
import type { Component, ComponentId } from "../lib/api";
import { useStudio } from "../lib/studio";

const COMPONENT_INFO: Record<ComponentId, { name: string; detail: string }> = {
  core: { name: "mypaper-core", detail: "The supervisor and tray icon. Starts the others." },
  wallpaper: { name: "mypaper-wallpaper", detail: "Draws the wallpaper behind your desktop icons." },
  widgets: { name: "mypaper-widgets", detail: "Desktop widgets: one transparent layer per monitor." },
  bars: { name: "mypaper-bars", detail: "The top bar, the dock and the taskbar options." },
};

/** Engine, playback, startup and desktop extras. Two columns once the page is wide enough. */
export function Settings() {
  const { config, updateConfig, engine, components, startEngine, stopEngine, autostart, setAutostart, busy } = useStudio();
  const engineBusy = busy.has("engine");

  return (
    <Page title="Settings" subtitle="Changes apply to the running wallpaper immediately." width="wide">
      <div className="@container">
        <div className="grid items-start gap-8 @[76rem]:grid-cols-2 @[76rem]:gap-10">
          <div className="flex min-w-0 flex-col gap-8">
            <Group title="Engine">
              <div className="flex flex-wrap items-center gap-x-4 gap-y-3 px-4 py-4">
                <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-surface-hi text-ink-2 ring-1 ring-line-strong">
                  <Cpu className="size-5" strokeWidth={1.6} aria-hidden />
                </span>
                <div className="min-w-0 flex-[1_1_14rem]">
                  <div className="flex items-center gap-2 text-[14px] font-semibold text-ink">
                    <StatusDot state={!engine ? "unknown" : engine.running ? "on" : "off"} />
                    {!engine ? "Checking" : engine.running ? "Running" : "Stopped"}
                  </div>
                  <div className="selectable mt-0.5 truncate font-mono text-xs text-ink-3" title={engine?.corePath ?? undefined}>
                    {engine?.corePath ?? (engine ? "mypaper-core.exe not found. Build the workspace or put it beside Studio." : "")}
                  </div>
                </div>
                {engine?.running ? (
                  <Button icon={Square} busy={engineBusy} onClick={() => void stopEngine()}>
                    Stop
                  </Button>
                ) : (
                  <Button variant="primary" icon={Play} busy={engineBusy} disabled={!engine?.corePath} onClick={() => void startEngine()}>
                    Start
                  </Button>
                )}
              </div>
              <Row title="Panic hotkey" detail="Stops mypaper and restores your desktop from any state.">
                <span className="flex items-center gap-1" aria-label="Control, Alt, Shift, Backspace">
                  <Keycap>Ctrl</Keycap>
                  <Keycap>Alt</Keycap>
                  <Keycap>Shift</Keycap>
                  <Keycap>Backspace</Keycap>
                </span>
              </Row>
            </Group>

            <Group title="Components" description="The programs mypaper is made of. Core starts the rest.">
              {components === null ? <Row title="Checking" /> : components.map((component) => <ComponentRow key={component.id} component={component} />)}
            </Group>

            <Group title="Playback">
              <ToggleRow
                title="Pause wallpaper"
                detail="Stops rendering until you resume. The engine keeps running."
                checked={config?.paused ?? false}
                disabled={!config}
                onChange={(paused) => void updateConfig({ paused })}
              />
              <ToggleRow
                title="Pause on battery"
                detail="Save power when the laptop is unplugged."
                checked={config?.pause_on_battery ?? false}
                disabled={!config}
                onChange={(pause_on_battery) => void updateConfig({ pause_on_battery })}
              />
              <ToggleRow
                title="Pause while gaming / fullscreen apps"
                detail="Gives games the whole GPU while a fullscreen window is in front."
                checked={config?.pause_on_fullscreen ?? false}
                disabled={!config}
                onChange={(pause_on_fullscreen) => void updateConfig({ pause_on_fullscreen })}
              />
            </Group>

            <Group title="Startup">
              <ToggleRow
                title="Start with Windows"
                detail={autostart?.enabled ? (autostart.command ?? "Starts when you sign in.") : "Launch mypaper when you sign in."}
                mono={!!autostart?.enabled && !!autostart.command}
                checked={autostart?.enabled ?? false}
                disabled={!autostart || !engine?.corePath || busy.has("autostart")}
                onChange={(enabled) => void setAutostart(enabled)}
              />
              {autostart?.enabled && (
                <Row
                  title="How it starts"
                  detail={
                    autostart.method === "task"
                      ? "A Task Scheduler logon task for your account, with no elevation. Windows starts it at sign-in itself, so mypaper can be up before Explorer has drawn the desktop."
                      : "The Run registry value (HKCU\\…\\Run). Explorer starts it after the desktop is up, which can be a few seconds later. A logon task couldn't be registered here."
                  }
                >
                  <Badge>{autostart.method === "task" ? "Logon task" : "Run key"}</Badge>
                </Row>
              )}
              <ToggleRow
                title="Show the theme's still at sign-in"
                detail="Also sets a still of the theme as your Windows wallpaper, so Windows shows the theme's look before mypaper has started. Turning it off puts your own wallpaper back. So does turning off Start with Windows, and the panic hotkey."
                checked={config?.static_snapshot ?? true}
                disabled={!config}
                onChange={(static_snapshot) => void updateConfig({ static_snapshot })}
              />
            </Group>
          </div>

          <div className="flex min-w-0 flex-col gap-8">
            <Group title="Desktop extras" description="Opt-in changes to Windows itself. Each one is undone exactly when you turn it off.">
              <ToggleRow
                title="mypaper's right-click menu on the desktop"
                detail="Right-clicking the bare desktop opens a themed menu instead of Explorer's. Icons, windows, the taskbar and Shift+right-click always get the native menu. It uses a mouse hook inside mypaper-core's own process, switched off while a fullscreen app is in front and removed when mypaper quits. Off by default; off installs nothing."
                checked={config?.desktop_menu.enabled ?? false}
                disabled={!config}
                onChange={(enabled) => void updateConfig({ desktop_menu: { enabled } })}
              />
              {config && (
                <Block title="Menu items" detail="Top to bottom. Hide an item by removing it; use the arrows to reorder.">
                  <DesktopMenuEditor items={config.desktop_menu.items} defaults={config.desktop_menu_default} onChange={(items) => void updateConfig({ desktop_menu: { items } })} />
                </Block>
              )}
              <ToggleRow
                title="Classic Windows 11 context menu"
                detail="Shows the full classic menu everywhere instead of Windows 11's short one with “Show more options”. This sets an undocumented registry value Microsoft doesn't promise to keep, and turning it off removes exactly what was added. Explorer reads it when it starts: restart Explorer or sign out and back in. mypaper never restarts Explorer for you."
                checked={config?.classic_context_menu ?? false}
                disabled={!config}
                onChange={(classic_context_menu) => void updateConfig({ classic_context_menu })}
              />
            </Group>
          </div>
        </div>
      </div>
    </Page>
  );
}

/** One program: running, stopped (found but not running), or missing (not built or installed). */
function ComponentRow({ component }: { component: Component }) {
  const info = COMPONENT_INFO[component.id];
  const state = component.running ? "Running" : component.path ? "Stopped" : "Not found";
  return (
    <Row title={info.name} detail={info.detail}>
      <span
        className={`flex h-6 items-center gap-2 rounded-md px-2 text-[12px] font-medium ${component.running ? "bg-surface-hi text-ink ring-1 ring-line-strong" : "text-ink-3"}`}
        title={component.path ?? `${component.exe} wasn't found beside Studio or in a target folder`}
      >
        <StatusDot state={component.running ? "on" : "off"} />
        {state}
      </span>
    </Row>
  );
}
