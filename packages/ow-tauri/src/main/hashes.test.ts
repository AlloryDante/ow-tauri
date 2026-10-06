/// <reference types="node" />
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { emailHashes, md5, normalizeEmail, sha1, sha256 } from './hashes.js';

interface Vector {
  email: string;
  normalized: string | null;
  hex: { sha1?: string; sha256?: string; md5?: string } | null;
}

const fixture = JSON.parse(
  readFileSync(
    join(
      dirname(fileURLToPath(import.meta.url)),
      '../../../../crates/tauri-plugin-overwolf/tests/fixtures/email-hashes.json',
    ),
    'utf8',
  ),
) as { vectors: Vector[] };

const hex = (bytes: Uint8Array): string => Buffer.from(bytes).toString('hex');
const utf8 = (text: string): Uint8Array => new TextEncoder().encode(text);

describe('digests', () => {
  it('match the published test vectors', () => {
    expect(hex(md5(utf8('')))).toBe('d41d8cd98f00b204e9800998ecf8427e');
    expect(hex(md5(utf8('abc')))).toBe('900150983cd24fb0d6963f7d28e17f72');
    expect(hex(sha1(utf8('abc')))).toBe('a9993e364706816aba3e25717850c26c9cd0d89d');
    expect(hex(sha256(utf8('abc')))).toBe(
      'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
    );
    const block = utf8('abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq');
    expect(hex(sha1(block))).toBe('84983e441c3bd26ebaae4aa1f95129e5e54670f1');
    expect(hex(sha256(block))).toBe(
      '248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1',
    );
  });

  it('handle messages around the padding boundaries', () => {
    for (const length of [55, 56, 63, 64, 65, 119, 120, 1000]) {
      const data = new Uint8Array(length).fill(0x61);
      const node = (algorithm: string): string => createHash(algorithm).update(data).digest('hex');
      expect(hex(md5(data))).toBe(node('md5'));
      expect(hex(sha1(data))).toBe(node('sha1'));
      expect(hex(sha256(data))).toBe(node('sha256'));
    }
  });
});

describe('generateUserEmailHashes (CONTRACT A.2.2)', () => {
  it('matches the shared vectors used by the Rust plugin', () => {
    expect(fixture.vectors.length).toBeGreaterThan(3);
    for (const vector of fixture.vectors) {
      expect(normalizeEmail(vector.email) ?? null).toBe(vector.normalized);
      const hashes = emailHashes(vector.email);
      if (vector.hex === null || vector.normalized === null) expect(hashes).toEqual({});
      else expect(hashes).toEqual(vector.hex);
    }
  });

  it('gives the values ow-electron gives, keys in its order [OBS]', () => {
    const expected = {
      sha1: '2c44f8a418bbfa88e80e3ce17d56cb30944f7675',
      md5: '170d78feecf2b8e7b804ba6b45af7ac2',
      sha256: 'ac43b559f15c2eb262ea8d5d4921f639aaf1cde84bc280bad2e1879d0ded68c2',
    };
    const hashes = emailHashes('test.email@overwolf.com');
    expect(hashes).toEqual(expected);
    expect(Object.keys(hashes)).toEqual(['sha1', 'md5', 'sha256']);
    expect(emailHashes('  Test.Email@Overwolf.COM  ')).toEqual(expected);
  });

  it('hashes input that is not an email address and returns {} for blank input', () => {
    expect(emailHashes('not an email').md5).toBe(hex(md5(utf8('not an email'))));
    expect(emailHashes('   ')).toEqual({});
    expect(normalizeEmail('A.B+c@GMail.com')).toBe('ab@gmail.com');
    expect(normalizeEmail('a+b@gmail.com.example')).toBe('a+b@gmail.com.example');
    expect(normalizeEmail('Ünïcode@Example.com')).toBe('ünïcode@example.com');
  });
});
