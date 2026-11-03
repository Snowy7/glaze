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

/**
 * A grayscale two-glow gradient seeded by the theme name, for cards without a
 * preview image. Pair it with the `.grain` overlay from index.css.
 */
export function placeholderBackground(name: string): string {
  const h = hash(name);
  const ax = 10 + (h % 40);
  const ay = 5 + ((h >>> 6) % 35);
  const bx = 55 + ((h >>> 12) % 40);
  const by = 60 + ((h >>> 18) % 35);
  const light = 0.34 + ((h >>> 4) % 14) / 100;
  const dim = 0.2 + ((h >>> 9) % 10) / 100;
  return [
    `radial-gradient(110% 90% at ${ax}% ${ay}%, oklch(${light} 0 0 / 0.95), transparent 62%)`,
    `radial-gradient(100% 100% at ${bx}% ${by}%, oklch(${dim} 0 0 / 0.9), transparent 60%)`,
    "oklch(0.17 0 0)",
  ].join(", ");
}
