// The audio mixer (docs/bars.md, "Flyouts" → "Mixer"): the output's volume and mute, the output
// devices (switchable only with `audio.switchOutput`, which uses an undocumented API; otherwise a
// link to Sound settings), and every app playing with its own volume and mute, live.

import { button, glyph, h } from "../dom";
import { defineFlyout } from "../flyout";
import { fire, request } from "../host";
import { glyphs } from "./glyphs";
import type { AudioSession, MixerState } from "./protocol";
import { show } from "./surface";
import { appIcon, header, notice, row, settingsLink, slider } from "./ui";

export async function open(anchor: HTMLElement): Promise<void> {
  await show(mixer, null, { anchor, align: "end", extent: 520 });
}

const muteGlyph = (muted: boolean, level: number) => glyph(muted || level === 0 ? glyphs.speakerMute : glyphs.speaker);

const mixer = defineFlyout<null>("mixer", (_props, ctx) => {
  // The output.
  const masterMute = button("fly-icon-button");
  const master = slider({ value: 0, label: "Volume", onInput: (level) => fire("volume", { set: level }) });
  const masterValue = h("span", "mix-value");
  const output = h("div", "mix-row mix-master", masterMute, master.el, masterValue);
  let masterMuted = false;
  masterMute.addEventListener("click", () => {
    request("volume", { mute: !masterMuted })
      .then(applyVolume)
      .catch((error: unknown) => console.warn("[bars] volume failed", error));
  });
  const applyVolume = (v: { level: number; muted: boolean } | null) => {
    output.hidden = !v;
    if (!v) return;
    masterMuted = v.muted;
    master.set(v.muted ? 0 : v.level);
    masterValue.textContent = v.muted ? "Muted" : `${Math.round(v.level * 100)}`;
    masterMute.replaceChildren(muteGlyph(v.muted, v.level));
    masterMute.title = v.muted ? "Unmute" : "Mute";
  };

  const devices = h("div", "fly-list mix-devices");
  const deviceName = h("span", "mix-device-name");
  const deviceToggle = button("mix-device", glyph(glyphs.speaker, "mix-device-glyph"), deviceName, glyph(glyphs.chevronDown, "mix-device-chevron"));
  let devicesOpen = false;
  deviceToggle.addEventListener("click", () => {
    devicesOpen = !devicesOpen;
    devices.hidden = !devicesOpen;
    deviceToggle.classList.toggle("is-open", devicesOpen);
    ctx.reposition();
  });
  devices.hidden = true;

  const apps = h("div", "mix-apps");
  const rows = new Map<string, { el: HTMLElement; set(session: AudioSession): void }>();

  const sessionRow = (session: AudioSession) => {
    const mute = button("fly-icon-button");
    const level = slider({ value: session.level, label: `${session.name} volume`, onInput: (v) => fire("setSession", { id: session.id, level: v }) });
    const name = h("span", "mix-app-name", session.name);
    const icon = session.system ? h("span", "fly-icon fly-glyph-icon", glyph(glyphs.speaker)) : appIcon(session.icon, session.name);
    const el = h("div", "mix-app", icon, h("div", "mix-app-body", name, h("div", "mix-row", mute, level.el)));
    let current = session;
    mute.addEventListener("click", () => fire("setSession", { id: current.id, mute: !current.muted }));
    const set = (next: AudioSession) => {
      current = next;
      name.textContent = next.name;
      level.set(next.muted ? 0 : next.level);
      mute.replaceChildren(muteGlyph(next.muted, next.level));
      mute.title = next.muted ? "Unmute" : "Mute";
      el.dataset.active = String(next.active);
    };
    set(session);
    return { el, set };
  };

  const apply = (state: MixerState) => {
    const current = state.devices.find((d) => d.default);
    deviceName.textContent = current?.name ?? "No output";
    devices.replaceChildren(
      ...state.devices.map((device) => {
        const item = row(null, device.name, "", ...(device.default ? [glyph(glyphs.check, "fly-check")] : []));
        item.classList.toggle("is-current", device.default);
        item.disabled = !state.switchable && !device.default;
        if (state.switchable && !device.default) {
          item.addEventListener("click", () => {
            request("setOutput", { id: device.id })
              .then(apply)
              .catch((error: unknown) => console.warn("[bars] setOutput failed", error));
          });
        }
        return item;
      }),
      ...(state.switchable ? [] : [settingsLink("Choose the output in Sound settings", "ms-settings:sound")]),
    );
    // Rows are kept across updates so a slider being dragged isn't rebuilt under the pointer.
    const seen = new Set<string>();
    const ordered = state.sessions.map((session) => {
      seen.add(session.id);
      const existing = rows.get(session.id);
      if (existing) {
        existing.set(session);
        return existing.el;
      }
      const made = sessionRow(session);
      rows.set(session.id, made);
      return made.el;
    });
    for (const id of [...rows.keys()]) if (!seen.has(id)) rows.delete(id);
    apps.replaceChildren(...(ordered.length ? ordered : [notice("No apps are playing")]));
    ctx.reposition();
  };

  request("mixer")
    .then(apply)
    .catch((error: unknown) => console.warn("[bars] mixer failed", error));
  ctx.scope.on("mixer", apply);
  ctx.scope.on("volume", applyVolume);
  for (const topic of ["mixer", "volume"] as const) {
    fire("watch", { topic });
    ctx.scope.add(() => fire("unwatch", { topic }));
  }
  return h(
    "div",
    "fly-panel mix",
    header("Sound"),
    deviceToggle,
    devices,
    output,
    h("div", "fly-subtitle", "Apps"),
    apps,
    h("div", "fly-footer", settingsLink("Volume mixer", "ms-settings:apps-volume"), settingsLink("Sound settings", "ms-settings:sound")),
  );
});
