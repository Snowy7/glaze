import {
  AppWindow,
  BatteryMedium,
  Clock,
  Cpu,
  Ellipsis,
  LayoutGrid,
  Music,
  Power,
  Rows3,
  SlidersHorizontal,
  Volume2,
  Wifi,
  type LucideIcon,
} from "lucide-react";

import type { BarModule } from "./api";

/** The top-bar modules (docs/bars.md, "Modules"): how Studio names and sketches them. */
export const MODULE_INFO: Record<BarModule, { label: string; hint: string; icon: LucideIcon; sample?: string }> = {
  start: { label: "Start", hint: "Opens the Start menu", icon: LayoutGrid },
  workspaces: { label: "Desktops", hint: "Virtual desktops: count and current", icon: Rows3, sample: "1 2 3" },
  app: { label: "App", hint: "The foreground app's name and icon", icon: AppWindow, sample: "Files" },
  clock: { label: "Clock", hint: "Time and date", icon: Clock, sample: "Tue 4 Nov  10:42" },
  media: { label: "Media", hint: "Now playing: title and artist", icon: Music, sample: "Slow Light" },
  tray: { label: "Tray", hint: "Notification-area icons (best effort)", icon: Ellipsis },
  network: { label: "Network", hint: "Wi-Fi, ethernet or offline", icon: Wifi },
  volume: { label: "Volume", hint: "Level and mute", icon: Volume2 },
  battery: { label: "Battery", hint: "Percent and charging; hidden on desktops", icon: BatteryMedium, sample: "82%" },
  quick: { label: "Quick", hint: "Opens quick settings", icon: SlidersHorizontal },
  power: { label: "Power", hint: "Lock, sleep, sign out, restart, shut down", icon: Power },
  stats: { label: "Stats", hint: "CPU and RAM gauges", icon: Cpu, sample: "12%" },
};
