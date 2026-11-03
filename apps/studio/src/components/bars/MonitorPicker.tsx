import { Check, Monitor } from "lucide-react";

import type { Monitors } from "../../lib/api";
import { useMonitors } from "../../lib/hooks";
import { Segmented } from "../ui";

type Mode = "all" | "primary" | "choose";

/** `\\.\DISPLAY2` -> `Display 2`. */
function displayName(device: string): string {
  const number = /DISPLAY(\d+)/i.exec(device)?.[1];
  return number ? `Display ${number}` : device;
}

/** Which monitors a bar shows on: all, the primary one, or a chosen few. */
export function MonitorPicker({ label, value, onChange }: { label: string; value: Monitors; onChange: (value: Monitors) => void }) {
  const monitors = useMonitors();
  const mode: Mode = value === "all" || value === "primary" ? value : "choose";
  const chosen = Array.isArray(value) ? value : [];
  // Chosen monitors that aren't connected stay listed, so they can be unchecked.
  const devices = [
    ...monitors.map((m) => ({ device: m.device, detail: `${m.width}×${m.height}${m.primary ? " · primary" : ""}`, connected: true })),
    ...chosen.filter((d) => !monitors.some((m) => m.device === d)).map((device) => ({ device, detail: "not connected", connected: false })),
  ];

  function pick(next: Mode) {
    if (next !== "choose") return onChange(next);
    const primary = monitors.find((m) => m.primary) ?? monitors[0];
    onChange(primary ? [primary.device] : []);
  }

  function toggle(device: string) {
    const next = chosen.includes(device) ? chosen.filter((d) => d !== device) : [...chosen, device];
    // At least one monitor stays selected: an empty list would show the bar nowhere.
    if (next.length > 0) onChange(next);
  }

  return (
    <div className="space-y-3">
      <Segmented<Mode>
        label={label}
        hideLabel
        value={mode}
        onChange={pick}
        options={[
          { value: "all", label: "All monitors" },
          { value: "primary", label: "Primary only" },
          { value: "choose", label: "Choose…" },
        ]}
      />
      {mode === "choose" && (
        <ul className="flex flex-wrap gap-2" aria-label="Monitors">
          {devices.map(({ device, detail, connected }) => {
            const on = chosen.includes(device);
            return (
              <li key={device}>
                <button
                  type="button"
                  role="checkbox"
                  aria-checked={on}
                  onClick={() => toggle(device)}
                  className={`flex cursor-default items-center gap-2.5 rounded-lg py-2 pr-3.5 pl-3 text-left text-[13px] ring-1 transition ${
                    on ? "bg-solid text-solid-ink ring-transparent" : "bg-canvas text-ink-2 ring-line-strong hover:ring-line-hi"
                  } ${connected ? "" : "opacity-70"}`}
                >
                  {on ? <Check className="size-4" aria-hidden /> : <Monitor className="size-4" aria-hidden />}
                  <span>
                    <span className="block font-medium">{displayName(device)}</span>
                    <span className={`block text-[11.5px] ${on ? "opacity-70" : "text-ink-3"}`}>{detail}</span>
                  </span>
                </button>
              </li>
            );
          })}
          {devices.length === 0 && <li className="text-[13px] text-ink-3">No monitors found.</li>}
        </ul>
      )}
    </div>
  );
}
