import { describe, expect, it } from 'vitest';

import { stagedManifest, tauriConfig } from './manifest.mjs';

const base = {
  name: 'ow-tauri-ad-showcase',
  productName: 'ow-tauri Ad Showcase',
  author: 'Example Studio',
  version: '1.0.0',
  overwolf: { packages: [] },
  main: 'main/main.js',
};

describe('stagedManifest', () => {
  it('keeps the placeholder identity without an identity file', () => {
    expect(stagedManifest(base, null)).toEqual(base);
  });

  it('merges author, product name, name and version', () => {
    const out = stagedManifest(base, {
      author: { name: 'Studio B' },
      productName: 'Game Helper',
      name: 'game-helper',
      version: '2.0.0',
      unrelated: 'ignored',
    });
    expect(out).toMatchObject({
      author: { name: 'Studio B' },
      productName: 'Game Helper',
      name: 'game-helper',
      version: '2.0.0',
      main: 'main/main.js',
    });
    expect(out).not.toHaveProperty('unrelated');
  });

  it('writes a uid override as overwolf.uid and keeps the packages list', () => {
    const out = stagedManifest(base, { uid: ' aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj ' });
    expect(out.overwolf).toEqual({ packages: [], uid: 'aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj' });
  });

  it('ignores an empty uid and rejects an unsafe one', () => {
    expect(stagedManifest(base, { uid: '' }).overwolf).toEqual({ packages: [] });
    expect(() => stagedManifest(base, { uid: '../x' })).toThrow(/uid/);
  });

  it('does not change the input', () => {
    const copy = structuredClone(base);
    stagedManifest(base, { productName: 'X', uid: 'abc' });
    expect(base).toEqual(copy);
  });
});

describe('tauriConfig', () => {
  it('carries the placeholder identity', () => {
    expect(tauriConfig(base)).toEqual({
      productName: 'ow-tauri Ad Showcase',
      version: '1.0.0',
      plugins: { overwolf: { name: 'ow-tauri Ad Showcase', author: 'Example Studio' } },
    });
  });

  it('carries a local identity with its uid', () => {
    const staged = stagedManifest(base, {
      productName: 'Other App',
      author: { name: 'Other Studio' },
      uid: 'abc123',
    });
    expect(tauriConfig(staged).plugins).toEqual({
      overwolf: { name: 'Other App', author: 'Other Studio', uid: 'abc123' },
    });
  });

  it('falls back to the package name', () => {
    expect(tauriConfig({ name: 'x', version: '2.0.0' })).toEqual({
      productName: 'x',
      version: '2.0.0',
      plugins: { overwolf: { name: 'x' } },
    });
  });
});
