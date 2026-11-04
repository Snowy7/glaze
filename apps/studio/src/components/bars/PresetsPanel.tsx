import { Check } from "lucide-react";
import { useState } from "react";

import { resolve } from "../../lib/barsDoc";
import { useBars } from "../../lib/barsContext";
import type { BarsDoc, PresetId } from "../../lib/barsSchema";
import { Button, Notice } from "../ui";
import { BarPreview } from "./BarPreview";
import { clearPaths, conflictsWith } from "./overrides";

const PRESETS: readonly { id: PresetId; name: string; blurb: string }[] = [
  { id: "monochrome", name: "Monochrome", blurb: "Black glass over the live wallpaper, white type, a floating dock. The default." },
  { id: "macos", name: "macOS", blurb: "A translucent menu bar and a magnifying dock with springy motion." },
  { id: "windows11", name: "Windows 11", blurb: "One full-width taskbar: Start, search, task buttons, tray and clock. No top bar." },
  { id: "islands", name: "Islands", blurb: "A transparent rail with one pill per group of modules." },
  { id: "neon-glass", name: "Neon glass", blurb: "Dark glass, neon gradient edges and glow." },
  { id: "floating", name: "Floating", blurb: "A detached, rounded top bar and dock." },
];

/** Gallery of the built-in presets, each drawn from its own values. Applying writes `preset`. */
export function PresetsPanel() {
  const { doc, catalog, update } = useBars();
  const current = doc.preset ?? "monochrome";
  const [pending, setPending] = useState<{ id: PresetId; conflicts: string[][]; needsBackground: boolean } | null>(null);

  function choose(id: PresetId) {
    const bundle = catalog.presets.find((p) => p.id === id)?.bundle ?? {};
    const conflicts = conflictsWith(doc, bundle);
    // The host merges a file's background over the preset's key by key: some pairs can't be parsed.
    const type = doc.look?.background?.type;
    const needsBackground = type !== undefined && !catalog.backgroundTypes[id].includes(type);
    if (conflicts.length === 0 && !needsBackground) apply(id, []);
    else setPending({ id, conflicts, needsBackground });
  }

  function apply(id: PresetId, clear: string[][]) {
    update((d): BarsDoc => ({ ...clearPaths(d, clear), preset: id }));
    setPending(null);
  }

  // What "clear" removes: the clashing values, and a background that can't be merged whole.
  const clearing = pending ? [...pending.conflicts, ...(pending.needsBackground ? [["look", "background"]] : [])] : [];

  return (
    <div className="flex flex-col gap-5">
      <p className="text-[13px] text-ink-3">
        A preset is a starting point: layout, look and motion. Whatever your <code className="font-mono text-xs">bars.json</code> sets goes on top of it, so you can tweak a preset and keep its character.
      </p>

      {pending && (
        <Notice title={`Apply ${PRESETS.find((p) => p.id === pending.id)?.name}?`}>
          {pending.conflicts.length > 0 ? (
            <>
              Your file sets {pending.conflicts.length} {pending.conflicts.length === 1 ? "value" : "values"} the preset also sets, so they would keep winning:
              <ul className="my-2 flex flex-wrap gap-1.5">
                {pending.conflicts.slice(0, 12).map((path) => (
                  <li key={path.join(".")} className="rounded-md bg-surface-hi px-1.5 py-0.5 font-mono text-[11.5px] text-ink-2">
                    {path.join(".")}
                  </li>
                ))}
                {pending.conflicts.length > 12 && <li className="px-1 text-xs text-ink-3">and {pending.conflicts.length - 12} more</li>}
              </ul>
            </>
          ) : (
            <p className="my-1">Your background can't be combined with this preset's (the host merges them key by key), so it has to go.</p>
          )}
          <div className="mt-2 flex flex-wrap gap-2">
            <Button variant="primary" onClick={() => apply(pending.id, clearing)}>
              Apply and clear these
            </Button>
            {!pending.needsBackground && <Button onClick={() => apply(pending.id, [])}>Apply, keep my changes</Button>}
            <Button variant="ghost" onClick={() => setPending(null)}>
              Cancel
            </Button>
          </div>
          {pending.needsBackground && pending.conflicts.length > 0 && <p className="mt-2 text-xs text-ink-3">Your background would be cleared in either case.</p>}
        </Notice>
      )}

      <ul className="grid grid-cols-2 gap-4">
        {PRESETS.map(({ id, name, blurb }) => {
          const active = id === current;
          return (
            <li key={id} className={`rounded-2xl bg-surface p-3 shadow-card ring-1 ${active ? "ring-2 ring-ink" : "ring-line"}`}>
              <BarPreview config={resolve(catalog, { preset: id })} catalog={catalog} width={330} wallpaper="dark" dense />
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
                  <Button className="shrink-0" onClick={() => choose(id)}>
                    Apply
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
