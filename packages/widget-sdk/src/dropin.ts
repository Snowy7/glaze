// Entry of the drop-in build (`dist/mypaper.js`): `<script src="mypaper.js"></script>` defines
// the global `mypaper`, for widgets with no build step. It also applies the appearance
// (surface, text tone, variables, theme CSS) to the page, so a widget only has to use the variables.
/// <reference path="../types/global.d.ts" />
import { applyAppearance, DEFAULT_APPEARANCE } from "./appearance";
import { connect } from "./index";
import type { AppearanceState } from "./types";

const mypaper = connect();
globalThis.mypaper = mypaper;

// The defaults right away (no flash of unstyled attributes), then the host's look once known.
applyAppearance({ appearance: DEFAULT_APPEARANCE, themeCss: null }, undefined);
let widgetId: string | undefined;
const apply = (state: AppearanceState) => applyAppearance(state, widgetId);
mypaper.appearance.onChange(apply);
void mypaper.ready
  .then(() => {
    widgetId = mypaper.widget.id;
    return mypaper.appearance.get();
  })
  .then(apply, () => {});
