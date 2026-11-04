// Data seeds from the URL. `shared=<json>` seeds the shared storage namespaces: `{ "activity": { "todo": { "2026-11-02": 3 } } }`,
// so a widget that reads `storage.shared:<ns>` can be shot with data in it.

import type { FakeBackend } from "./fake";

export function seedSharedFromParams(backend: FakeBackend, q: URLSearchParams): void {
  // batterypct=<0..100> starts the fake battery at that charge.
  const pct = Number(q.get("batterypct"));
  if (q.has("batterypct") && Number.isFinite(pct)) backend.batteryPercent = Math.min(100, Math.max(0, pct));
  const raw = q.get("shared");
  if (!raw) return;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return;
    for (const [namespace, data] of Object.entries(parsed)) {
      if (typeof data === "object" && data !== null) backend.seedShared(namespace, { ...data });
    }
  } catch {
    /* ignore a malformed param */
  }
}
