import { test } from 'node:test';
import assert from 'node:assert/strict';
import { flatten, Role, Tone, View } from '../src/view.ts';

test('WIT pre-order traversal counts direct children across nested subtrees', () => {
  const face = View.stack([
    new View({ tag: 'card', val: { title: 'Group' } }, [View.row([
      View.tinted('Heading', Role.Headline, Tone.Accent), View.button('Click', 'click'),
    ])]),
    View.empty('cloud', 'Nothing here', 'Try again'),
  ]).face();
  face.footer = 'Ready';
  face.refresh = true;
  assert.deepEqual(flatten(face), {
    nodes: [
      { kind: { tag: 'stack', val: undefined }, children: 2 },
      { kind: { tag: 'card', val: { title: 'Group' } }, children: 1 },
      { kind: { tag: 'row', val: undefined }, children: 2 },
      { kind: { tag: 'text', val: { text: 'Heading', role: 'headline', color: { tag: 'accent' }, align: 'start' } }, children: 0 },
      { kind: { tag: 'button', val: { label: 'Click', action: 'click', icon: undefined, primary: false } }, children: 0 },
      { kind: { tag: 'empty', val: { icon: 'cloud', title: 'Nothing here', detail: 'Try again' } }, children: 0 },
    ], footer: 'Ready', refresh: true,
  });
  const plain = flatten(View.text('Body', Role.Body).face());
  assert.equal(plain.footer, undefined);
  assert.equal(plain.refresh, false);
  assert.deepEqual(plain.nodes[0], { kind: { tag: 'text', val: { text: 'Body', role: 'body', color: undefined, align: 'start' } }, children: 0 });
});
