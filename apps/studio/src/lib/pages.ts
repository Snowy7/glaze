import { AppWindow, Blocks, Image as ImageIcon, LayoutGrid, PanelTop, Settings, type LucideIcon } from "lucide-react";

import type { PageId, Route } from "./nav";

/** Studio's six places (docs/studio-ux.md, "Information architecture"), in the sidebar's order. */
export const PAGES: readonly { page: PageId; route: Route; label: string; icon: LucideIcon }[] = [
  { page: "desktop", route: { page: "desktop" }, label: "Desktop", icon: LayoutGrid },
  { page: "themes", route: { page: "themes" }, label: "Themes", icon: ImageIcon },
  { page: "widgets", route: { page: "widgets" }, label: "Widgets", icon: Blocks },
  { page: "bars", route: { page: "bars" }, label: "Bars", icon: PanelTop },
  { page: "windows", route: { page: "windows" }, label: "Windows", icon: AppWindow },
  { page: "settings", route: { page: "settings" }, label: "Settings", icon: Settings },
];
