import type { Error as WitError } from './generated/interfaces/mypaper-plugin-types.js';

/** jco's generated types use undefined for option::none. QuickJS returns null; normalize
 * that representation at the trusted WIT boundary. Preserve typed arrays and scalar values. */
export function fromWit<T>(value: T): T {
  function normalize(input: unknown): unknown {
    if (input === null) return undefined;
    if (Array.isArray(input)) return input.map(normalize);
    if (typeof input === 'object' && input !== null && !ArrayBuffer.isView(input)) {
      return Object.fromEntries(Object.entries(input).map(([key, child]) => [key, normalize(child)]));
    }
    return input;
  }
  // WIT has already validated the shape; this changes only its option representation.
  return normalize(value) as T;
}
/** Imported WIT results throw a ComponentError with a typed payload. Accept a direct payload
 * too, so callers need no casts and work with both component backends. */
export function capabilityError(error: unknown): WitError | undefined {
  const payload = typeof error === 'object' && error !== null && 'payload' in error ? error.payload : error;
  if (typeof payload !== 'object' || payload === null || !('tag' in payload) || !('val' in payload) || typeof payload.val !== 'string') return undefined;
  switch (payload.tag) {
    case 'denied': case 'unsupported': case 'invalid': case 'failed':
      return { tag: payload.tag, val: payload.val };
    default: return undefined;
  }
}
