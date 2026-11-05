import { test } from 'node:test';
import assert from 'node:assert/strict';
import { capabilityError, fromWit } from '../src/compat.ts';

test('option normalization preserves bytes and nested WIT result payloads', () => {
  const bytes = new Uint8Array([1, 255]);
  const normalized = fromWit({ tag: 'ok', val: { body: bytes, optional: null, tuple: [2n, null] } });
  assert.equal(normalized.val.body, bytes);
  assert.equal(normalized.val.optional, undefined);
  assert.deepEqual(normalized.val.tuple, [2n, undefined]);
});

test('capability errors narrow without trusting arbitrary thrown values', () => {
  const denied = { tag: 'denied', val: 'no permission' };
  assert.deepEqual(capabilityError({ payload: denied }), denied);
  assert.deepEqual(capabilityError(denied), denied);
  assert.equal(capabilityError(new Error('ordinary')), undefined);
  assert.equal(capabilityError({ payload: { tag: 'denied', val: 3 } }), undefined);
});
