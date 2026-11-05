// Runs only in the throwaway headless runtime; storage is under the probe's scratch folder.
import { capabilityError, definePlugin, http, Role, storage, timers, Tone, ui, View } from '../src/index.js';
let clicks = 0;
function draw(instance: string) { ui.setView(instance, View.text(`${clicks}`, Role.Body).face()); }
export const lifecycle = definePlugin({
  init(info, state) {
    if (state) clicks = state[0] ?? 0;
    if (info.state === null) throw new Error('init state uses null rather than undefined');
    if (storage.get('missing') !== undefined) throw new Error('storage option uses null rather than undefined');
    storage.set('bytes', new Uint8Array([1, 2, 255]));
    const bytes = storage.get('bytes');
    if (!(bytes instanceof Uint8Array) || bytes[2] !== 255) throw new Error('list<u8> is not a Uint8Array');
    let denied = false;
    try { http.fetch({ method: 'get', url: 'https://example.com/', headers: [] }); }
    catch (error) { denied = capabilityError(error)?.tag === 'denied'; }
    if (!denied) throw new Error('WIT result errors must throw their typed variant');
    try { timers.every('too-fast', 1); throw new Error('timer budget was bypassed'); }
    catch (error) { if (capabilityError(error)?.tag !== 'invalid') throw error; }
  },
  onEvent(event) {
    if (event.tag === 'open') {
      ui.setView(event.val.instance, View.stack([
        new View({ tag: 'grid', val: { columns: 2, gap: 4 } }, [
          new View({ tag: 'sparkline', val: { values: new Float32Array([0.1, 0.5, 1]), color: Tone.Accent } }),
          new View({ tag: 'segmented-bar', val: [{ fraction: 0.5, color: Tone.Positive }] }),
        ]),
        new View({ tag: 'card', val: { title: 'Metrics' } }, [new View({
          tag: 'area-chart', val: { series: [{ label: 'Test', values: new Float32Array([1, 2, 3]) }], xLabels: ['A', 'B', 'C'] },
        })]),
      ]).face());
    }
    if (event.tag === 'action') {
      if (event.val.value !== undefined) throw new Error('action option uses null rather than undefined');
      if (event.val.action === 'click') { clicks++; draw(event.val.instance); }
      if (event.val.action === 'spin') { while (true) {} }
    }
  },
  save() { return new Uint8Array([clicks & 255]); },
});
