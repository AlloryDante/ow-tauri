/**
 * OTJ ("ow-tauri JSON"): the tagged JSON encoding every IPC value travels in.
 *
 * Electron serialises IPC values with the structured clone algorithm; Tauri
 * IPC carries JSON. OTJ keeps the structured-clone types that JSON loses
 * (`undefined`, non-finite numbers, `-0`, `bigint`, `Date`, `RegExp`,
 * errors, binary data, `Map`, `Set`) by wrapping them in objects with a
 * `"$otj"` tag. The table is `docs/CONTRACT.md` section C.7; Rust treats OTJ
 * values as opaque JSON.
 *
 * Documented differences from structured clone: `undefined` object
 * properties are omitted, extra (non-index) array properties and symbol keys
 * are dropped, boxed primitives arrive unboxed, and shared or cyclic
 * references throw instead of being preserved.
 *
 * @packageDocumentation
 */
import { OwTauriError } from './errors.js';

/** A JSON value produced by {@link encode}. */
export type OtjValue = null | boolean | number | string | OtjValue[] | { [key: string]: OtjValue };

/** The tag key. */
export const TAG = '$otj';

/** Typed-array and buffer constructors OTJ round-trips, by tag name. */
const BYTE_TYPES: Readonly<Record<string, ((buffer: ArrayBuffer) => unknown) | undefined>> = {
  ArrayBuffer: (b) => b,
  DataView: (b) => new DataView(b),
  Int8Array: (b) => new Int8Array(b),
  Uint8Array: (b) => new Uint8Array(b),
  Uint8ClampedArray: (b) => new Uint8ClampedArray(b),
  Int16Array: (b) => new Int16Array(b),
  Uint16Array: (b) => new Uint16Array(b),
  Int32Array: (b) => new Int32Array(b),
  Uint32Array: (b) => new Uint32Array(b),
  Float32Array: (b) => new Float32Array(b),
  Float64Array: (b) => new Float64Array(b),
  BigInt64Array: (b) => new BigInt64Array(b),
  BigUint64Array: (b) => new BigUint64Array(b),
  // Buffer arrives as Uint8Array, as Electron delivers it to renderers.
  Buffer: (b) => new Uint8Array(b),
};

/** Error constructors structured clone restores by name. */
const ERROR_TYPES: Readonly<Record<string, ErrorConstructor | undefined>> = {
  Error,
  EvalError,
  RangeError,
  ReferenceError,
  SyntaxError,
  TypeError,
  URIError,
};

/**
 * Platform objects Electron's IPC refuses to clone (by `Symbol.toStringTag`).
 * Functions, symbols, DOM nodes and `Window` are checked separately.
 */
const REFUSED_TAGS: ReadonlySet<string> = new Set([
  'Promise',
  'WeakMap',
  'WeakSet',
  'WeakRef',
  'FinalizationRegistry',
  'Generator',
  'AsyncGenerator',
  'Window',
  'Blob',
  'File',
  'FileList',
  'MessagePort',
  'ReadableStream',
  'WritableStream',
  'TransformStream',
  'ImageBitmap',
  'OffscreenCanvas',
  'CryptoKey',
  'Request',
  'Response',
  'Headers',
  'AbortSignal',
]);

/** Options for {@link encode}. */
export interface EncodeOptions {
  /** Name of the root value in error messages; default `value`. */
  root?: string;
}

/**
 * Encodes a value to OTJ.
 *
 * @param value - any structured-clone-compatible value
 * @param options - error-message options
 * @returns the JSON-compatible encoding
 * @throws OwTauriError `ipc-serialization` for functions, symbols, DOM nodes,
 *   `Window`, promises, `WeakMap` / `WeakSet` and similar platform objects,
 *   and for cyclic or shared references; the message names the path of the
 *   offending value (for example `args[1].handler`)
 *
 * @example
 * ```ts
 * encode(new Map([[1, undefined]])); // { $otj: 'map', entries: [[1, { $otj: 'undefined' }]] }
 * ```
 */
export function encode(value: unknown, options?: EncodeOptions): OtjValue {
  return new Encoder().value(value, options?.root ?? 'value');
}

/**
 * Encodes an argument list: `undefined` arguments are kept as tags.
 *
 * @param args - the arguments
 * @param root - name of the list in error messages; default `args`
 * @returns one encoded value per argument
 * @throws OwTauriError `ipc-serialization`, as {@link encode}
 */
export function encodeArgs(args: readonly unknown[], root = 'args'): OtjValue[] {
  const encoder = new Encoder();
  return args.map((arg, i) => encoder.value(arg, `${root}[${String(i)}]`));
}

/**
 * Decodes an OTJ value.
 *
 * Unknown tags decode as plain objects (forward compatibility). Keys such as
 * `__proto__` become own data properties, never prototypes.
 *
 * @param value - a value produced by {@link encode} (after a JSON round trip)
 * @returns the decoded value
 */
export function decode(value: unknown): unknown {
  if (value === null || typeof value !== 'object') return value;
  if (Array.isArray(value)) return value.map(decode);
  const record = value as Record<string, unknown>;
  if (Object.hasOwn(record, TAG)) return decodeTagged(record);
  return decodePlain(record);
}

/**
 * Decodes an argument list.
 *
 * @param args - encoded arguments; a non-array decodes to `[]`
 * @returns the decoded arguments
 */
export function decodeArgs(args: unknown): unknown[] {
  return Array.isArray(args) ? args.map(decode) : [];
}

/**
 * The size in bytes of the UTF-8 JSON text of an encoded value.
 *
 * @param encoded - an OTJ value
 * @returns the byte length of `JSON.stringify(encoded)`
 */
export function encodedSize(encoded: unknown): number {
  const text = JSON.stringify(encoded) as string | undefined;
  if (text === undefined) return 0;
  return utf8Length(text);
}

function utf8Length(text: string): number {
  let bytes = 0;
  for (let i = 0; i < text.length; i++) {
    const unit = text.charCodeAt(i);
    if (unit < 0x80) bytes += 1;
    else if (unit < 0x800) bytes += 2;
    else if (unit >= 0xd800 && unit <= 0xdbff && i + 1 < text.length) {
      const next = text.charCodeAt(i + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        bytes += 4;
        i++;
      } else bytes += 3;
    } else bytes += 3;
  }
  return bytes;
}

class Encoder {
  /** Every object seen so far: OTJ rejects cyclic and shared references. */
  private readonly seen = new WeakSet();

  value(value: unknown, path: string): OtjValue {
    switch (typeof value) {
      case 'undefined':
        return { [TAG]: 'undefined' };
      case 'boolean':
      case 'string':
        return value;
      case 'number':
        return encodeNumber(value);
      case 'bigint':
        return { [TAG]: 'bigint', v: value.toString(10) };
      case 'symbol':
        return refuse(path, 'a symbol');
      case 'function':
        return refuse(path, 'a function');
      default:
        break;
    }
    if (value === null) return null;
    return this.object(value as object, path);
  }

  private object(value: object, path: string): OtjValue {
    if (isDomNode(value)) return refuse(path, 'a DOM node');
    const tag = Object.prototype.toString.call(value).slice(8, -1);
    if (REFUSED_TAGS.has(tag) || isWindow(value)) return refuse(path, `a ${tag} object`);
    if (this.seen.has(value)) return refuse(path, 'a cyclic or shared reference');
    this.seen.add(value);

    if (Array.isArray(value)) {
      const out: OtjValue[] = [];
      for (let i = 0; i < value.length; i++) {
        out.push(this.value(i in value ? value[i] : undefined, `${path}[${String(i)}]`));
      }
      return out;
    }
    switch (tag) {
      case 'Date': {
        const time = (value as Date).getTime();
        return { [TAG]: 'date', v: Number.isNaN(time) ? null : (value as Date).toISOString() };
      }
      case 'RegExp':
        return {
          [TAG]: 'regexp',
          source: (value as RegExp).source,
          flags: (value as RegExp).flags,
        };
      case 'Map': {
        const entries: OtjValue[] = [];
        let i = 0;
        for (const [k, v] of value as Map<unknown, unknown>) {
          const at = `${path}.entries[${String(i++)}]`;
          entries.push([this.value(k, `${at}[0]`), this.value(v, `${at}[1]`)]);
        }
        return { [TAG]: 'map', entries };
      }
      case 'Set': {
        const values: OtjValue[] = [];
        let i = 0;
        for (const v of value as Set<unknown>)
          values.push(this.value(v, `${path}.values[${String(i++)}]`));
        return { [TAG]: 'set', values };
      }
      case 'Number':
      case 'String':
      case 'Boolean':
      case 'BigInt':
        // Boxed primitives travel as their primitive value.
        return this.value((value as { valueOf(): unknown }).valueOf(), path);
      default:
        break;
    }
    if (value instanceof Error || tag === 'Error') return encodeError(value as Error);
    if (value instanceof ArrayBuffer || ArrayBuffer.isView(value)) return encodeBytes(value, tag);

    const props: Record<string, OtjValue> = {};
    let read: unknown;
    for (const key of Object.keys(value)) {
      try {
        read = (value as Record<string, unknown>)[key];
      } catch (cause) {
        throw new OwTauriError(
          'ipc-serialization',
          `${path}${keyPath(key)} threw while being read`,
          {
            cause,
            data: { path: `${path}${keyPath(key)}` },
          },
        );
      }
      if (read === undefined) continue;
      props[key] = this.value(read, `${path}${keyPath(key)}`);
    }
    return Object.hasOwn(value, TAG) ? { [TAG]: 'object', v: props } : props;
  }
}

function encodeNumber(value: number): OtjValue {
  if (Number.isNaN(value)) return { [TAG]: 'number', v: 'NaN' };
  if (value === Infinity) return { [TAG]: 'number', v: 'Infinity' };
  if (value === -Infinity) return { [TAG]: 'number', v: '-Infinity' };
  if (Object.is(value, -0)) return { [TAG]: 'number', v: '-0' };
  return value;
}

function encodeError(error: Error): OtjValue {
  const out: Record<string, OtjValue> = {
    [TAG]: 'error',
    name: typeof error.name === 'string' ? error.name : 'Error',
    message: typeof error.message === 'string' ? error.message : '',
  };
  if (typeof error.stack === 'string') out['stack'] = error.stack;
  return out;
}

function encodeBytes(value: ArrayBuffer | ArrayBufferView, tag: string): OtjValue {
  let type = tag;
  let bytes: Uint8Array;
  if (value instanceof ArrayBuffer) {
    type = 'ArrayBuffer';
    bytes = new Uint8Array(value);
  } else {
    if (value.constructor.name === 'Buffer') type = 'Buffer';
    bytes = new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }
  return { [TAG]: 'bytes', type, b64: toBase64(bytes) };
}

function keyPath(key: string): string {
  return /^[A-Za-z_$][\w$]*$/.test(key) ? `.${key}` : `[${JSON.stringify(key)}]`;
}

function refuse(path: string, what: string): never {
  throw new OwTauriError(
    'ipc-serialization',
    `${path} cannot be sent over ow-tauri IPC: it is ${what}`,
    { data: { path } },
  );
}

function isDomNode(value: object): boolean {
  return typeof Node === 'function' && value instanceof Node;
}

function isWindow(value: object): boolean {
  return typeof window === 'object' && value === window;
}

function decodeTagged(record: Record<string, unknown>): unknown {
  switch (record[TAG]) {
    case 'undefined':
      return undefined;
    case 'number':
      return decodeNumber(record['v']);
    case 'bigint':
      return typeof record['v'] === 'string' ? BigInt(record['v']) : undefined;
    case 'date':
      return new Date(typeof record['v'] === 'string' ? record['v'] : NaN);
    case 'regexp':
      return new RegExp(
        typeof record['source'] === 'string' ? record['source'] : '',
        typeof record['flags'] === 'string' ? record['flags'] : '',
      );
    case 'error':
      return decodeError(record);
    case 'bytes':
      return decodeBytes(record);
    case 'map': {
      const entries = Array.isArray(record['entries']) ? (record['entries'] as unknown[]) : [];
      const map = new Map<unknown, unknown>();
      for (const entry of entries) {
        if (Array.isArray(entry)) map.set(decode(entry[0]), decode(entry[1]));
      }
      return map;
    }
    case 'set': {
      const values = Array.isArray(record['values']) ? (record['values'] as unknown[]) : [];
      return new Set(values.map(decode));
    }
    case 'object': {
      const inner = record['v'];
      return typeof inner === 'object' && inner !== null && !Array.isArray(inner)
        ? decodePlain(inner as Record<string, unknown>)
        : {};
    }
    default:
      return decodePlain(record);
  }
}

function decodePlain(record: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const key of Object.keys(record)) {
    const v = decode(record[key]);
    if (key === '__proto__') {
      Object.defineProperty(out, key, {
        value: v,
        enumerable: true,
        writable: true,
        configurable: true,
      });
    } else {
      out[key] = v;
    }
  }
  return out;
}

function decodeNumber(v: unknown): number {
  switch (v) {
    case 'Infinity':
      return Infinity;
    case '-Infinity':
      return -Infinity;
    case '-0':
      return -0;
    default:
      return NaN;
  }
}

function decodeError(record: Record<string, unknown>): Error {
  const name = typeof record['name'] === 'string' ? record['name'] : 'Error';
  const message = typeof record['message'] === 'string' ? record['message'] : '';
  const Ctor = ERROR_TYPES[name] ?? Error;
  const error = new Ctor(message);
  if (error.name !== name) {
    Object.defineProperty(error, 'name', { value: name, writable: true, configurable: true });
  }
  if (typeof record['stack'] === 'string') {
    Object.defineProperty(error, 'stack', {
      value: record['stack'],
      writable: true,
      configurable: true,
    });
  }
  return error;
}

function decodeBytes(record: Record<string, unknown>): unknown {
  const bytes = fromBase64(typeof record['b64'] === 'string' ? record['b64'] : '');
  const make =
    BYTE_TYPES[typeof record['type'] === 'string' ? record['type'] : ''] ??
    BYTE_TYPES['Uint8Array'];
  const buffer: ArrayBuffer = new Uint8Array(bytes).buffer;
  try {
    return (make as (b: ArrayBuffer) => unknown)(buffer);
  } catch {
    // A length that does not fit the element size: deliver the raw bytes.
    return new Uint8Array(buffer);
  }
}

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
const B64_INDEX: Readonly<Record<string, number>> = Object.fromEntries(
  B64.split('').map((c, i) => [c, i]),
);

/**
 * Standard base64 (RFC 4648, with padding) without `btoa` or `Buffer`.
 *
 * @param bytes - the bytes
 * @returns the base64 text
 */
export function toBase64(bytes: Uint8Array): string {
  let out = '';
  let i = 0;
  for (; i + 2 < bytes.length; i += 3) {
    const n = ((bytes[i] ?? 0) << 16) | ((bytes[i + 1] ?? 0) << 8) | (bytes[i + 2] ?? 0);
    out +=
      B64.charAt(n >> 18) +
      B64.charAt((n >> 12) & 63) +
      B64.charAt((n >> 6) & 63) +
      B64.charAt(n & 63);
  }
  const rest = bytes.length - i;
  if (rest === 1) {
    const n = (bytes[i] ?? 0) << 16;
    out += `${B64.charAt(n >> 18)}${B64.charAt((n >> 12) & 63)}==`;
  } else if (rest === 2) {
    const n = ((bytes[i] ?? 0) << 16) | ((bytes[i + 1] ?? 0) << 8);
    out += `${B64.charAt(n >> 18)}${B64.charAt((n >> 12) & 63)}${B64.charAt((n >> 6) & 63)}=`;
  }
  return out;
}

/**
 * Decodes standard base64; characters outside the alphabet are skipped.
 *
 * @param text - base64 text
 * @returns the bytes
 */
export function fromBase64(text: string): Uint8Array {
  const clean = text.replace(/[^A-Za-z0-9+/]/g, '');
  const out = new Uint8Array(Math.floor((clean.length * 3) / 4));
  let o = 0;
  let acc = 0;
  let bits = 0;
  for (const c of clean) {
    acc = (acc << 6) | (B64_INDEX[c] ?? 0);
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      out[o++] = (acc >> bits) & 0xff;
      acc &= (1 << bits) - 1;
    }
  }
  return out.subarray(0, o);
}
