import { definePlugin, Role, ui, View } from '@mypaper/plugin-sdk';

let clicks = 0;
const instances = new Set<string>();
function draw(instance: string) {
  const face = View.stack([
    View.text(`Clicked ${clicks} times`, Role.Headline),
    View.button('Click', 'click'),
  ]).face();
  face.footer = 'TypeScript / QuickJS (experimental)';
  ui.setView(instance, face);
}

export const lifecycle = definePlugin({
  init(_info, state) {
    if (state?.length === 4) clicks = new DataView(state.buffer, state.byteOffset, 4).getUint32(0, true);
  },
  onEvent(event) {
    switch (event.tag) {
      case 'open':
      case 'resize':
        instances.add(event.val.instance);
        draw(event.val.instance);
        break;
      case 'close': instances.delete(event.val); break;
      case 'action':
        if (event.val.action === 'click') {
          clicks = (clicks + 1) >>> 0;
          for (const instance of instances) draw(instance);
        }
        // Headless budget probe only; this action has no button in the view.
        if (event.val.action === 'spin') { while (true) {} }
        break;
    }
  },
  save() {
    const state = new Uint8Array(4);
    new DataView(state.buffer).setUint32(0, clicks, true);
    return state;
  },
});
