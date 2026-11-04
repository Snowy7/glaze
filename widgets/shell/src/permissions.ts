// The shell's permission rule for widget requests (docs/widgets.md, "Wire protocol"). Pure.
// It runs before a request is forwarded; the host checks again from the manifest on disk.

import type { WidgetManifest } from "./protocol";

type Granted = Pick<WidgetManifest, "permissions" | "plugins">;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null;

/** Why `method` is denied for this manifest, or `undefined` if it is allowed (or not the shell's to judge). */
export function denial(manifest: Granted, method: string, params: unknown): string | undefined {
  const needs = (permission: "system.stats" | "media" | "launch" | "theme" | "audio" | "user") =>
    manifest.permissions?.includes(permission)
      ? undefined
      : `"${method}" needs the "${permission}" permission in widget.json`;

  const plugin = (id: unknown) =>
    typeof id === "string" && manifest.plugins?.includes(id)
      ? undefined
      : `plugin ${JSON.stringify(id)} is not listed in "plugins" in widget.json`;

  switch (method) {
    case "system.stats":
      return needs("system.stats");
    case "media.current":
    case "media.control":
      return needs("media");
    case "launch":
    case "appIcon":
      return needs("launch");
    case "theme.current":
      return needs("theme");
    case "user.info":
      return needs("user");
    case "storage.get":
    case "storage.set": {
      // Instance storage needs nothing; a shared namespace needs `storage.shared:<ns>`.
      if (!isRecord(params) || params.shared === undefined) return undefined;
      const permission = `storage.shared:${String(params.shared)}`;
      return typeof params.shared === "string" && manifest.permissions?.some((p) => p === permission)
        ? undefined
        : `"${method}" on the shared namespace ${JSON.stringify(params.shared)} needs the "${permission}" permission in widget.json`;
    }
    case "plugin":
      return plugin(isRecord(params) ? params.plugin : undefined);
    case "subscribe": {
      if (!isRecord(params)) return undefined; // malformed: the host answers InvalidParams
      switch (params.topic) {
        case "system":
          return needs("system.stats");
        case "media":
          return needs("media");
        case "theme":
          return needs("theme");
        case "audio":
          return needs("audio");
        case "plugin":
          return plugin(params.plugin);
        default:
          return undefined; // unknown topic: the host answers
      }
    }
    default:
      return undefined; // hello, storage.*, pickFile (the dialog is the consent), unsubscribe need nothing; unknown methods go to the host
  }
}
