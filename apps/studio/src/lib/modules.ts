import {
  AppWindow,
  BatteryMedium,
  Bell,
  Clock,
  Cpu,
  Ellipsis,
  LayoutGrid,
  Languages,
  Music,
  Power,
  Puzzle,
  Rows3,
  Search,
  SlidersHorizontal,
  Volume2,
  Wifi,
  type LucideIcon,
} from "lucide-react";

import { customId, isBuiltinModule, type BuiltinModule, type ModuleRef } from "./barsSchema";

type ModuleInfo = { label: string; hint: string; icon: LucideIcon; sample?: string };

/** The built-in modules (docs/bars.md, "Modules"): how Studio names and sketches them. */
export const MODULE_INFO: Record<BuiltinModule, ModuleInfo> = {
  start: { label: "Start", hint: "Opens the launcher (our Start menu)", icon: LayoutGrid },
  search: { label: "Search", hint: "Opens Windows Search", icon: Search, sample: "Search" },
  workspaces: { label: "Desktops", hint: "Virtual desktops: count and current", icon: Rows3, sample: "1 2 3" },
  app: { label: "App", hint: "The foreground app's name and icon", icon: AppWindow, sample: "Files" },
  clock: { label: "Clock", hint: "Time and date; opens the calendar", icon: Clock, sample: "Tue 4 Nov  10:42" },
  media: { label: "Media", hint: "Now playing: title and artist", icon: Music, sample: "Slow Light" },
  tray: { label: "Tray", hint: "Notification-area icons", icon: Ellipsis },
  network: { label: "Network", hint: "Wi-Fi, ethernet or offline; opens the network flyout", icon: Wifi },
  volume: { label: "Volume", hint: "Level and mute; opens the mixer", icon: Volume2 },
  battery: { label: "Battery", hint: "Percent and charging; hidden on desktops", icon: BatteryMedium, sample: "82%" },
  quick: { label: "Quick", hint: "Opens quick settings", icon: SlidersHorizontal },
  power: { label: "Power", hint: "Lock, sleep, sign out, restart, shut down", icon: Power },
  stats: { label: "Stats", hint: "CPU and RAM gauges", icon: Cpu, sample: "12%" },
  language: { label: "Language", hint: "The keyboard layout of the app in front; opens the layout list", icon: Languages, sample: "ENG" },
  notifications: { label: "Notifications", hint: "A bell with the number of notifications", icon: Bell, sample: "3" },
};

/** Name, hint and icon of any list entry: a built-in, a custom module, or a break. */
export function describeModule(ref: ModuleRef): ModuleInfo {
  if (isBuiltinModule(ref)) return MODULE_INFO[ref];
  if (ref === "|") return { label: "Break", hint: "A new island in the islands layout, a thin separator elsewhere", icon: Ellipsis };
  const id = customId(ref);
  return { label: id, hint: `A custom module: the output of a command (custom:${id})`, icon: Puzzle, sample: id };
}
