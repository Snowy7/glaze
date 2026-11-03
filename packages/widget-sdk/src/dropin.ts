// Entry of the drop-in build (`dist/mypaper.js`): `<script src="mypaper.js"></script>` defines
// the global `mypaper`, for widgets with no build step.
/// <reference path="../types/global.d.ts" />
import { connect } from "./index";

globalThis.mypaper = connect();
