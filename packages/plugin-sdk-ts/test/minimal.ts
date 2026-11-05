import { setView } from 'mypaper:plugin/ui@0.1.0';
import type { lifecycle as Lifecycle } from '../src/generated/plugin.js';
let clicks = 0;
let instance = 'hello-1';
function draw() {
  setView(instance, {nodes: [
    {kind: {tag: 'stack', val: undefined}, children: 2},
    {kind: {tag: 'text', val: {text: `Clicked ${clicks} times`, role: 'headline', color: undefined, align: 'start'}}, children: 0},
    {kind: {tag: 'button', val: {label: 'Click', action: 'click', icon: undefined, primary: false}}, children: 0}
  ], footer: undefined, refresh: false});
}
export const lifecycle: typeof Lifecycle = {
  init(info) { if (info.state?.length === 4) clicks = new DataView(info.state.buffer, info.state.byteOffset, 4).getUint32(0, true); },
  onEvent(event) {
    if (event.tag === 'open' || event.tag === 'resize') { instance = event.val.instance; draw(); }
    if (event.tag === 'action') {
      if (event.val.action === 'click') { clicks++; draw(); }
      if (event.val.action === 'spin') { while (true) {} }
    }
  },
  save() { const bytes = new Uint8Array(4); new DataView(bytes.buffer).setUint32(0, clicks, true); return bytes; }
};

