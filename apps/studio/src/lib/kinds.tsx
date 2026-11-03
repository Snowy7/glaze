import { Box, Film, Image as ImageIcon, Images, Waves, type LucideIcon } from "lucide-react";

import type { Kind } from "./api";

export const KINDS: Record<Kind, { label: string; icon: LucideIcon; hint: string }> = {
  shader: { label: "Shader", icon: Waves, hint: "A WGSL fragment shader" },
  image: { label: "Image", icon: ImageIcon, hint: "One still picture" },
  slideshow: { label: "Slideshow", icon: Images, hint: "Pictures from a folder, cross-faded" },
  video: { label: "Video", icon: Film, hint: "A looping video" },
  scene: { label: "3D Scene", icon: Box, hint: "A glTF / GLB scene" },
};

/** Deterministic hash, so a theme keeps its placeholder colors across launches. */
function hash(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h = Math.imul(h ^ text.charCodeAt(i), 16777619);
  }
  return h >>> 0;
}

/** A soft two-blob gradient seeded by the theme name, for cards without a preview image. */
export function placeholderBackground(name: string): string {
  const h = hash(name);
  const a = h % 360;
  const b = (a + 50 + ((h >>> 9) % 70)) % 360;
  return [
    `radial-gradient(120% 90% at 18% 8%, oklch(0.78 0.13 ${a} / 0.95), transparent 62%)`,
    `radial-gradient(110% 100% at 92% 92%, oklch(0.62 0.16 ${b} / 0.85), transparent 58%)`,
    `oklch(0.24 0.05 ${a})`,
  ].join(", ");
}
