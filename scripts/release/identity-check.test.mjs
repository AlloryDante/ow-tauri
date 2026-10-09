// node --test scripts/release/*.test.mjs
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, it } from 'node:test';

import { DENIED, hashTerm, scanFiles, scanText } from './identity-check.mjs';

// Made-up terms; the real list is only stored as hashes.
const TEST_DENIED = [hashTerm('zebracorn'), hashTerm('quux labs'), hashTerm('a b c')];

describe('hashTerm', () => {
  it('normalises case and separators', () => {
    assert.equal(hashTerm('Quux  Labs'), hashTerm('quux-labs'));
    assert.equal(hashTerm('ZEBRACORN'), TEST_DENIED[0]);
  });
});

describe('scanText', () => {
  it('finds a term in any case, inside identifiers and paths', () => {
    const text = ['ok', 'by ZebraCorn', 'path /home/zebracorn/x', 'zebracorns are fine'].join('\n');
    const hits = scanText(text, TEST_DENIED);
    assert.deepEqual(
      hits.map((h) => h.line),
      [2, 3],
    );
  });
  it('finds pairs and triples, across separators and a line break', () => {
    assert.deepEqual(
      scanText('made by Quux-Labs', TEST_DENIED).map((h) => h.entry),
      [2],
    );
    assert.deepEqual(
      scanText('Quux\nLabs', TEST_DENIED).map((h) => [h.line, h.entry]),
      [[1, 2]],
    );
    assert.deepEqual(
      scanText('x a.b.c y', TEST_DENIED).map((h) => h.entry),
      [3],
    );
  });
  it('does not match parts of a word', () => {
    assert.deepEqual(scanText('quuxlabs zebracornish', TEST_DENIED), []);
  });
});

describe('scanFiles', () => {
  const root = mkdtempSync(join(tmpdir(), 'identity-check-'));
  after(() => rmSync(root, { recursive: true, force: true }));

  it('scans text files and file names, skips binary files', () => {
    mkdirSync(join(root, 'docs'));
    writeFileSync(join(root, 'docs/a.md'), 'line one\nzebracorn\n');
    writeFileSync(join(root, 'docs/zebracorn.md'), 'clean\n');
    writeFileSync(join(root, 'docs/b.bin'), Buffer.from([0x7a, 0, 0x7a, 0x65]));
    writeFileSync(
      join(root, 'docs/c.bin'),
      Buffer.concat([Buffer.from('zebracorn'), Buffer.from([0])]),
    );
    const hits = scanFiles(
      root,
      ['docs/a.md', 'docs/zebracorn.md', 'docs/c.bin', 'docs/missing.md'],
      TEST_DENIED,
    );
    assert.deepEqual(
      hits.map((h) => `${h.file}:${h.line}`),
      ['docs/a.md:2', 'docs/zebracorn.md:0'],
    );
  });
});

describe('DENIED', () => {
  it('holds only SHA-256 hex digests, without duplicates', () => {
    assert.ok(DENIED.length > 0);
    for (const h of DENIED) assert.match(h, /^[0-9a-f]{64}$/);
    assert.equal(new Set(DENIED).size, DENIED.length);
  });
  it('allows the repository URL', () => {
    assert.deepEqual(scanText('https://github.com/AlloryDante/ow-tauri.git'), []);
  });
});
