import { beforeEach, describe, expect, it } from 'vitest';

import { MAX_SNAPSHOT_UNITS, restoreSessionStorage, snapshotSessionStorage } from './session.js';

const ORIGIN = 'https://ads.example.com';

/** A fake frame window with its own storage. */
function frame(origin = ORIGIN, top = true): Window {
  const store = new Map<string, string>();
  const storage = {
    get length() {
      return store.size;
    },
    key: (i: number) => [...store.keys()][i] ?? null,
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => store.set(key, value),
  };
  const win: Record<string, unknown> = { location: { origin }, sessionStorage: storage };
  win['top'] = top ? win : {};
  return win as unknown as Window;
}

describe('restoreSessionStorage', () => {
  let win: Window;
  beforeEach(() => {
    win = frame();
  });

  it('restores the string values of a snapshot object or its JSON text', () => {
    expect(restoreSessionStorage(win, ORIGIN, { a: '1', b: 2, c: '3' })).toBe(2);
    expect(win.sessionStorage.getItem('a')).toBe('1');
    expect(win.sessionStorage.getItem('b')).toBeNull();
    expect(restoreSessionStorage(win, ORIGIN, '{"d":"4"}')).toBe(1);
    expect(win.sessionStorage.getItem('d')).toBe('4');
  });

  it('does nothing for a missing or malformed snapshot', () => {
    for (const snapshot of [null, undefined, 7, '[1]', ['a'], 'not json'])
      expect(restoreSessionStorage(win, ORIGIN, snapshot)).toBe(0);
    expect(win.sessionStorage.length).toBe(0);
  });

  it('does nothing in a subframe or on another origin', () => {
    expect(restoreSessionStorage(frame(ORIGIN, false), ORIGIN, { a: '1' })).toBe(0);
    expect(restoreSessionStorage(frame('https://other.example'), ORIGIN, { a: '1' })).toBe(0);
  });

  it('does nothing when the frame hides its top window', () => {
    const hidden = frame();
    Object.defineProperty(hidden, 'top', {
      get: () => {
        throw new Error('cross-origin');
      },
    });
    expect(restoreSessionStorage(hidden, ORIGIN, { a: '1' })).toBe(0);
  });
});

describe('snapshotSessionStorage', () => {
  it('reads the top frame storage as an object', () => {
    const win = frame();
    win.sessionStorage.setItem('a', '1');
    win.sessionStorage.setItem('b', '');
    expect(snapshotSessionStorage(win, ORIGIN)).toEqual({ a: '1', b: '' });
  });

  it('round-trips through restore', () => {
    const from = frame();
    from.sessionStorage.setItem('k', 'v');
    const to = frame();
    restoreSessionStorage(to, ORIGIN, JSON.stringify(snapshotSessionStorage(from, ORIGIN)));
    expect(to.sessionStorage.getItem('k')).toBe('v');
  });

  it('never returns a truncated snapshot over the limit', () => {
    const win = frame();
    win.sessionStorage.setItem('ab', 'cd');
    expect(snapshotSessionStorage(win, ORIGIN, 4)).toEqual({ ab: 'cd' });
    expect(snapshotSessionStorage(win, ORIGIN, 3)).toBeNull();
    expect(MAX_SNAPSHOT_UNITS).toBe(2 * 1024 * 1024);
  });

  it('returns null in a subframe, on another origin or when storage throws', () => {
    expect(snapshotSessionStorage(frame(ORIGIN, false), ORIGIN)).toBeNull();
    expect(snapshotSessionStorage(frame('https://other.example'), ORIGIN)).toBeNull();
    const blocked = frame();
    Object.defineProperty(blocked, 'sessionStorage', {
      get: () => {
        throw new Error('SecurityError');
      },
    });
    expect(snapshotSessionStorage(blocked, ORIGIN)).toBeNull();
  });

  it('skips a key that vanished while reading', () => {
    const win = frame();
    win.sessionStorage.setItem('a', '1');
    const storage = win.sessionStorage;
    Object.defineProperty(win, 'sessionStorage', {
      value: { length: 2, key: (i: number) => (i === 0 ? 'a' : null), getItem: storage.getItem },
    });
    expect(snapshotSessionStorage(win, ORIGIN)).toEqual({ a: '1' });
  });
});
