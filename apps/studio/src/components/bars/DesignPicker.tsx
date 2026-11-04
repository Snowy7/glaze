import { Check } from "lucide-react";

import { resolve } from "../../lib/barsDoc";
import { useBars } from "../../lib/barsContext";
import type { BarKind, BarsDoc, DockDesign, TopDesign } from "../../lib/barsSchema";
import { Button, Group } from "../ui";
import { BarPreview } from "./BarPreview";

const TOP: readonly { id: TopDesign; name: string; blurb: string }[] = [
  { id: "monochrome", name: "Monochrome", blurb: "A black glass strip with white type." },
  { id: "macos", name: "macOS", blurb: "The menu bar: liquid glass, the app's name in bold, status icons and the date on the right." },
  { id: "fluent", name: "Windows 11", blurb: "Mica-like, Segoe Fluent Icons, Fluent hover and press." },
  { id: "capsule", name: "Capsule", blurb: "A floating liquid-glass pill, iPadOS and visionOS style." },
  { id: "minimal", name: "Minimal", blurb: "No surface: the clock in the middle, a few status icons." },
  { id: "islands", name: "Islands", blurb: "A transparent rail with one pill per group." },
  { id: "neon", name: "Neon", blurb: "Dark glass, a neon gradient edge and glow." },
  { id: "floating", name: "Floating", blurb: "A detached, rounded bar." },
];

const DOCK: readonly { id: DockDesign; name: string; blurb: string }[] = [
  { id: "monochrome", name: "Monochrome", blurb: "A black glass pill with dots." },
  { id: "macos", name: "macOS dock", blurb: "A liquid-glass plate, magnify, running dots, the trash past a divider." },
  { id: "windows11", name: "Windows 11 taskbar", blurb: "Centered, 48 DIPs, Fluent icons, pill indicators, two-line clock." },
  { id: "windows10", name: "Windows 10 taskbar", blurb: "Left-aligned, 40 DIPs, square buttons, underline indicators." },
  { id: "chromeos", name: "ChromeOS shelf", blurb: "A launcher circle, centered apps, the status area as one pill." },
  { id: "fluent", name: "Fluent dock", blurb: "The Windows 11 taskbar as a floating, rounded island." },
  { id: "islands", name: "Islands", blurb: "A solid pill with bar indicators." },
  { id: "neon", name: "Neon", blurb: "Dark glass, glow indicators." },
  { id: "floating", name: "Floating", blurb: "A detached, rounded pill." },
];

/**
 * One bar's design gallery. Each card is the bar as the host would resolve it with that design
 * (the file's other settings still on top). Choosing one writes `<bar>.design`; the bar's own look
 * and motion stay, and "Reset" clears them so the design shows as made.
 */
export function DesignPicker({ bar }: { bar: BarKind }) {
  const { doc, resolved, catalog, set, update } = useBars();
  const designs = bar === "top" ? TOP : DOCK;
  const current = resolved[bar].design;
  const own = doc[bar];
  const styled = own?.look !== undefined || own?.animation !== undefined;

  const withDesign = (id: string): BarsDoc => {
    const shown: BarsDoc = { ...doc, top: { ...doc.top, enabled: bar === "top" }, dock: { ...doc.dock, enabled: bar === "dock" } };
    return bar === "top" ? { ...shown, top: { ...shown.top, design: id as TopDesign } } : { ...shown, dock: { ...shown.dock, design: id as DockDesign } };
  };

  return (
    <div className="flex flex-col gap-5">
      {styled && (
        <Group>
          <div className="flex items-center justify-between gap-4 px-5 py-4">
            <p className="text-[13px] text-ink-2">This bar has its own look or motion on top of its design.</p>
            <Button
              onClick={() =>
                update((d) => {
                  const strip = <S extends { look?: unknown; animation?: unknown }>({ look: _look, animation: _animation, ...rest }: S) => rest;
                  return bar === "top" ? { ...d, top: strip(d.top ?? {}) } : { ...d, dock: strip(d.dock ?? {}) };
                })
              }
            >
              Reset to the design
            </Button>
          </div>
        </Group>
      )}
      <ul className="grid grid-cols-2 gap-4">
        {designs.map(({ id, name, blurb }) => {
          const active = id === current;
          return (
            <li key={id} className={`rounded-2xl bg-surface p-3 shadow-card ring-1 ${active ? "ring-2 ring-ink" : "ring-line"}`}>
              <BarPreview config={resolve(catalog, withDesign(id))} catalog={catalog} width={330} wallpaper="dark" dense />
              <div className="mt-3 flex items-start justify-between gap-3 px-1 pb-1">
                <div className="min-w-0">
                  <h3 className="font-display text-[15px] font-semibold text-ink">{name}</h3>
                  <p className="mt-0.5 text-[12.5px] text-ink-3">{blurb}</p>
                </div>
                {active ? (
                  <span className="mt-0.5 flex shrink-0 items-center gap-1 rounded-full bg-solid px-2.5 py-1 text-xs font-medium text-solid-ink">
                    <Check className="size-3.5" aria-hidden />
                    In use
                  </span>
                ) : (
                  <Button className="shrink-0" onClick={() => set([bar, "design"], id)}>
                    Use
                  </Button>
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
