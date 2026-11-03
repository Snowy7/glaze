import { Play, Square } from "lucide-react";
import type { ReactNode } from "react";

import { Button, Keycap, PageHeader, Switch } from "../components/ui";
import { useStudio } from "../lib/studio";

export function Settings() {
  const { config, updateConfig, engine, startEngine, stopEngine, autostart, setAutostart, busy } = useStudio();

  return (
    <>
      <PageHeader title="Settings" subtitle="Changes apply to the running wallpaper immediately." />

      <div className="flex max-w-2xl flex-col gap-8">
        <Group title="Engine">
          <div className="flex items-center gap-4 px-5 py-4">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2 text-[14px] font-medium">
                <span className={`size-2 rounded-full ${engine?.running ? "bg-ok" : "bg-ink-3"}`} aria-hidden />
                {!engine ? "Checking" : engine.running ? "Running" : "Stopped"}
              </div>
              <div className="selectable mt-0.5 truncate font-mono text-xs text-ink-3" title={engine?.corePath ?? undefined}>
                {engine?.corePath ?? (engine ? "mypaper-core.exe not found. Build the workspace or put it beside Studio." : "")}
              </div>
            </div>
            <Button
              variant="primary"
              icon={Play}
              busy={busy.has("engine") && !engine?.running}
              disabled={!engine || engine.running || !engine.corePath || busy.has("engine")}
              onClick={() => void startEngine()}
            >
              Start
            </Button>
            <Button
              icon={Square}
              busy={busy.has("engine") && !!engine?.running}
              disabled={!engine?.running || busy.has("engine")}
              onClick={() => void stopEngine()}
            >
              Stop
            </Button>
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
            detail={autostart?.command ?? "Launch mypaper when you sign in."}
            mono={!!autostart?.command}
            checked={autostart?.enabled ?? false}
            disabled={!autostart || busy.has("autostart")}
            onChange={(enabled) => void setAutostart(enabled)}
          />
        </Group>
      </div>
    </>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section aria-label={title}>
      <h2 className="mb-2.5 px-1 text-xs font-semibold tracking-[0.12em] text-ink-3 uppercase">{title}</h2>
      <div className="divide-y divide-line overflow-hidden rounded-2xl bg-surface shadow-card ring-1 ring-line">{children}</div>
    </section>
  );
}

function Row({ title, detail, mono, children }: { title: string; detail: string; mono?: boolean; children: ReactNode }) {
  return (
    <div className="flex items-center gap-4 px-5 py-4">
      <div className="min-w-0 flex-1">
        <div className="text-[14px] font-medium">{title}</div>
        <div className={`mt-0.5 text-[12.5px] text-ink-3 ${mono ? "selectable truncate font-mono text-xs" : ""}`} title={mono ? detail : undefined}>
          {detail}
        </div>
      </div>
      {children}
    </div>
  );
}

function ToggleRow({
  title,
  detail,
  mono,
  checked,
  disabled,
  onChange,
}: {
  title: string;
  detail: string;
  mono?: boolean;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <Row title={title} detail={detail} mono={mono}>
      <Switch label={title} checked={checked} disabled={disabled} onChange={onChange} />
    </Row>
  );
}
