import { AppWindow, Box, Film, Globe, Image as ImageIcon, Images, Waves, type LucideIcon } from "lucide-react";

import type { Kind } from "./api";

export const KINDS: Record<Kind, { label: string; icon: LucideIcon; hint: string }> = {
  shader: { label: "Shader", icon: Waves, hint: "A WGSL fragment shader" },
  image: { label: "Image", icon: ImageIcon, hint: "One still picture" },
  slideshow: { label: "Slideshow", icon: Images, hint: "Pictures from a folder, cross-faded" },
  video: { label: "Video", icon: Film, hint: "A looping video" },
  scene: { label: "3D Scene", icon: Box, hint: "A glTF / GLB scene" },
  web: { label: "Web", icon: Globe, hint: "A web page, suspended while hidden" },
  app: { label: "App", icon: AppWindow, hint: "A program rendering into the desktop, such as a Unity player" },
};

/** Deterministic hash, so a theme keeps its placeholder look across launches. */
function hash(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h = Math.imul(h ^ text.charCodeAt(i), 16777619);
  }
  return h >>> 0;
}

/** A quiet grayscale gradient seeded by the theme name, for cards without a preview image. */
export function placeholderBackground(name: string): string {
  const h = hash(name);
  const angle = 145 + (h % 50);
  const top = 0.27 + ((h >>> 4) % 8) / 100;
  const bottom = 0.16 + ((h >>> 9) % 5) / 100;
  return `linear-gradient(${angle}deg, oklch(${top} 0 0), oklch(${bottom} 0 0))`;
}
