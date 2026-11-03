// Types for the global `mypaper` that the drop-in script defines. Reference it from a widget's
// JS (`// @ts-check` plus `/// <reference types="@mypaper/widget-sdk/global" />`) or tsconfig `types`.
import type { Mypaper } from "../src/types";

declare global {
  // eslint-disable-next-line no-var
  var mypaper: Mypaper;
}
