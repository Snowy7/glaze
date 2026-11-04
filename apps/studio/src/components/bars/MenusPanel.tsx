import { Plus, X } from "lucide-react";
import { useEffect, useState } from "react";

import { setIn } from "../../lib/barsDoc";
import { useBars } from "../../lib/barsContext";
import { LAUNCHER_LAYOUTS, type BuiltinModule, type LauncherLayout, type Weekday, type Zone } from "../../lib/barsSchema";
import { describeModule } from "../../lib/modules";
import { Block, Button, Group, IconButton, INPUT_BASE, Note, Notice, Row, Segmented, Switch } from "../ui";
import { SettingRow, ToggleSetting } from "./BarSettings";
import { PinnedList } from "./PinnedList";

const LAUNCHERS: Record<LauncherLayout, { label: string; blurb: string }> = {
  win11: { label: "Windows 11", blurb: "Search, pinned apps and recommended (recent files, else recent apps)." },
  grid: { label: "Grid", blurb: "A big grid of every app, like Launchpad." },
  compact: { label: "Compact", blurb: "Just a search bar, with recent apps until you type." },
  dashboard: { label: "Dashboard", blurb: "Search and pinned apps, a row of widgets (time, now playing, CPU / memory / battery) and the clipboard history." },
};

/** The flyouts the bars open (docs/bars.md, "Flyouts"): launcher, quick settings, network, mixer, language, calendar, notifications, tray. */
export function MenusPanel() {
  return (
    <div className="flex flex-col gap-6">
      <LauncherSection />
      <Group title="Quick settings, network and mixer">
        <ShowModule module="quick" detail="Wi-Fi and Bluetooth switches, power mode, battery saver, brightness and volume. Do not disturb, focus and night light show their state and open their Settings page: Windows has no documented way to change them." />
        <ShowModule module="network" detail="Wi-Fi networks in range with signal and security, connecting with a saved profile or a new password, ethernet, and a link for VPN. On Windows 11 24H2 and later it needs location access turned on, and says so when it isn't." />
        <ShowModule module="volume" detail="The default output's volume and every app playing sound, each with its own slider and mute." />
        <ToggleSetting
          path={["audio", "switchOutput"]}
          title="Let the mixer switch the output device"
          detail="Uses IPolicyConfig, an undocumented COM interface (the one Windows' own Sound settings page uses). Without it the mixer lists the devices and links to Sound settings."
        />
      </Group>
      <Group title="Notifications and language">
        <ShowModule module="notifications" detail="A bell with the number of notifications. Windows has no change events for unpackaged apps, so the count is re-read every 15 seconds while the bell is on screen, and every 4 seconds while the flyout is open. Nothing runs when it isn't shown." />
        <ShowModule module="language" detail="The keyboard layout of the app in front (ENG over US), with a list to pick another. Picking sends the documented layout-change request to that app; console windows and some Store apps ignore it." />
      </Group>
      <CalendarSection />
      <TraySection />
    </div>
  );
}

/** A switch that puts a module in the top bar's right side, or takes it out of every list. */
function ShowModule({ module, detail }: { module: BuiltinModule; detail: string }) {
  const { resolved, update } = useBars();
  const { top } = resolved;
  const placed = [...top.left, ...top.center, ...top.right].includes(module);
  const { label } = describeModule(module);

  function toggle(on: boolean) {
    update((doc) => {
      if (on) return setIn(doc, ["top", "right"], [...top.right, module]);
      return (["left", "center", "right"] as const).reduce((acc, zone) => (top[zone].includes(module) ? setIn(acc, ["top", zone], top[zone].filter((m) => m !== module)) : acc), doc);
    });
  }

  return (
    <Row title={`${label} module`} detail={<>{detail}{!placed && " Switch it on to add it to the top bar's right side; move it in Modules."}</>}>
      <Switch label={`Show the ${label} module in the top bar`} checked={placed} onChange={toggle} />
    </Row>
  );
}

function LauncherSection() {
  const { resolved, set } = useBars();
  const { launcher, taskbar } = resolved;
  const hook = launcher.winKey && taskbar.mode === "hide";

  return (
    <Group title="Launcher (the Start menu)">
      <Block title="Layout" detail="What the Start module opens.">
        <ul className="grid grid-cols-2 gap-2" role="radiogroup" aria-label="Launcher layout">
          {LAUNCHER_LAYOUTS.map((layout) => (
            <li key={layout}>
              <button
                type="button"
                role="radio"
                aria-checked={launcher.layout === layout}
                onClick={() => set(["launcher", "layout"], layout)}
                className={`w-full cursor-default rounded-xl p-3 text-left ring-1 transition ${launcher.layout === layout ? "bg-solid text-solid-ink ring-transparent" : "bg-canvas text-ink-2 ring-line-strong hover:ring-line-hi"}`}
              >
                <div className="text-[13.5px] font-semibold">{LAUNCHERS[layout].label}</div>
                <div className={`mt-0.5 text-[12px] ${launcher.layout === layout ? "opacity-75" : "text-ink-3"}`}>{LAUNCHERS[layout].blurb}</div>
              </button>
            </li>
          ))}
        </ul>
      </Block>
      <ToggleSetting path={["launcher", "files"]} title="Search recent files too" detail="Reads your Recent files folder when the launcher opens." fallback />
      <SettingRow
        title="Windows key opens the launcher"
        detail="A lone press of the Windows key opens this launcher instead of Windows' Start menu."
        path={["launcher", "winKey"]}
      >
        <Switch label="Windows key opens the launcher" checked={launcher.winKey} onChange={(on) => set(["launcher", "winKey"], on)} />
      </SettingRow>
      <div className="px-5 pb-4">
        <Notice title={hook ? "Active now" : "Not active right now"}>
          <p>
            This replaces what the Windows key does, so it only works while the Windows taskbar is hidden (Windows taskbar → Hide) and no fullscreen app is in front.
            {!hook && launcher.winKey && taskbar.mode !== "hide" && " The taskbar isn't hidden, so the Windows key still opens the native Start menu."}
          </p>
          <ul className="mt-2 list-disc space-y-1 pl-5 text-[12.5px] text-ink-3">
            <li>It is a low-level keyboard hook inside mypaper-bars' own process. Nothing is loaded into other programs, and every key passes through.</li>
            <li>The one thing it changes is the release of a lone Windows key. Shortcuts such as Win+E and Win+Shift+S are untouched.</li>
            <li>Whenever the launcher can't open (a fullscreen game, no bar to show it, a stuck bar) the native Start menu opens as usual.</li>
            <li>The hook is removed when the bars quit, on the panic hotkey, and if they crash.</li>
          </ul>
        </Notice>
      </div>
      <Block title="Pinned apps" detail="The launcher's pinned grid. Right-click an app in the launcher to pin it too.">
        {launcher.pinned === null ? (
          <div className="rounded-xl border border-dashed border-line-strong px-4 py-5 text-[13px] text-ink-3">
            The launcher shows the dock's pins.
            <div className="mt-3">
              <Button onClick={() => set(["launcher", "pinned"], resolved.dock.pinned ?? [])}>Pin different apps here</Button>
            </div>
          </div>
        ) : (
          <>
            <PinnedList items={launcher.pinned} onChange={(pinned) => set(["launcher", "pinned"], pinned)} />
            <Note>
              <button type="button" onClick={() => set(["launcher", "pinned"], undefined)} className="cursor-default underline decoration-line-hi underline-offset-2 hover:text-ink">
                Go back to the dock's pins
              </button>
            </Note>
          </>
        )}
      </Block>
    </Group>
  );
}

const FIRST_DAYS: readonly { value: Weekday | "locale"; label: string }[] = [
  { value: "locale", label: "Locale's" },
  { value: "monday", label: "Monday" },
  { value: "sunday", label: "Sunday" },
  { value: "saturday", label: "Saturday" },
];

function CalendarSection() {
  const { resolved, set } = useBars();
  const { calendar } = resolved;
  const zones = calendar.zones;
  const setZones = (next: Zone[]) => set(["calendar", "zones"], next.length === 0 ? undefined : next);

  return (
    <Group title="Calendar">
      <Block title="Extra clocks" detail="Local time, day and offset from here for up to four other places. Use IANA names such as America/New_York.">
        <ul className="space-y-2">
          {zones.map((zone, index) => (
            <li key={index} className="flex items-center gap-2">
              <ZoneName label={`Time zone ${index + 1}`} value={zone.zone} onCommit={(name) => setZones(zones.map((z, i) => (i === index ? { ...z, zone: name } : z)))} />
              <input
                aria-label={`Label for time zone ${index + 1}`}
                value={zone.label ?? ""}
                placeholder="Label (the city, by default)"
                onChange={(e) => setZones(zones.map((z, i) => (i === index ? withLabel(z, e.currentTarget.value) : z)))}
                className={`${INPUT_BASE} flex-1`}
              />
              <IconButton icon={X} label={`Remove time zone ${index + 1}`} onClick={() => setZones(zones.filter((_, i) => i !== index))} />
            </li>
          ))}
        </ul>
        <Button icon={Plus} className="mt-3" disabled={zones.length >= 4} onClick={() => setZones([...zones, { zone: "Europe/London" }])}>
          Add a clock
        </Button>
      </Block>
      <SettingRow title="First day of the week" path={["calendar", "firstDay"]}>
        <Segmented<Weekday | "locale"> className="w-96" label="First day of the week" hideLabel value={calendar.firstDay ?? "locale"} options={FIRST_DAYS} onChange={(v) => set(["calendar", "firstDay"], v === "locale" ? undefined : v)} />
      </SettingRow>
    </Group>
  );
}

/** A zone name. The host refuses an empty one, so only a name is written; leaving the box empty restores the last. */
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
      className={`${INPUT_BASE} w-56 font-mono text-[13px]`}
    />
  );
}

const withLabel = (zone: Zone, label: string): Zone => {
  const { label: _old, ...rest } = zone;
  return label === "" ? rest : { ...rest, label };
};

function TraySection() {
  const { resolved, set } = useBars();
  const hosting = resolved.tray.host || resolved.taskbar.mode === "hide";
  return (
    <Group title="Tray">
      <SettingRow title="Be the tray host" detail="Where the tray module gets the real notification-area icons from." path={["tray", "host"]}>
        <Switch label="Be the tray host" checked={resolved.tray.host} onChange={(on) => set(["tray", "host"], on)} />
      </SettingRow>
      <div className="px-5 pb-4">
        <Notice title={hosting ? "The tray host is running while a tray module is on screen" : "Off: the tray module reads Explorer's tray"}>
          <p>
            With the Windows taskbar hidden the tray host is always on; with this switch it also runs while the taskbar is kept. Turning it on replaces where apps send their tray icons:
          </p>
          <ul className="mt-2 list-disc space-y-1 pl-5 text-[12.5px] text-ink-3">
            <li>mypaper creates its own hidden Shell_TrayWnd and asks every running app to add its icons again, now to it. Nothing is injected into Explorer.</li>
            <li>Everything is passed on to Explorer's real tray, so its state stays correct, balloons still become toasts and nothing is lost.</li>
            <li>When it stops (the tray module is removed, the taskbar goes back to Keep, mypaper quits) apps re-register with Explorer. A crash leaves nothing behind.</li>
            <li>Apps that don't handle Windows' TaskbarCreated message keep their icon in Explorer's tray only, and elevated apps' icons arrive but can't always be clicked.</li>
          </ul>
        </Notice>
      </div>
    </Group>
  );
}
