/**
 * Synchronous email hashing for `app.overwolf.generateUserEmailHashes()`
 * (`docs/CONTRACT.md` sections A.2.2 and B.1.1).
 *
 * ow-electron returns the hashes synchronously, so they cannot come from the
 * plugin: this module implements MD5, SHA-1 and SHA-256 in plain TypeScript
 * over the UTF-8 bytes of the normalised address. The Rust plugin implements
 * the same function (`identity::email_hashes`); both are checked against the
 * shared vectors in
 * `crates/tauri-plugin-overwolf/tests/fixtures/email-hashes.json`.
 *
 * @packageDocumentation
 */

/** Hex digits, indexed by nibble. */
const HEX = '0123456789abcdef';

/**
 * Lower-case hex of a byte array.
 *
 * @param bytes - the bytes
 * @returns the hex string
 */
function toHex(bytes: Uint8Array): string {
  let out = '';
  for (const byte of bytes) out += (HEX[byte >>> 4] ?? '') + (HEX[byte & 15] ?? '');
  return out;
}

/**
 * Pads a message the way MD5 and the SHA family do: a `0x80` byte, zeros to
 * 56 mod 64, then the bit length as a 64-bit integer.
 *
 * @param data - the message
 * @param littleEndian - MD5 stores the length little-endian, SHA big-endian
 * @returns the padded message, a multiple of 64 bytes long
 */
function pad(data: Uint8Array, littleEndian: boolean): Uint8Array {
  const length = data.length;
  const total = (((length + 8) >>> 6) + 1) << 6;
  const out = new Uint8Array(total);
  out.set(data);
  out[length] = 0x80;
  const view = new DataView(out.buffer);
  const bits = length * 8;
  const high = Math.floor(bits / 0x100000000);
  const low = bits >>> 0;
  if (littleEndian) {
    view.setUint32(total - 8, low, true);
    view.setUint32(total - 4, high, true);
  } else {
    view.setUint32(total - 8, high, false);
    view.setUint32(total - 4, low, false);
  }
  return out;
}

function rotl(x: number, n: number): number {
  return (x << n) | (x >>> (32 - n));
}

function rotr(x: number, n: number): number {
  return (x >>> n) | (x << (32 - n));
}

/** MD5 per-round shift amounts (RFC 1321). */
const MD5_S = [
  7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14,
  20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6,
  10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/** MD5 constants: `floor(abs(sin(i + 1)) * 2^32)` (RFC 1321). */
const MD5_K = Array.from({ length: 64 }, (_, i) => Math.floor(Math.abs(Math.sin(i + 1)) * 2 ** 32));

/**
 * MD5 (RFC 1321).
 *
 * @param data - the message
 * @returns the 16-byte digest
 */
export function md5(data: Uint8Array): Uint8Array {
  const message = pad(data, true);
  const view = new DataView(message.buffer);
  let a0 = 0x67452301;
  let b0 = 0xefcdab89;
  let c0 = 0x98badcfe;
  let d0 = 0x10325476;
  const m = new Array<number>(16);
  for (let offset = 0; offset < message.length; offset += 64) {
    for (let i = 0; i < 16; i++) m[i] = view.getUint32(offset + i * 4, true);
    let a = a0;
    let b = b0;
    let c = c0;
    let d = d0;
    for (let i = 0; i < 64; i++) {
      let f: number;
      let g: number;
      if (i < 16) {
        f = (b & c) | (~b & d);
        g = i;
      } else if (i < 32) {
        f = (d & b) | (~d & c);
        g = (5 * i + 1) % 16;
      } else if (i < 48) {
        f = b ^ c ^ d;
        g = (3 * i + 5) % 16;
      } else {
        f = c ^ (b | ~d);
        g = (7 * i) % 16;
      }
      const sum = (f + a + (MD5_K[i] ?? 0) + (m[g] ?? 0)) | 0;
      a = d;
      d = c;
      c = b;
      b = (b + rotl(sum, MD5_S[i] ?? 0)) | 0;
    }
    a0 = (a0 + a) | 0;
    b0 = (b0 + b) | 0;
    c0 = (c0 + c) | 0;
    d0 = (d0 + d) | 0;
  }
  const out = new Uint8Array(16);
  const outView = new DataView(out.buffer);
  [a0, b0, c0, d0].forEach((word, i) => {
    outView.setUint32(i * 4, word >>> 0, true);
  });
  return out;
}

/**
 * SHA-1 (FIPS 180-4).
 *
 * @param data - the message
 * @returns the 20-byte digest
 */
export function sha1(data: Uint8Array): Uint8Array {
  const message = pad(data, false);
  const view = new DataView(message.buffer);
  const h = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];
  const w = new Array<number>(80);
  for (let offset = 0; offset < message.length; offset += 64) {
    for (let i = 0; i < 16; i++) w[i] = view.getUint32(offset + i * 4, false);
    for (let i = 16; i < 80; i++)
      w[i] = rotl((w[i - 3] ?? 0) ^ (w[i - 8] ?? 0) ^ (w[i - 14] ?? 0) ^ (w[i - 16] ?? 0), 1);
    let [a, b, c, d, e] = h as [number, number, number, number, number];
    for (let i = 0; i < 80; i++) {
      let f: number;
      let k: number;
      if (i < 20) {
        f = (b & c) | (~b & d);
        k = 0x5a827999;
      } else if (i < 40) {
        f = b ^ c ^ d;
        k = 0x6ed9eba1;
      } else if (i < 60) {
        f = (b & c) | (b & d) | (c & d);
        k = 0x8f1bbcdc;
      } else {
        f = b ^ c ^ d;
        k = 0xca62c1d6;
      }
      const temp = (rotl(a, 5) + f + e + k + (w[i] ?? 0)) | 0;
      e = d;
      d = c;
      c = rotl(b, 30);
      b = a;
      a = temp;
    }
    h[0] = ((h[0] ?? 0) + a) | 0;
    h[1] = ((h[1] ?? 0) + b) | 0;
    h[2] = ((h[2] ?? 0) + c) | 0;
    h[3] = ((h[3] ?? 0) + d) | 0;
    h[4] = ((h[4] ?? 0) + e) | 0;
  }
  return wordsBigEndian(h);
}

/** SHA-256 round constants (FIPS 180-4, section 4.2.2). */
const SHA256_K = [
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/**
 * SHA-256 (FIPS 180-4).
 *
 * @param data - the message
 * @returns the 32-byte digest
 */
export function sha256(data: Uint8Array): Uint8Array {
  const message = pad(data, false);
  const view = new DataView(message.buffer);
  const h = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
  ];
  const w = new Array<number>(64);
  for (let offset = 0; offset < message.length; offset += 64) {
    for (let i = 0; i < 16; i++) w[i] = view.getUint32(offset + i * 4, false);
    for (let i = 16; i < 64; i++) {
      const w15 = w[i - 15] ?? 0;
      const w2 = w[i - 2] ?? 0;
      const s0 = rotr(w15, 7) ^ rotr(w15, 18) ^ (w15 >>> 3);
      const s1 = rotr(w2, 17) ^ rotr(w2, 19) ^ (w2 >>> 10);
      w[i] = ((w[i - 16] ?? 0) + s0 + (w[i - 7] ?? 0) + s1) | 0;
    }
    let [a, b, c, d, e, f, g, hh] = h as [
      number,
      number,
      number,
      number,
      number,
      number,
      number,
      number,
    ];
    for (let i = 0; i < 64; i++) {
      const s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const ch = (e & f) ^ (~e & g);
      const t1 = (hh + s1 + ch + (SHA256_K[i] ?? 0) + (w[i] ?? 0)) | 0;
      const s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const t2 = (s0 + maj) | 0;
      hh = g;
      g = f;
      f = e;
      e = (d + t1) | 0;
      d = c;
      c = b;
      b = a;
      a = (t1 + t2) | 0;
    }
    const next = [a, b, c, d, e, f, g, hh];
    for (let i = 0; i < 8; i++) h[i] = ((h[i] ?? 0) + (next[i] ?? 0)) | 0;
  }
  return wordsBigEndian(h);
}

function wordsBigEndian(words: readonly number[]): Uint8Array {
  const out = new Uint8Array(words.length * 4);
  const view = new DataView(out.buffer);
  words.forEach((word, i) => {
    view.setUint32(i * 4, word >>> 0, false);
  });
  return out;
}

/**
 * Normalises an email address before hashing (CONTRACT A.2.2): trims and
 * lower-cases it [OBS]; for `gmail.com` addresses also removes `.` and any
 * `+suffix` from the local part (the UID2 rule the typings link to) [DEC].
 * An input that is not an email address is kept as it is [OBS].
 *
 * @param email - the address the app passed
 * @returns the normalised address, or `undefined` for empty or whitespace input
 *
 * @example
 * ```ts
 * normalizeEmail('  Jane.Doe+ads@GMAIL.com '); // 'janedoe@gmail.com'
 * normalizeEmail('   '); // undefined
 * ```
 */
export function normalizeEmail(email: string): string | undefined {
  const lower = email.trim().toLowerCase();
  if (lower === '') return undefined;
  const at = lower.lastIndexOf('@');
  if (at !== -1 && lower.slice(at + 1) === 'gmail.com') {
    const plus = lower.indexOf('+');
    const local = lower.slice(0, plus !== -1 && plus < at ? plus : at);
    return `${local.replaceAll('.', '')}@gmail.com`;
  }
  return lower;
}

/** The hashes `generateUserEmailHashes()` returns, keys in ow-electron's order. */
export interface UserEmailHashes {
  /** SHA-1, lower-case hex. */
  readonly sha1?: string;
  /** MD5, lower-case hex. */
  readonly md5?: string;
  /** SHA-256, lower-case hex. */
  readonly sha256?: string;
}

/**
 * Hashes an email address the way ow-electron's
 * `generateUserEmailHashes()` does: lower-case hex digests of the normalised
 * address, keys in the order `sha1`, `md5`, `sha256` [OBS].
 *
 * @param email - the address
 * @returns the hashes, or `{}` for empty or whitespace input [DEC]
 *
 * @example
 * ```ts
 * emailHashes('test.email@overwolf.com').md5; // '170d78feecf2b8e7b804ba6b45af7ac2'
 * ```
 */
export function emailHashes(email: string): UserEmailHashes {
  const normalized = normalizeEmail(email);
  if (normalized === undefined) return {};
  const bytes = new TextEncoder().encode(normalized);
  return {
    sha1: toHex(sha1(bytes)),
    md5: toHex(md5(bytes)),
    sha256: toHex(sha256(bytes)),
  };
}
