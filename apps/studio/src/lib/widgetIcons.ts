import {
  Bitcoin,
  Blocks,
  CalendarDays,
  Clock,
  CloudSun,
  Gauge,
  Grid3x3,
  Hourglass,
  Image,
  ListTodo,
  Music,
  Quote,
  Rocket,
  Rss,
  StickyNote,
  Timer,
  ToggleRight,
  type LucideIcon,
} from "lucide-react";

/** Icons for the bundled widgets (widgets/<id>), so the lists scan at a glance. */
const ICONS: Record<string, LucideIcon> = {
  calendar: CalendarDays,
  clock: Clock,
  countdown: Hourglass,
  crypto: Bitcoin,
  heatmap: Grid3x3,
  launcher: Rocket,
  "media-pill": Music,
  notes: StickyNote,
  "now-playing": Music,
  photo: Image,
  "quick-toggles": ToggleRight,
  quote: Quote,
  rss: Rss,
  "system-monitor": Gauge,
  timer: Timer,
  todo: ListTodo,
  weather: CloudSun,
};

/** A widget's icon by id; other widgets get a generic one. */
export const widgetIcon = (id: string): LucideIcon => ICONS[id] ?? Blocks;
