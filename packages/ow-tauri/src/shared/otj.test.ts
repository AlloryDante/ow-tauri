import { describe, expect, it } from 'vitest';

import { OwTauriError } from './errors.js';
import {
  decode,
  decodeArgs,
  encode,
  encodeArgs,
  encodedSize,
  fromBase64,
  toBase64,
} from './otj.js';

/** Encode, go through JSON text as Tauri does, decode. */
function roundTrip(value: unknown): unknown {
  return decode(JSON.parse(JSON.stringify(encode(value))));
}

function serializationError(fn: () => unknown): OwTauriError {
  try {
    fn();
  } catch (error) {
    expect(error).toBeInstanceOf(OwTauriError);
    expect((error as OwTauriError).code).toBe('ipc-serialization');
    return error as OwTauriError;
  }
  throw new Error('expected an ipc-serialization error');
}

describe('OTJ: values JSON already carries (C.7 row 1)', () => {
  it.each([
    null,
    true,
    false,
    '',
    'text',
    0,
    1,
    -1,
    1.5,
    Number.MAX_SAFE_INTEGER,
    -Number.MAX_VALUE,
    Number.MIN_VALUE,
  ])('encodes %s as itself', (value) => {
    expect(encode(value)).toBe(value);
    expect(roundTrip(value)).toBe(value);
  });

  it('keeps unicode, including lone surrogates and U+2028', () => {
    const text = 'a\u2028b\u{1F600}\uD800';
    expect(roundTrip(text)).toBe(text);
  });
});

describe('OTJ: undefined', () => {
  it('tags undefined as an argument or array element', () => {
    expect(encode(undefined)).toEqual({ $otj: 'undefined' });
    expect(encodeArgs([undefined, 1])).toEqual([{ $otj: 'undefined' }, 1]);
    expect(roundTrip([1, undefined, 3])).toEqual([1, undefined, 3]);
    expect(decodeArgs(JSON.parse(JSON.stringify(encodeArgs([undefined]))))).toEqual([undefined]);
  });

  it('turns array holes into undefined elements', () => {
    // eslint-disable-next-line no-sparse-arrays -- the hole is the point
    const decoded = roundTrip([1, , 3]) as unknown[];
    expect(decoded).toHaveLength(3);
    expect(1 in decoded).toBe(true);
    expect(decoded[1]).toBeUndefined();
  });

  it('omits undefined object properties (documented difference from structured clone)', () => {
    expect(encode({ a: undefined, b: 1 })).toEqual({ b: 1 });
    expect(Object.keys(roundTrip({ a: undefined }) as object)).toEqual([]);
  });
});

describe('OTJ: numbers JSON cannot carry', () => {
  it.each([
    [NaN, 'NaN'],
    [Infinity, 'Infinity'],
    [-Infinity, '-Infinity'],
    [-0, '-0'],
  ])('tags %s', (value, v) => {
    expect(encode(value)).toEqual({ $otj: 'number', v });
    expect(Object.is(roundTrip(value), value)).toBe(true);
  });

  it('decodes an unknown number tag as NaN', () => {
    expect(decode({ $otj: 'number', v: 'weird' })).toBeNaN();
  });
});

describe('OTJ: bigint, Date, RegExp', () => {
  it('round-trips bigint as a decimal string', () => {
    const big = 2n ** 80n + 1n;
    expect(encode(big)).toEqual({ $otj: 'bigint', v: big.toString() });
    expect(roundTrip(big)).toBe(big);
    expect(roundTrip(-5n)).toBe(-5n);
    expect(decode({ $otj: 'bigint', v: 5 })).toBeUndefined();
  });

  it('round-trips dates, including invalid dates', () => {
    const date = new Date('2026-10-06T12:34:56.789Z');
    expect(encode(date)).toEqual({ $otj: 'date', v: '2026-10-06T12:34:56.789Z' });
    const decoded = roundTrip(date) as Date;
    expect(decoded).toBeInstanceOf(Date);
    expect(decoded.getTime()).toBe(date.getTime());
    expect(encode(new Date(NaN))).toEqual({ $otj: 'date', v: null });
    expect((roundTrip(new Date(NaN)) as Date).getTime()).toBeNaN();
  });

  it('round-trips regular expressions with flags (lastIndex is not kept, like structured clone)', () => {
    const re = /a+b/giu;
    re.lastIndex = 3;
    const decoded = roundTrip(re) as RegExp;
    expect(decoded).toBeInstanceOf(RegExp);
    expect(decoded.source).toBe('a+b');
    expect(decoded.flags).toBe('giu');
    expect(decoded.lastIndex).toBe(0);
  });
});

describe('OTJ: errors', () => {
  it('keeps name, message and stack', () => {
    const error = new Error('boom');
    const encoded = encode(error) as Record<string, unknown>;
    expect(encoded['$otj']).toBe('error');
    expect(encoded['name']).toBe('Error');
    expect(encoded['message']).toBe('boom');
    expect(typeof encoded['stack']).toBe('string');
    const decoded = roundTrip(error) as Error;
    expect(decoded).toBeInstanceOf(Error);
    expect(decoded.message).toBe('boom');
    expect(decoded.stack).toBe(error.stack);
  });

  it.each([EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError])(
    'restores the standard constructor %o by name, as structured clone does',
    (Ctor) => {
      const decoded = roundTrip(new Ctor('x')) as Error;
      expect(decoded).toBeInstanceOf(Ctor);
      expect(decoded.name).toBe(Ctor.name);
    },
  );

  it('decodes custom subclasses as Error with the name set', () => {
    class ValidationError extends Error {
      override name = 'ValidationError';
    }
    const decoded = roundTrip(new ValidationError('bad')) as Error;
    expect(decoded.constructor).toBe(Error);
    expect(decoded.name).toBe('ValidationError');
    expect(decoded.message).toBe('bad');
  });

  it('tolerates malformed error tags', () => {
    const decoded = decode({ $otj: 'error', name: 5, message: null }) as Error;
    expect(decoded).toBeInstanceOf(Error);
    expect(decoded.message).toBe('');
    expect(decoded.name).toBe('Error');
  });

  it('encodes an error without a stack', () => {
    const error = new Error('no stack');
    Object.defineProperty(error, 'stack', { value: undefined });
    expect(encode(error)).toEqual({ $otj: 'error', name: 'Error', message: 'no stack' });
  });
});

describe('OTJ: binary data', () => {
  const ctors = [
    Int8Array,
    Uint8Array,
    Uint8ClampedArray,
    Int16Array,
    Uint16Array,
    Int32Array,
    Uint32Array,
    Float32Array,
    Float64Array,
  ] as const;

  it.each(ctors)('round-trips %o', (Ctor) => {
    const value = new Ctor([1, 2, 3, 250]);
    const decoded = roundTrip(value) as ArrayLike<number>;
    expect(decoded).toBeInstanceOf(Ctor);
    expect([...(decoded as unknown as Iterable<number>)]).toEqual([...value]);
  });

  it('round-trips bigint arrays', () => {
    const value = new BigInt64Array([1n, -2n]);
    expect(roundTrip(value)).toEqual(value);
    expect(roundTrip(new BigUint64Array([3n]))).toEqual(new BigUint64Array([3n]));
  });

  it('round-trips ArrayBuffer and DataView', () => {
    const buffer = new Uint8Array([9, 8, 7]).buffer;
    const decodedBuffer = roundTrip(buffer) as ArrayBuffer;
    expect(decodedBuffer).toBeInstanceOf(ArrayBuffer);
    expect([...new Uint8Array(decodedBuffer)]).toEqual([9, 8, 7]);
    const view = new DataView(new Uint8Array([1, 2, 3, 4]).buffer, 1, 2);
    const decodedView = roundTrip(view) as DataView;
    expect(decodedView).toBeInstanceOf(DataView);
    expect(decodedView.byteLength).toBe(2);
    expect(decodedView.getUint8(0)).toBe(2);
  });

  it('encodes only the viewed region of a typed array', () => {
    const view = new Uint8Array([0, 1, 2, 3, 4]).subarray(1, 3);
    expect(encode(view)).toEqual({
      $otj: 'bytes',
      type: 'Uint8Array',
      b64: toBase64(new Uint8Array([1, 2])),
    });
  });

  it('decodes a Node Buffer as Uint8Array, as Electron delivers it to renderers', () => {
    class Buffer extends Uint8Array {}
    const encoded = encode(Buffer.from([1, 2])) as Record<string, unknown>;
    expect(encoded['type']).toBe('Buffer');
    const decoded = roundTrip(Buffer.from([1, 2]));
    expect(decoded).toBeInstanceOf(Uint8Array);
    expect(decoded).not.toBeInstanceOf(Buffer);
  });

  it('falls back to Uint8Array for unknown types and misaligned lengths', () => {
    expect(decode({ $otj: 'bytes', type: 'Mystery', b64: 'AQI=' })).toEqual(new Uint8Array([1, 2]));
    expect(decode({ $otj: 'bytes', type: 'Int32Array', b64: 'AQI=' })).toEqual(
      new Uint8Array([1, 2]),
    );
    expect(decode({ $otj: 'bytes' })).toEqual(new Uint8Array([]));
  });

  it('implements RFC 4648 base64', () => {
    const enc = new TextEncoder();
    const vectors: [string, string][] = [
      ['', ''],
      ['f', 'Zg=='],
      ['fo', 'Zm8='],
      ['foo', 'Zm9v'],
      ['foob', 'Zm9vYg=='],
      ['fooba', 'Zm9vYmE='],
      ['foobar', 'Zm9vYmFy'],
    ];
    for (const [plain, b64] of vectors) {
      expect(toBase64(enc.encode(plain))).toBe(b64);
      expect(new TextDecoder().decode(fromBase64(b64))).toBe(plain);
    }
    const all = new Uint8Array(256).map((_, i) => i);
    expect(fromBase64(toBase64(all))).toEqual(all);
    expect(fromBase64('Zm9v\nYmFy')).toEqual(enc.encode('foobar'));
  });
});

describe('OTJ: Map, Set, objects, arrays', () => {
  it('round-trips Map with non-string keys and nested values', () => {
    const map = new Map<unknown, unknown>([
      [1, 'one'],
      ['u', undefined],
      [{ k: 1 }, new Set([1n])],
    ]);
    const decoded = roundTrip(map) as Map<unknown, unknown>;
    expect(decoded).toBeInstanceOf(Map);
    expect([...decoded.entries()]).toEqual([
      [1, 'one'],
      ['u', undefined],
      [{ k: 1 }, new Set([1n])],
    ]);
  });

  it('round-trips Set', () => {
    const decoded = roundTrip(new Set([1, 'a', undefined])) as Set<unknown>;
    expect(decoded).toBeInstanceOf(Set);
    expect([...decoded]).toEqual([1, 'a', undefined]);
  });

  it('copies own enumerable string keys of class instances into a plain object', () => {
    class Point {
      constructor(
        public x: number,
        public y: number,
      ) {}
      get length(): number {
        return Math.hypot(this.x, this.y);
      }
    }
    const decoded = roundTrip(new Point(3, 4));
    expect(Object.getPrototypeOf(decoded)).toBe(Object.prototype);
    expect(decoded).toEqual({ x: 3, y: 4 });
  });

  it('drops symbol keys and non-enumerable properties', () => {
    const value = Object.defineProperty({ [Symbol('s')]: 1, a: 1 }, 'hidden', {
      value: 2,
      enumerable: false,
    });
    expect(roundTrip(value)).toEqual({ a: 1 });
  });

  it('escapes objects that have an own "$otj" key', () => {
    const value = { $otj: 'date', v: 'not a date', nested: { $otj: 'undefined' } };
    expect(encode(value)).toEqual({
      $otj: 'object',
      v: { $otj: 'date', v: 'not a date', nested: { $otj: 'object', v: { $otj: 'undefined' } } },
    });
    expect(roundTrip(value)).toEqual(value);
  });

  it('decodes __proto__ keys as own data properties, never as prototypes', () => {
    const decoded = decode(JSON.parse('{"__proto__": {"polluted": true}, "a": 1}')) as Record<
      string,
      unknown
    >;
    expect(Object.getPrototypeOf(decoded)).toBe(Object.prototype);
    expect(Object.hasOwn(decoded, '__proto__')).toBe(true);
    expect(({} as Record<string, unknown>)['polluted']).toBeUndefined();
  });

  it('unboxes boxed primitives (documented difference)', () => {
    expect(encode(new Number(5))).toBe(5);
    expect(encode(new String('s'))).toBe('s');
    expect(encode(new Boolean(false))).toBe(false);
    expect(encode(Object(3n))).toEqual({ $otj: 'bigint', v: '3' });
  });

  it('decodes unknown tags as plain objects and non-object payloads safely', () => {
    expect(decode({ $otj: 'future', a: 1 })).toEqual({ $otj: 'future', a: 1 });
    expect(decode({ $otj: 'map' })).toEqual(new Map());
    expect(decode({ $otj: 'map', entries: [5, [1, 2]] })).toEqual(new Map([[1, 2]]));
    expect(decode({ $otj: 'set' })).toEqual(new Set());
    expect(decode({ $otj: 'object', v: 3 })).toEqual({});
    expect(decode({ $otj: 'regexp' })).toEqual(/(?:)/);
    expect(decode({ $otj: 'date', v: 7 })).toEqual(new Date(NaN));
    expect(decodeArgs('nope')).toEqual([]);
  });
});

describe('OTJ: values that throw ipc-serialization (C.7)', () => {
  it.each([
    ['a function', () => 1],
    ['a symbol', Symbol('s')],
    ['a Promise object', Promise.resolve()],
    ['a WeakMap object', new WeakMap()],
    ['a WeakSet object', new WeakSet()],
    ['a WeakRef object', new WeakRef({})],
  ])('refuses %s', (what, value) => {
    const error = serializationError(() => encodeArgs([1, { handler: value }]));
    expect(error.message).toBe(`args[1].handler cannot be sent over ow-tauri IPC: it is ${what}`);
    expect(error.data).toEqual({ path: 'args[1].handler' });
  });

  it('refuses DOM nodes and the window', () => {
    expect(serializationError(() => encode(document.createElement('div'))).message).toContain(
      'a DOM node',
    );
    expect(serializationError(() => encode({ w: window })).message).toContain('value.w');
  });

  it('refuses cycles and shared references, naming the path', () => {
    const cyclic: Record<string, unknown> = {};
    cyclic['self'] = cyclic;
    expect(serializationError(() => encode(cyclic)).message).toContain('value.self');
    const shared = { a: 1 };
    expect(serializationError(() => encodeArgs([shared, shared])).message).toContain('args[1]');
    expect(serializationError(() => encode([[shared], { 'odd key': shared }])).message).toContain(
      'value[1]["odd key"]',
    );
  });

  it('names Map and Set paths', () => {
    expect(serializationError(() => encode(new Map([[1, () => 1]]))).message).toContain(
      'value.entries[0][1]',
    );
    expect(serializationError(() => encode(new Set([Symbol('x')]))).message).toContain(
      'value.values[0]',
    );
  });

  it('reports a getter that throws', () => {
    const value = {
      get bad(): never {
        throw new Error('nope');
      },
    };
    const error = serializationError(() => encode(value, { root: 'result' }));
    expect(error.message).toBe('result.bad threw while being read');
    expect(error.cause).toBeInstanceOf(Error);
  });
});

describe('encodedSize', () => {
  it('counts UTF-8 bytes of the JSON text', () => {
    expect(encodedSize('a')).toBe(3); // "a"
    expect(encodedSize('é')).toBe(4);
    expect(encodedSize('€')).toBe(5);
    expect(encodedSize('\u{1F600}')).toBe(6);
    expect(encodedSize('\uD800x')).toBe(2 + 6 + 1); // lone surrogate escaped by JSON.stringify
    expect(encodedSize(undefined)).toBe(0);
  });
});
