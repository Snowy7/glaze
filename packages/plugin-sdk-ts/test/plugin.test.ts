import { test } from 'node:test';
import assert from 'node:assert/strict';
import { definePlugin, type Event } from '../src/plugin.ts';

test('typed handlers restore state, receive events, and provide saved bytes', () => {
  let counter = 0;
  const plugin = definePlugin({
    init(_info, state) { counter = state?.[0] ?? 0; },
    onEvent(event) { if (event.tag === 'action') counter++; },
    save() { return new Uint8Array([counter]); },
  });
  plugin.init({ plugin: 'test', settings: '{}', state: new Uint8Array([41]) });
  plugin.onEvent({ tag: 'action', val: { instance: 'one', action: 'click' } });
  assert.deepEqual(plugin.save(), new Uint8Array([42]));
  const events: Event[] = [];
  const defaults = definePlugin({ onEvent(event) { events.push(event); } });
  defaults.init({ plugin: 'test', settings: '{}' });
  defaults.onEvent({ tag: 'resume' });
  assert.deepEqual(events, [{ tag: 'resume' }]);
  assert.deepEqual(defaults.save(), new Uint8Array());
});

test('init errors cross the WIT string-error boundary', () => {
  const plugin = definePlugin({
    init() { throw new Error('bad settings'); },
    onEvent() {},
  });
  assert.throws(() => plugin.init({ plugin: 'test', settings: '{}' }), error => error === 'bad settings');
});

// An async export would hold a synchronous runtime call open and bypass event-driven I/O.
definePlugin({
  // @ts-expect-error Promise-returning handlers are forbidden.
  async onEvent() {},
});
