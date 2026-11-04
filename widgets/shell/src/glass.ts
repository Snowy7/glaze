// Snapshot-backed glass under the iframes. Cards stay visible until an image actually decodes.
import { lensMap } from "../../../packages/widget-sdk/src/lens";
import type { Slot } from "./frames";
import type { AppearanceValues, ShellState } from "./protocol";
import { platesOf } from "./plates";

const NS = "http://www.w3.org/2000/svg";
type Pane = { el: HTMLDivElement; image: HTMLDivElement; wash: HTMLDivElement; filter: SVGFilterElement; key: string };

export function createWidgetGlass(changed: () => void) {
  const defs = document.createElementNS(NS, "svg");
  defs.style.cssText = "position:absolute;width:0;height:0;pointer-events:none";
  document.body.append(defs);
  const panes = new Map<string, Pane>();
  let wanted: string | undefined;
  let ready: string | undefined;
  let serial = 0;

  function render(slots: Iterable<Slot>, state: ShellState | undefined, looks: ReadonlyMap<string, AppearanceValues>) {
    const backdrop = state?.layer !== "overlay" ? state?.backdrop : null;
    const url = backdrop?.url;
    if (wanted !== url) {
      wanted = url;
      if (!url) ready = undefined;
      if (url) {
        const image = new Image();
        image.src = url;
        image.decode().then(() => { if (wanted === url) { ready = url; changed(); } })
          .catch((error: unknown) => { if (wanted === url) { ready = undefined; changed(); } console.warn("[widgets] backdrop unavailable", error); });
      }
    }
    const seen = new Set<string>();
    for (const slot of slots) {
      const inst = state?.instances.find((i) => i.instance === slot.instance);
      const look = inst && (looks.get(slot.instance) ?? inst.appearance);
      const plate = platesOf([slot])[0];
      if (!look || look.style !== "liquid" || look.surface !== "glass" || !plate || !slot.rect || !backdrop || !ready) continue;
      seen.add(slot.instance);
      let pane = panes.get(slot.instance);
      if (!pane) {
        const el = document.createElement("div");
        const image = document.createElement("div");
        const wash = document.createElement("div");
        el.className = "widget-glass";
        image.className = "widget-glass-image";
        wash.className = "widget-glass-wash";
        el.append(image, wash);
        slot.el.prepend(el);
        const filter = document.createElementNS(NS, "filter");
        filter.id = `widget-lens-${++serial}`;
        filter.setAttribute("color-interpolation-filters", "sRGB");
        defs.append(filter);
        image.style.filter = `url(#${filter.id})`;
        pane = { el, image, wash, filter, key: "" };
        panes.set(slot.instance, pane);
      }
      const [px, py, w, h] = plate.rect;
      const x = px - slot.rect.x, y = py - slot.rect.y;
      const r = Math.min(plate.radius, w / 2, h / 2);
      const knobs = state?.glass;
      const blur = Math.min(48, Math.max(12, look.blur + (knobs?.blur ?? 0)));
      const margin = Math.ceil(blur * 3 + 24);
      Object.assign(pane.el.style, { left: `${x}px`, top: `${y}px`, width: `${w}px`, height: `${h}px`, borderRadius: `${r}px` });
      Object.assign(pane.image.style, { inset: `${-margin}px`, backgroundImage: `url("${ready}")`, backgroundSize: `${backdrop.width}px ${backdrop.height}px`, backgroundPosition: `${margin - slot.rect.x - x}px ${margin - slot.rect.y - y}px` });
      const tint = 1 - (1 - look.opacity) * (1 - (knobs?.tint ?? .06));
      pane.wash.style.background = `color-mix(in srgb, ${look.tint} ${tint * 100}%, transparent)`;
      pane.wash.style.setProperty("--specular", String((knobs?.highlight ?? .55) * (knobs?.intensity ?? 1)));
      const bend = (knobs?.refraction ?? .6) * (knobs?.intensity ?? 1);
      const key = JSON.stringify([w, h, r, blur, bend]);
      if (pane.key !== key) {
        pane.key = key;
        const bezel = Math.min(r + 8, Math.min(w, h) / 2);
        const map = lensMap({ width: w, height: h, radius: r, bezel, margin, flush: { top: false, left: false, right: false, bottom: false } }, Math.min(1, Math.sqrt(160000 / ((w + margin * 2) * (h + margin * 2)))));
        const canvas = document.createElement("canvas");
        canvas.width = map.width; canvas.height = map.height;
        canvas.getContext("2d")?.putImageData(new ImageData(map.data, map.width, map.height), 0, 0);
        pane.filter.setAttribute("x", "0"); pane.filter.setAttribute("y", "0");
        pane.filter.setAttribute("width", "1"); pane.filter.setAttribute("height", "1");
        pane.filter.setAttribute("primitiveUnits", "userSpaceOnUse");
        pane.filter.innerHTML = `<feGaussianBlur stdDeviation="${blur}" edgeMode="duplicate" result="blurred"/><feImage href="${canvas.toDataURL()}" x="0" y="0" width="${w + margin * 2}" height="${h + margin * 2}" preserveAspectRatio="none" result="map"/><feDisplacementMap in="blurred" in2="map" scale="${bezel * bend * 2}" xChannelSelector="R" yChannelSelector="G"/>`;
      }
    }
    for (const [id, pane] of panes) if (!seen.has(id)) { pane.el.remove(); pane.filter.remove(); panes.delete(id); }
  }
  return { render, has: (instance: string) => panes.has(instance), count: () => panes.size };
}
