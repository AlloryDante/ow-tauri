import { describe, expect, it } from 'vitest';

import {
  authorName,
  exportFileName,
  formulaUid,
  maskId,
  relaunchArgs,
  TEST_AD_SWITCH,
} from './identity.js';

describe('formulaUid', () => {
  it('matches the contract vector', async () => {
    await expect(formulaUid('Example Studio', 'Parity Harness')).resolves.toBe(
      'bijigndkghcikkfmhgkmicdkjpdehpjafgpmdhcc',
    );
  });

  it('is 40 letters a..p and depends on both inputs', async () => {
    const a = await formulaUid('Example Studio', 'ow-tauri Ad Showcase');
    const b = await formulaUid('Other Studio', 'ow-tauri Ad Showcase');
    const c = await formulaUid('Example Studio', 'Other App');
    expect(a).toMatch(/^[a-p]{40}$/);
    expect(new Set([a, b, c]).size).toBe(3);
  });
});

describe('authorName', () => {
  it('reads a string or an object author, else "unknown"', () => {
    expect(authorName('Example Studio')).toBe('Example Studio');
    expect(authorName({ name: 'Example Studio', email: 'a@example.com' })).toBe('Example Studio');
    expect(authorName('')).toBe('unknown');
    expect(authorName({})).toBe('unknown');
    expect(authorName(undefined)).toBe('unknown');
    expect(authorName(null)).toBe('unknown');
  });
});

describe('maskId', () => {
  it('keeps the first and last four characters', () => {
    expect(maskId('abcdefghijklmnopqrstuvwxyz')).toBe('abcd…wxyz');
  });

  it('fully masks short values and names an empty one', () => {
    expect(maskId('abcdefghij')).toBe('••••••••••');
    expect(maskId('abc')).toBe('•••');
    expect(maskId('')).toBe('(none)');
  });

  it('never contains the middle of the id', () => {
    const id = 'aaaaMIDDLEbbbb';
    expect(maskId(id)).not.toContain('MIDDLE');
  });
});

describe('relaunchArgs', () => {
  const exe = '/Applications/App.app/Contents/MacOS/App';

  it('adds --test-ad first for test mode', () => {
    expect(relaunchArgs([exe, '.stage/electron'], 'test')).toEqual([
      TEST_AD_SWITCH,
      '.stage/electron',
    ]);
  });

  it('removes --test-ad for live mode', () => {
    expect(relaunchArgs([exe, '--test-ad', '.stage/electron'], 'live')).toEqual([
      '.stage/electron',
    ]);
  });

  it('never repeats the switch', () => {
    expect(relaunchArgs([exe, '--x', '--test-ad'], 'test')).toEqual([TEST_AD_SWITCH, '--x']);
    expect(relaunchArgs([exe], 'live')).toEqual([]);
  });
});

describe('exportFileName', () => {
  it('is file-name safe', () => {
    const name = exportFileName('ow-tauri', 'test', new Date('2026-10-07T12:34:56.789Z'));
    expect(name).toBe('timeline-ow-tauri-test-2026-10-07T12-34-56-789Z.json');
    expect(name).not.toMatch(/[:/\\]/);
  });
});
