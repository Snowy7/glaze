import { useEffect, useState } from "react";

import { api, errorText, type ThemeEntry } from "../lib/api";
import { Group, Skeleton, ToggleRow } from "./ui";

/** Wallpaper options of web and 3D scene themes: whether the mouse reaches the wallpaper. Saves at once. */
export function WallpaperEditor({ theme }: { theme: ThemeEntry }) {
  const [interactive, setInteractive] = useState<boolean | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    api.getInteractive(theme.dir).then(
      (value) => current && setInteractive(value),
      (e: unknown) => current && setError(errorText(e)),
    );
    return () => {
      current = false;
    };
  }, [theme.dir]);

  async function change(on: boolean) {
    setError(null);
    try {
      setInteractive(await api.setInteractive(theme.dir, on));
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <div className="flex-1 overflow-y-auto px-6 py-5">
      {interactive === undefined && !error ? (
        <Skeleton className="h-20 rounded-xl" />
      ) : interactive === null ? (
        <p className="text-[13px] text-ink-3">This wallpaper kind has no options here. Only web pages and 3D scenes can react to the mouse.</p>
      ) : (
        <Group>
          <ToggleRow
            title="Interactive"
            detail={theme.kind === "scene" ? "Dragging with the left button over the bare desktop orbits the camera." : "The page receives mouse moves, clicks and the wheel while the pointer is over the bare desktop."}
            checked={interactive === true}
            disabled={interactive === undefined}
            onChange={(on) => void change(on)}
          />
        </Group>
      )}
      <ul className="mt-5 list-disc space-y-1.5 pl-5 text-[12.5px] leading-relaxed text-ink-3 marker:text-ink-4">
        <li>Off by default. When off, nothing listens to the mouse at all.</li>
        <li>On, mypaper reads the mouse through Raw Input, like Lively does: no hook, nothing is swallowed, and the desktop behaves as before.</li>
        <li>Only the bare desktop counts: clicks on icons, windows, the taskbar or a widget never reach the wallpaper.</li>
        <li>It stops while a fullscreen app or game is in front.</li>
        <li>An interactive scene renders at its frame cap even when it would otherwise be still, so it can react.</li>
      </ul>
      {error && (
        <p role="alert" className="selectable mt-4 text-[13px] text-ink">
          {error}
        </p>
      )}
    </div>
  );
}
