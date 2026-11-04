import { Check, Plus, X } from "lucide-react";
import { useEffect, useState } from "react";

import { useBars } from "../../lib/barsContext";
import { LAUNCHER_LAYOUTS, type LauncherLayout, type Weekday, type Zone } from "../../lib/barsSchema";
import { Button, Group, IconButton, INPUT_BASE, Segmented, Switch } from "../ui";
import { SettingRow, ToggleSetting } from "./BarSettings";
import { PinnedList } from "./PinnedList";

// What the bars' menus and panels do (docs/bars.md, "Flyouts"): the Start menu, sound, the calendar
// and the tray. Which modules open them is chosen on the bars themselves.

const LAUNCHERS: Record<LauncherLayout, { label: string; blurb: string }> = {
  win11: { label: "Windows 11", blurb: "Search, pinned apps and recent files" },
  grid: { label: "Grid", blurb: "Every app in a big grid" },
  compact: { label: "Compact", blurb: "Just search, with recent apps" },
  dashboard: { label: "Dashboard", blurb: "Search, pins, a row of widgets and the clipboard" },
};

/** The Start menu, sound, calendar and tray, for both bars. */
export function MenusPanel() {
  return (
    <>
      <LauncherSection />
      <Group title="Sound">
        <ToggleSetting path={["audio", "switchOutput"]} title="Switch the output device" detail="From the volume panel. Uses an undocumented part of Windows." />
      </Group>
      <CalendarSection />
      <TraySection />
    </>
  );
}

function LauncherSection() {
  const { resolved, set } = useBars();
  const { launcher, taskbar } = resolved;
  return (
    <Group title="Start menu">
      <div role="radiogroup" aria-label="Start menu layout" data-setting="bars.launcher.layout" className="setting-anchor -mx-2 flex flex-col">
        {LAUNCHER_LAYOUTS.map((layout) => {
          const on = launcher.layout === layout;
          return (
            <button key={layout} type="button" role="radio" aria-checked={on} onClick={() => set(["launcher", "layout"], layout, "Change the Start menu")} className="flex cursor-default items-center gap-3 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-surface-hi">
              <span className="min-w-0 flex-1">
                <span className={`block text-[13px] ${on ? "font-medium text-ink" : "text-ink-2"}`}>{LAUNCHERS[layout].label}</span>
                <span className="block truncate text-[12px] text-ink-3">{LAUNCHERS[layout].blurb}</span>
              </span>
              {on && <Check className="size-4 shrink-0 text-ink" aria-hidden />}
            </button>
          );
        })}
      </div>
      <ToggleSetting path={["launcher", "files"]} title="Search recent files" fallback />
      <SettingRow title="Windows key opens it" detail={launcher.winKey && taskbar.mode !== "hide" ? "Works while the Windows taskbar is hidden." : undefined} path={["launcher", "winKey"]}>
        <Switch label="Windows key opens the Start menu" checked={launcher.winKey} onChange={(on) => set(["launcher", "winKey"], on)} />
      </SettingRow>
      <SettingRow title="Pinned apps" path={["launcher", "pinned"]} stacked>
        {launcher.pinned === null ? (
          <div className="flex items-center justify-between gap-3">
            <span className="text-[12px] text-ink-3">The bottom bar's pins</span>
            <Button size="sm" onClick={() => set(["launcher", "pinned"], resolved.dock.pinned ?? [], "Pin other apps in the Start menu")}>
              Pin others
            </Button>
          </div>
        ) : (
          <PinnedList items={launcher.pinned} onChange={(pinned) => set(["launcher", "pinned"], pinned, "Change the Start menu's pins")} />
        )}
      </SettingRow>
    </Group>
  );
}

const FIRST_DAYS: readonly { value: Weekday | "locale"; label: string }[] = [
  { value: "locale", label: "Usual" },
  { value: "monday", label: "Mon" },
  { value: "sunday", label: "Sun" },
  { value: "saturday", label: "Sat" },
];

const MAX_CLOCKS = 4;

function CalendarSection() {
  const { resolved, set } = useBars();
  const { calendar } = resolved;
  const zones = calendar.zones;
  const setZones = (next: Zone[], label: string) => set(["calendar", "zones"], next.length === 0 ? undefined : next, label);

  return (
    <Group title="Calendar">
      <SettingRow title="Week starts on" path={["calendar", "firstDay"]} stacked>
        <Segmented<Weekday | "locale"> className="w-full" size="sm" label="Week starts on" hideLabel value={calendar.firstDay ?? "locale"} options={FIRST_DAYS} onChange={(v) => set(["calendar", "firstDay"], v === "locale" ? undefined : v)} />
      </SettingRow>
      <SettingRow title="Other time zones" path={["calendar", "zones"]} stacked>
        <div className="flex flex-col gap-2">
          {zones.map((zone, index) => (
            <div key={index} className="flex items-center gap-1.5">
              <ZoneName label={`Time zone ${index + 1}`} value={zone.zone} onCommit={(name) => setZones(zones.map((z, i) => (i === index ? { ...z, zone: name } : z)), "Change a time zone")} />
              <input
                aria-label={`Name for time zone ${index + 1}`}
                value={zone.label ?? ""}
                placeholder="Name"
                onChange={(e) => setZones(zones.map((z, i) => (i === index ? withLabel(z, e.currentTarget.value) : z)), "Rename a time zone")}
                className={`${INPUT_BASE} ui-input-sm w-24 min-w-0`}
              />
              <IconButton icon={X} variant="ghost" size="sm" label={`Remove time zone ${index + 1}`} onClick={() => setZones(zones.filter((_, i) => i !== index), "Remove a time zone")} />
            </div>
          ))}
          {zones.length < MAX_CLOCKS && (
            <Button size="sm" icon={Plus} className="self-start" onClick={() => setZones([...zones, { zone: "Europe/London" }], "Add a time zone")}>
              Add a time zone
            </Button>
          )}
        </div>
      </SettingRow>
    </Group>
  );
}

/** A zone name. The bars refuse an empty one, so only a name is written; leaving the box empty restores the last. */
function ZoneName({ label, value, onCommit }: { label: string; value: string; onCommit: (name: string) => void }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <input
      aria-label={label}
      value={draft}
      placeholder="Asia/Tokyo"
      spellCheck={false}
      onChange={(e) => {
        setDraft(e.currentTarget.value);
        if (e.currentTarget.value.trim() !== "") onCommit(e.currentTarget.value.trim());
      }}
      onBlur={() => setDraft(value)}
      className={`${INPUT_BASE} ui-input-sm min-w-0 flex-1 font-mono text-[12px]`}
    />
  );
}

const withLabel = (zone: Zone, label: string): Zone => {
  const { label: _old, ...rest } = zone;
  return label === "" ? rest : { ...rest, label };
};

/** With the Windows taskbar hidden the bars always collect the tray icons; otherwise it's a choice. */
function TraySection() {
  const { resolved } = useBars();
  if (resolved.taskbar.mode === "hide") return null;
  return (
    <Group title="Tray">
      <ToggleSetting path={["tray", "host"]} title="Collect the tray icons" detail="The bars gather every app's tray icon themselves, as when the Windows taskbar is hidden." />
    </Group>
  );
}
