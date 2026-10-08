import { Window as HappyWindow } from 'happy-dom';
import { afterEach, describe, expect, it } from 'vitest';

import { Outbox, withPostMessageIpc } from './outbox.js';

const windows: HappyWindow[] = [];
afterEach(async () => {
  for (const w of windows.splice(0)) await w.happyDOM.close();
});

/** A guest window whose own `fetch` records what it was asked for. */
function guest(): { win: Window; fetched: unknown[]; pageFetch: unknown } {
  const happy = new HappyWindow({
    url: 'https://www.overwolf.com/monsdk/electron/latest/adview.html',
  });
  windows.push(happy);
  const win = happy as unknown as Window;
  const fetched: unknown[] = [];
  const pageFetch = (input: unknown): Promise<string> => {
    fetched.push(input);
    return Promise.resolve('page');
  };
  Reflect.set(win, 'fetch', pageFetch);
  return { win, fetched, pageFetch };
}

/**
 * Tauri's IPC as its `ipc-protocol.js` sends a call: a synchronous fetch of
 * the IPC endpoint, then `postMessage` when that fetch fails.
 */
function tauriIpc(win: Window, endpoint: string) {
  const via: string[] = [];
  let protocolFailed = false;
  const send = (command: string): void => {
    if (protocolFailed) {
      via.push(`postMessage ${command}`);
      return;
    }
    const fetchFn = Reflect.get(win, 'fetch') as (url: string) => Promise<unknown>;
    fetchFn(`${endpoint}${encodeURIComponent(command)}`).then(
      () => via.push(`fetch ${command}`),
      () => {
        protocolFailed = true;
        send(command);
      },
    );
  };
  Reflect.set(win, '__TAURI_INTERNALS__', {
    invoke: (command: string) => {
      send(command);
      return Promise.resolve();
    },
  });
  return via;
}

const flush = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

describe('guest IPC transport', () => {
  // Regression (Windows lab, ipc-probe): the remote ad page reached Tauri's
  // http://ipc.localhost endpoint without an Origin header; Tauri answered
  // every guest call "missing Origin header" and the host never heard from
  // its guests (no dom-ready, no ad events).
  it('sends guest calls through postMessage where IPC is served over HTTP', async () => {
    const { win, fetched, pageFetch } = guest();
    const via = tauriIpc(win, 'http://ipc.localhost/');
    const outbox = new Outbox(win, 'plugin:overwolf|adview_event');
    outbox.post({ name: '__host:domReady' });
    outbox.post({ name: 'display_ad_loaded' });
    await flush();
    expect(via).toEqual([
      'postMessage plugin:overwolf|adview_event',
      'postMessage plugin:overwolf|adview_event',
    ]);
    // The page's own fetch is back and never saw the IPC request.
    expect(Reflect.get(win, 'fetch')).toBe(pageFetch);
    expect(fetched).toEqual([]);
  });

  it('leaves a custom-scheme IPC (macOS) on its own fetch', async () => {
    const { win, fetched } = guest();
    const via = tauriIpc(win, 'ipc://localhost/');
    new Outbox(win, 'plugin:overwolf|adview_event').post({ name: 'x' });
    await flush();
    expect(via).toEqual(['fetch plugin:overwolf|adview_event']);
    expect(fetched).toEqual(['ipc://localhost/plugin%3Aoverwolf%7Cadview_event']);
  });

  it('passes every other request to the page fetch, even during a call', async () => {
    const { win, fetched, pageFetch } = guest();
    const inside = await withPostMessageIpc(win, () => {
      const f = Reflect.get(win, 'fetch') as (input: unknown) => Promise<unknown>;
      expect(f).not.toBe(pageFetch);
      return Promise.allSettled([
        f('https://example.com/a'),
        f(new URL('https://example.com/b')),
        f({ url: 'HTTPS://IPC.LOCALHOST/x' }),
        f('http://ipc.localhost/plugin%3Aoverwolf%7Ccmp_event'),
      ]);
    });
    expect(inside.map((r) => r.status)).toEqual(['fulfilled', 'fulfilled', 'rejected', 'rejected']);
    expect(fetched).toEqual(['https://example.com/a', new URL('https://example.com/b')]);
    expect(Reflect.get(win, 'fetch')).toBe(pageFetch);
  });

  it('restores the page fetch when the call throws, and copes without one', () => {
    const { win, pageFetch } = guest();
    expect(() =>
      withPostMessageIpc(win, () => {
        throw new Error('refused');
      }),
    ).toThrow('refused');
    expect(Reflect.get(win, 'fetch')).toBe(pageFetch);
    Reflect.set(win, 'fetch', undefined);
    expect(withPostMessageIpc(win, () => 7)).toBe(7);
    expect(Reflect.get(win, 'fetch')).toBeUndefined();
  });
});
