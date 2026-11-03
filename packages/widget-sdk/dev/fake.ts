// Fake data for the mock host: stats, media, themes.

import type { Backend } from "./host";
import type { MediaAction, MediaState, SystemStats, ThemeInfo } from "../src/types";
import { MypaperError } from "../src";
import { mockPlugin } from "./mock-plugins";

export interface Track {
  title: string;
  artist: string;
  album: string;
  duration: number;
  hue: number;
}

export const TRACKS: readonly Track[] = [
  { title: "Midnight City Lights", artist: "Parallel Hours", album: "After Dark", duration: 243, hue: 265 },
  { title: "Slow Static", artist: "Glass Harbor", album: "Low Tide", duration: 198, hue: 190 },
  { title: "A Very Long Song Title That Needs To Be Truncated Gracefully", artist: "The Extremely Verbose Quartet Of Somewhere", album: "Overflow", duration: 321, hue: 20 },
];

export const THEMES: Record<string, ThemeInfo> = {
  "Mono (no palette)": { name: "mono" },
  "Warm sand": { name: "sand", palette: { dominant: "#2a2118", accent: "#e8b86a" } },
  "Cool ocean": { name: "ocean", palette: { dominant: "#0f1c26", accent: "#6ab7e8" } },
  "Violet dusk": { name: "dusk", palette: { dominant: "#1d1530", accent: "#b48cff" } },
};

/** A procedural cover so the artwork path (data URL) is exercised. */
const artworkCache = new Map<number, string>();
function artwork(hue: number): string {
  const cached = artworkCache.get(hue);
  if (cached) return cached;
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 240;
  const g = canvas.getContext("2d");
  if (!g) return "";
  const bg = g.createLinearGradient(0, 0, 240, 240);
  bg.addColorStop(0, `hsl(${hue} 70% 55%)`);
  bg.addColorStop(1, `hsl(${(hue + 60) % 360} 70% 18%)`);
  g.fillStyle = bg;
  g.fillRect(0, 0, 240, 240);
  g.fillStyle = `hsl(${hue} 80% 85% / 0.35)`;
  g.beginPath();
  g.arc(150, 95, 70, 0, Math.PI * 2);
  g.fill();
  g.fillStyle = "rgb(0 0 0 / 0.25)";
  g.fillRect(0, 170, 240, 70);
  const url = canvas.toDataURL("image/jpeg", 0.85);
  artworkCache.set(hue, url);
  return url;
}

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n));
const GB = 1024 ** 3;

export class FakeBackend implements Backend {
  // stats: a mean-reverting random walk with occasional bursts, so sparklines look alive
  private cpu = 18;
  private gpu = 9;
  private mem = 0.46;
  private down = 0;
  private up = 0;

  // media
  present = true;
  playing = true;
  private track = 0;
  private position = 52;
  private stamp = performance.now();

  /** Set by the harness to push `media` events and log launches. */
  onMediaChange: () => void = () => {};
  onLaunch: (target: string) => void = () => {};
  /** Resolves real plugin calls; the harness swaps in a fetch to the dev server. */
  pluginCall: (id: string, method: string, params: unknown) => Promise<unknown> = (id, method, params) =>
    mockPlugin(id, method, params);

  theme_: ThemeInfo = THEMES["Mono (no palette)"] ?? { name: "mono" };

  stats(): SystemStats {
    this.cpu = clamp(this.cpu + (18 - this.cpu) * 0.12 + (Math.random() - 0.5) * 14 + (Math.random() < 0.08 ? 35 : 0), 2, 98);
    this.gpu = clamp(this.gpu + (9 - this.gpu) * 0.1 + (Math.random() - 0.5) * 10 + (Math.random() < 0.05 ? 40 : 0), 0, 100);
    this.mem = clamp(this.mem + (Math.random() - 0.5) * 0.012, 0.3, 0.9);
    this.down = Math.random() < 0.3 ? Math.random() ** 2 * 18_000_000 : this.down * 0.5;
    this.up = Math.random() < 0.2 ? Math.random() ** 2 * 1_500_000 : this.up * 0.5;
    // Drains slowly while unplugged and charges while plugged in, so the ring moves in a long session.
    if (this.hasBattery) {
      this.batteryPercent = clamp(this.batteryPercent + (this.charging ? 0.15 : -0.1), 3, 100);
    }
    const network: SystemStats["network"] =
      this.netKind === "wifi"
        ? { kind: "wifi", name: "Home-5G", signal: 78 }
        : this.netKind === "ethernet"
          ? { kind: "ethernet", name: "Ethernet" }
          : { kind: "none" };
    return {
      cpu: this.cpu,
      memory: { used: this.mem * 32 * GB, total: 32 * GB },
      gpu: this.gpu,
      net: { up: this.up, down: this.down },
      disks: [
        { name: "C:", used: 612 * GB, total: 1000 * GB },
        { name: "D:", used: 1710 * GB, total: 1863 * GB },
        { name: "E:", used: 41 * GB, total: 238 * GB },
      ],
      ...(this.hasBattery ? { battery: { percent: this.batteryPercent, charging: this.charging } } : {}),
      uptime: this.bootSeconds + (performance.now() - this.stamp0) / 1000,
      network,
    };
  }

  // battery / network / uptime, set from the harness panel and URL
  hasBattery = true;
  charging = false;
  batteryPercent = 64;
  netKind: "wifi" | "ethernet" | "none" = "wifi";
  /** Seconds the fake machine had been up when the page loaded: 2 days, 5 hours. */
  bootSeconds = 2 * 86_400 + 5 * 3600 + 14 * 60;
  private stamp0 = performance.now();

  private livePosition(): number {
    const t = TRACKS[this.track];
    if (!t) return 0;
    const elapsed = this.playing ? (performance.now() - this.stamp) / 1000 : 0;
    return clamp(this.position + elapsed, 0, t.duration);
  }

  media(): MediaState | null {
    const t = TRACKS[this.track];
    if (!this.present || !t) return null;
    return {
      title: t.title,
      artist: t.artist,
      album: t.album,
      app: "FakePlayer.exe",
      playing: this.playing,
      position: this.livePosition(),
      duration: t.duration,
      artwork: artwork(t.hue),
    };
  }

  control(action: MediaAction): void {
    if (!this.present) return;
    this.position = this.livePosition();
    this.stamp = performance.now();
    switch (action) {
      case "play":
        this.playing = true;
        break;
      case "pause":
        this.playing = false;
        break;
      case "toggle":
        this.playing = !this.playing;
        break;
      case "next":
        this.track = (this.track + 1) % TRACKS.length;
        this.position = 0;
        break;
      case "previous":
        // Like most players: restart the track first, go back only near its start.
        if (this.position <= 3) this.track = (this.track + TRACKS.length - 1) % TRACKS.length;
        this.position = 0;
        break;
    }
    this.onMediaChange();
  }

  setMedia(mode: "playing" | "paused" | "none"): void {
    this.present = mode !== "none";
    this.playing = mode === "playing";
    this.position = this.present ? this.position : 0;
    this.stamp = performance.now();
    this.onMediaChange();
  }

  theme(): ThemeInfo {
    return this.theme_;
  }

  launch(target: string): void {
    this.onLaunch(target);
  }

  plugin(id: string, method: string, params: unknown): Promise<unknown> {
    return this.pluginCall(id, method, params).catch((err: unknown) => {
      throw err instanceof MypaperError ? err : new MypaperError("PluginUnavailable", String(err));
    });
  }
}
