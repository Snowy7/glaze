// A theme's picture as a data URL, for pages that may only load their own files and `data:`
// images (the bars page behind its glass). Made once per picture, at most 1600 px wide.

import { useEffect, useState } from "react";

import { fileUrl } from "./api";

const MAX_WIDTH = 1600;
const made = new Map<string, Promise<string | null>>();

function toDataUrl(path: string): Promise<string | null> {
  return new Promise((resolve) => {
    const image = new Image();
    // The asset protocol allows Studio's origin, so the canvas isn't tainted.
    image.crossOrigin = "anonymous";
    image.onload = () => {
      const scale = Math.min(1, MAX_WIDTH / image.naturalWidth);
      const canvas = document.createElement("canvas");
      canvas.width = Math.round(image.naturalWidth * scale);
      canvas.height = Math.round(image.naturalHeight * scale);
      const context = canvas.getContext("2d");
      if (!context) return resolve(null);
      context.drawImage(image, 0, 0, canvas.width, canvas.height);
      try {
        resolve(canvas.toDataURL("image/jpeg", 0.86));
      } catch {
        resolve(null);
      }
    };
    image.onerror = () => resolve(null);
    image.src = fileUrl(path);
  });
}

/** The picture at `path` as a data URL: null while it's made, or when there is none. */
export function useWallpaperDataUrl(path: string | null): string | null {
  const [state, setState] = useState<{ path: string; url: string | null } | null>(null);
  useEffect(() => {
    if (!path) return;
    let current = true;
    let promise = made.get(path);
    if (!promise) {
      promise = toDataUrl(path);
      made.set(path, promise);
    }
    void promise.then((url) => current && setState({ path, url }));
    return () => {
      current = false;
    };
  }, [path]);
  return path && state?.path === path ? state.url : null;
}
