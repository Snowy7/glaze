// The calendar (docs/bars.md, "Flyouts" → "Calendar"): today, a month you can page through, and
// the extra clocks of `calendar.zones`. All local: dates and names come from Intl.

import { button, h } from "../dom";
import { defineFlyout } from "../flyout";
import { fire } from "../host";
import { glyphs } from "./glyphs";
import { firstDayOf, monthGrid, offsetLabel, zoneLabel, zoneOffset } from "./model";
import { barsConfig, show } from "./surface";
import { iconButton } from "./ui";

export async function open(anchor: HTMLElement): Promise<void> {
  await show(calendar, null, { anchor, align: "center", extent: 520 });
}

const calendar = defineFlyout<null>("calendar", (_props, ctx) => {
  const config = barsConfig().calendar;
  const locale = navigator.language;
  const firstDay = firstDayOf(config.firstDay, locale);
  let now = new Date();
  let shown = { year: now.getFullYear(), month: now.getMonth() };

  const weekday = h("div", "cal-weekday");
  const today = h("div", "cal-today");
  const time = h("div", "cal-time");
  const title = h("span", "cal-month");
  const grid = h("div", "cal-grid");
  const zones = h("div", "cal-zones");
  const back = button("fly-link cal-back", "Today");

  const names = Array.from({ length: 7 }, (_, i) =>
    new Date(2026, 1, 1 + ((firstDay + i) % 7)).toLocaleDateString(locale, { weekday: "narrow" }),
  ); // 1 February 2026 is a Sunday

  const render = () => {
    weekday.textContent = now.toLocaleDateString(locale, { weekday: "long" });
    today.textContent = now.toLocaleDateString(locale, { day: "numeric", month: "long", year: "numeric" });
    time.textContent = now.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
    title.textContent = new Date(shown.year, shown.month, 1).toLocaleDateString(locale, { month: "long", year: "numeric" });
    back.hidden = shown.year === now.getFullYear() && shown.month === now.getMonth();
    grid.replaceChildren(
      ...names.map((n) => h("span", "cal-head", n)),
      ...monthGrid(shown.year, shown.month, firstDay, now)
        .flat()
        .map((day) => {
          const el = h("span", "cal-day", String(day.date.getDate()));
          el.classList.toggle("is-out", !day.inMonth);
          el.classList.toggle("is-today", day.today);
          const dow = day.date.getDay();
          el.classList.toggle("is-weekend", dow === 0 || dow === 6);
          return el;
        }),
    );
    const here = -now.getTimezoneOffset();
    zones.replaceChildren(
      ...config.zones.flatMap((zone) => {
        try {
          const offset = zoneOffset(now, zone.zone);
          const clock = now.toLocaleTimeString(locale, { timeZone: zone.zone, hour: "2-digit", minute: "2-digit" });
          const day = now.toLocaleDateString(locale, { timeZone: zone.zone, weekday: "short" });
          return [
            h(
              "div",
              "cal-zone",
              h("span", "cal-zone-name", zoneLabel(zone)),
              h("span", "cal-zone-offset", `${day} · ${offsetLabel(offset, here)}`),
              h("span", "cal-zone-time", clock),
            ),
          ];
        } catch {
          return []; // not a zone Intl knows
        }
      }),
    );
    zones.hidden = config.zones.length === 0;
  };

  const page = (by: number) => {
    const next = new Date(shown.year, shown.month + by, 1);
    shown = { year: next.getFullYear(), month: next.getMonth() };
    render();
  };
  back.addEventListener("click", () => {
    shown = { year: now.getFullYear(), month: now.getMonth() };
    render();
  });
  const open = button("fly-link", "Calendar");
  open.addEventListener("click", () => {
    fire("launch", { path: "outlookcal:" });
    ctx.close();
  });

  ctx.scope.on("clock", ({ now: at }) => {
    now = new Date(at);
    render();
  });
  fire("watch", { topic: "clock" });
  ctx.scope.add(() => fire("unwatch", { topic: "clock" }));
  render();

  const root = h(
    "div",
    "fly-panel cal",
    h("div", "cal-top", h("div", "cal-date", weekday, today), time),
    h("div", "cal-nav", title, back, h("span", "fly-gap"), iconButton(glyphs.chevronLeft, "Previous month", () => page(-1)), iconButton(glyphs.chevronRight, "Next month", () => page(1))),
    grid,
    zones,
    h("div", "fly-footer", open, h("span", "fly-gap"), iconButton(glyphs.gear, "Date & time settings", () => fire("openSettings", { uri: "ms-settings:dateandtime" }))),
  );
  root.addEventListener("wheel", (event) => page(event.deltaY > 0 ? 1 : -1), { passive: true });
  return root;
});
