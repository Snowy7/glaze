// Pure decisions behind the top-bar glyphs and clicks.

import type { MediaInfo } from "../protocol";

/** Wi-Fi bars lit, 1 to 4 (the dot alone is 1). An unknown signal reads as full. */
export function signalLevel(signal: number | undefined): 1 | 2 | 3 | 4 {
  if (signal === undefined) return 4;
  return signal <= 25 ? 1 : signal <= 50 ? 2 : signal <= 75 ? 3 : 4;
}

export type VolumeGlyph = "mute" | "low" | "mid" | "high";

export function volumeGlyph({ level, muted }: { level: number; muted: boolean }): VolumeGlyph {
  if (muted || level <= 0) return "mute";
  return level < 1 / 3 ? "low" : level < 2 / 3 ? "mid" : "high";
}

/** The `workspace` requests needed to go from desktop `current` to `target` (0-based). */
export function workspaceSteps(
  current: number,
  target: number,
): { direction: "left" | "right"; count: number } | null {
  if (target === current) return null;
  return { direction: target < current ? "left" : "right", count: Math.abs(target - current) };
}

export function mediaLabel({ title, artist }: Pick<MediaInfo, "title" | "artist">): string {
  return artist ? `${title} — ${artist}` : title;
}

/** Mouse-wheel `deltaY` to a volume step: 100 is one 2% notch, capped so a flick can't jump. */
export function volumeDelta(deltaY: number): number {
  return Math.max(-0.06, Math.min(0.06, (-deltaY / 100) * 0.02));
}
