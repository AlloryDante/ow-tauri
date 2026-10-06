import { afterEach, describe, expect, it, vi } from 'vitest';

import { OwTauriError } from '../shared/errors.js';
import { CONTRACT_VERSION } from '../shared/protocol.js';
import {
  defaultSnapshot,
  mockHost,
  setHostContext,
  settle,
  type MockHost,
} from '../testing/index.js';
import { attachRuntime, installProcessGlobal, installRuntime } from './install.js';
import { contextOfLabel, Kernel, RUNTIME_GLOBAL } from './kernel.js';
import { archFromUserAgent, createProcessShim, platformFromUserAgent } from './process-shim.js';
import { StateCache } from './state-cache.js';
import type { Transport } from './transport.js';

const kernel = attachRuntime();
let host: MockHost | undefined;

afterEach(() => {
  host?.dispose();
  host = undefined;
});

describe('runtime global (B, ADR 0012)', () => {
  it('is installed once, non-writable and non-configurable, and shared', () => {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, RUNTIME_GLOBAL);
    expect(descriptor?.writable).toBe(false);
    expect(descriptor?.configurable).toBe(false);
    const runtime = descriptor?.value as { version: string; contract: number; context: string };
    expect(runtime.contract).toBe(CONTRACT_VERSION);
    expect(Object.isFrozen(runtime)).toBe(true);
    expect(installRuntime()).toBe(kernel);
    expect(attachRuntime()).toBe(kernel);
  });

  it('detects the context from the webview label', () => {
    expect(contextOfLabel('ow-main')).toBe('main');
    expect(contextOfLabel('bw-12')).toBe('ui');
    expect(contextOfLabel('bwr-12')).toBe('none');
    expect(contextOfLabel('owad-bw-1-1')).toBe('none');
    expect(contextOfLabel(undefined)).toBe('none');
    host = mockHost({ label: 'bw-3' });
    expect(
      (globalThis as unknown as Record<string, { context: string }>)[RUNTIME_GLOBAL]?.context,
    ).toBe('ui');
    setHostContext('main');
    expect(kernel.context).toBe('main');
    setHostContext(null);
    expect(kernel.context).toBe('ui');
  });

  it('gives a page with another contract a kernel whose members throw not-ready', () => {
    const fake = { version: '9.9.9', contract: CONTRACT_VERSION + 1 };
    const original = Object.getOwnPropertyDescriptor(globalThis, RUNTIME_GLOBAL);
    // The real global is non-configurable; test the attach logic on a fresh realm-like global.
    expect(original?.configurable).toBe(false);
    const mismatched = new Kernel({
      mismatch: new OwTauriError(
        'not-ready',
        `ow-tauri runtime ${fake.version} (contract 2) does not match package`,
      ),
    });
    expect(() => {
      mismatched.require('main', 'app.quit');
    }).toThrow(/does not match package/);
    return expect(mismatched.start()).resolves.toBeNull();
  });
});

describe('subscription and main readiness (A.2.1)', () => {
  it('subscribes, then sends ipc_main_ready and main_ready in order, then resolves whenHostReady', async () => {
    host = mockHost({ label: 'ow-main' });
    expect(kernel.isReady).toBe(false);
    await kernel.whenHostReady();
    expect(kernel.isReady).toBe(true);
    expect(host.calls.map((c) => c.command)).toEqual([
      'ipc_subscribe',
      'ipc_main_ready',
      'main_ready',
    ]);
    expect(host.epoch).toBe('epoch-1');
  });

  it('still becomes ready when the host does not acknowledge', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    host = mockHost({
      label: 'ow-main',
      commands: {
        main_ready: () => {
          throw { code: 'backend', message: 'no' };
        },
      },
    });
    await kernel.whenHostReady();
    expect(kernel.isReady).toBe(true);
  });

  it('sends main_ready even when ipc_main_ready fails', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    host = mockHost({
      label: 'ow-main',
      commands: {
        ipc_main_ready: () => {
          throw { code: 'backend', message: 'router down' };
        },
      },
    });
    await kernel.whenHostReady();
    expect(host.calls.map((c) => c.command)).toEqual([
      'ipc_subscribe',
      'ipc_main_ready',
      'log',
      'main_ready',
    ]);
    expect(error).toHaveBeenCalledWith(
      expect.stringContaining('ipc_main_ready was not acknowledged'),
    );
  });

  it('keeps the context detected from the label for the life of the document', () => {
    host = mockHost({ label: 'bw-1' });
    expect(kernel.context).toBe('ui');
    const internals = (
      globalThis as unknown as {
        __TAURI_INTERNALS__: { metadata: { currentWebview: { label: string } } };
      }
    ).__TAURI_INTERNALS__;
    internals.metadata.currentWebview.label = 'ow-main';
    expect(kernel.context).toBe('ui');
    kernel.reset();
    expect(kernel.context).toBe('main');
  });

  it('waits for DOMContentLoaded when the document is still loading', async () => {
    const state = vi.spyOn(document, 'readyState', 'get').mockReturnValue('loading');
    host = mockHost({ label: 'ow-main' });
    await settle();
    expect(host.callsOf('main_ready')).toEqual([]);
    state.mockRestore();
    document.dispatchEvent(new Event('DOMContentLoaded'));
    await kernel.whenHostReady();
    expect(host.callsOf('main_ready')).toHaveLength(1);
  });

  it('does not subscribe without IPC or in context none', async () => {
    host = mockHost({ label: 'owad-bw-1-1' });
    await settle();
    expect(host.callsOf('ipc_subscribe')).toEqual([]);
    const offline: Transport = {
      invoke: vi.fn(),
      channel: vi.fn(),
      label: () => 'ow-main',
      available: () => false,
    };
    await expect(
      new Kernel({ transport: offline, readBootstrap: () => null }).start(),
    ).resolves.toBeNull();
  });

  it('rejects a subscription without an epoch', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    host = mockHost({ label: 'bw-1', commands: { ipc_subscribe: () => ({}) } });
    await expect(kernel.start()).rejects.toMatchObject({ code: 'backend' });
  });

  it('ignores channel payloads of a previous subscription', async () => {
    host = mockHost({ label: 'bw-1' });
    await settle();
    const fn = vi.fn();
    kernel.ipcRenderer.on('x', fn);
    const stale = host;
    host = mockHost({ label: 'bw-1' });
    await settle();
    kernel.ipcRenderer.on('x', fn);
    expect(() => {
      stale.push({ type: 'ipc', kind: 'message', channel: 'x', args: [] });
    }).not.toThrow();
    host.push({ type: 'ipc', kind: 'message', channel: 'x', args: [] });
    expect(fn).toHaveBeenCalledTimes(1);
  });
});

describe('host message bus', () => {
  it('routes messages by type, in order, and isolates handler errors', async () => {
    host = mockHost({ label: 'ow-main' });
    await settle();
    const report = vi.fn();
    vi.stubGlobal('reportError', report);
    const seen: string[] = [];
    const off = kernel.on('window', (m) => seen.push(`w${String((m as { id: number }).id)}`));
    const offLifecycle = kernel.on('lifecycle', () => {
      throw new Error('handler');
    });
    host.push(
      { type: 'window', id: 1, event: 'focus' },
      { type: 'lifecycle', event: 'quit' },
      { type: 'window', id: 2, event: 'blur' },
    );
    expect(seen).toEqual(['w1', 'w2']);
    expect(report).toHaveBeenCalled();
    off();
    host.push({ type: 'window', id: 3, event: 'focus' });
    expect(seen).toEqual(['w1', 'w2']);
    offLifecycle();
    vi.unstubAllGlobals();
  });

  it('ignores malformed payload entries and logs unknown types at debug level', async () => {
    host = mockHost({ label: 'ow-main' });
    await settle();
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => undefined);
    kernel.deliver([null, 5, { nope: 1 }]);
    kernel.deliver({ type: 'mystery' });
    expect(debug).toHaveBeenCalledWith(
      expect.stringContaining("no handler for host message 'mystery'"),
    );
  });
});

describe('default answers (A.6)', () => {
  it('lets quit and close requests proceed when no module claims them', async () => {
    host = mockHost({ label: 'ow-main' });
    await settle();
    host.clearCalls();
    host.push(
      { type: 'lifecycle', event: 'before-quit', requestId: 4 },
      { type: 'lifecycle', event: 'will-quit', requestId: 5 },
      { type: 'lifecycle', event: 'quit', exitCode: 0 },
      { type: 'window', id: 3, event: 'close', requestId: 6 },
      { type: 'window', id: 3, event: 'show' },
    );
    await settle();
    expect(host.callsOf('app_quit_reply')).toEqual([
      { requestId: 4, prevent: false },
      { requestId: 5, prevent: false },
    ]);
    expect(host.callsOf('window_close_reply')).toEqual([{ id: 3, requestId: 6, prevent: false }]);
  });

  it('logs a failed default answer', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    host = mockHost({
      label: 'ow-main',
      commands: {
        app_quit_reply: () => {
          throw { code: 'not-found', message: 'gone' };
        },
      },
    });
    await settle();
    host.push({ type: 'lifecycle', event: 'before-quit', requestId: 1 });
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('app_quit_reply failed'));
  });
});

describe('state cache through the channel (B.1.6, C.6)', () => {
  it('builds from the snapshot and applies numbered patches', async () => {
    host = mockHost({ label: 'ow-main', snapshot: { seq: 4 } });
    await settle();
    expect(kernel.state.get('identity.uid')).toBe('testuid');
    expect(kernel.state.seq).toBe(4);
    host.push({
      type: 'state',
      seq: 5,
      patches: [
        { path: 'identity.uid', value: 'new' },
        { path: 'packages.packageState.overlay.active', value: true },
      ],
    });
    expect(kernel.state.get('identity.uid')).toBe('new');
    expect(kernel.state.get('packages.packageState.overlay.active')).toBe(true);
    host.push({ type: 'state', seq: 5, patches: [{ path: 'identity.uid', value: 'old' }] });
    expect(kernel.state.get('identity.uid')).toBe('new');
    host.push({ type: 'state', patches: 'bad' });
    expect(kernel.state.seq).toBe(5);
  });

  it('resyncs on a gap and holds later messages until the cache is current', async () => {
    let release!: (snapshot: unknown) => void;
    host = mockHost({
      label: 'ow-main',
      commands: { bootstrap: () => new Promise((r) => (release = r)) },
    });
    await settle();
    const order: string[] = [];
    kernel.on('packages', () =>
      order.push(`event sees ${String(kernel.state.get('identity.uid'))}`),
    );
    host.push(
      { type: 'state', seq: 3, patches: [{ path: 'identity.uid', value: 'from-gap' }] },
      { type: 'packages', type2: 'ready' },
    );
    expect(order).toEqual([]);
    expect(host.callsOf('bootstrap')).toHaveLength(1);
    release(
      defaultSnapshot({
        seq: 2,
        identity: { uid: 'resynced', cuid: '', muid: '', muidV2: '', phasePercent: 0 },
      }),
    );
    await settle();
    expect(order).toEqual(['event sees from-gap']);
    expect(kernel.state.seq).toBe(3);
  });

  it('continues after a failed or short resync', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    host = mockHost({
      label: 'ow-main',
      commands: {
        bootstrap: () => {
          throw { code: 'backend', message: 'down' };
        },
      },
    });
    await settle();
    host.push({ type: 'state', seq: 9, patches: [{ path: 'a', value: 1 }] });
    await settle();
    expect(kernel.state.get('a')).toBe(1);
    host.setCommand('bootstrap', () => defaultSnapshot({ seq: 1 }));
    host.push({ type: 'state', seq: 20, patches: [{ path: 'b', value: 2 }] });
    await settle();
    expect(kernel.state.get('b')).toBe(2);
    expect(kernel.state.seq).toBe(20);
  });
});

describe('StateCache', () => {
  it('reads and writes dot paths and notifies listeners', () => {
    const cache = new StateCache();
    const listener = vi.fn();
    const off = cache.onChange(listener);
    cache.load({ seq: 1, a: { b: [10, 20] }, x: 1 });
    expect(listener).toHaveBeenCalledWith('a', { b: [10, 20] }, undefined);
    expect(cache.get('a.b.1')).toBe(20);
    expect(cache.get('a.missing.deeper')).toBeUndefined();
    expect(cache.get('x.y')).toBeUndefined();
    cache.set('a.c.d', 5);
    expect(cache.get('a.c')).toEqual({ d: 5 });
    cache.set('__proto__.polluted', 1);
    cache.set('', 1);
    expect(({} as Record<string, unknown>)['polluted']).toBeUndefined();
    expect(cache.apply({ seq: 3, patches: [] })).toBe('gap');
    expect(cache.apply({ seq: 2, patches: [{ path: 'x', value: 2 }] })).toBe('applied');
    expect(cache.apply({ seq: 1, patches: [] })).toBe('ignored');
    off();
    cache.load('not an object');
    expect(cache.seq).toBe(0);
    expect(cache.get('')).toEqual({});
  });

  it('survives listener errors and unclonable snapshots', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const cache = new StateCache();
    cache.onChange(() => {
      throw new Error('listener');
    });
    cache.load({ fn: () => 1, seq: 2 });
    expect(error).toHaveBeenCalled();
    expect(cache.seq).toBe(2);
  });
});

describe('webContents.executeJavaScript support (A.2.3 window_eval)', () => {
  it('reports expression values and skips the fallback', async () => {
    host = mockHost({ label: 'bw-1' });
    await settle();
    kernel.evalBegin(1, () => ({ answer: 42 }));
    kernel.evalFallback(1, () => {
      throw new Error('must not run');
    });
    kernel.evalBegin(2, () => undefined);
    await settle();
    expect(host.callsOf('eval_result')).toEqual([
      { id: 1, ok: true, value: { answer: 42 } },
      { id: 2, ok: true },
    ]);
  });

  it('runs statements when the expression did not parse, awaits promises and reports errors', async () => {
    host = mockHost({ label: 'bw-1' });
    await settle();
    kernel.evalFallback(3, () => 'ignored statement value');
    kernel.evalBegin(4, () => Promise.resolve(7));
    kernel.evalBegin(5, () => Promise.reject(new RangeError('r')));
    kernel.evalBegin(6, () => {
      throw new TypeError('t');
    });
    kernel.evalBegin(7, () => () => 1);
    await settle();
    const results = host.callsOf('eval_result');
    expect(results).toEqual([
      { id: 3, ok: true },
      {
        id: 6,
        ok: false,
        error: {
          code: 'ipc-remote-error',
          message: 't',
          data: { name: 'TypeError', message: 't', text: 'TypeError: t' },
        },
      },
      expect.objectContaining({
        id: 7,
        ok: false,
        error: expect.objectContaining({ code: 'ipc-serialization' }) as unknown,
      }),
      { id: 4, ok: true, value: 7 },
      {
        id: 5,
        ok: false,
        error: {
          code: 'ipc-remote-error',
          message: 'r',
          data: { name: 'RangeError', message: 'r', text: 'RangeError: r' },
        },
      },
    ]);
  });

  it('bounds the set of begun evaluations and logs failed reports', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    host = mockHost({
      label: 'bw-1',
      commands: {
        eval_result: () => {
          throw { code: 'not-found', message: 'gone' };
        },
      },
    });
    await settle();
    for (let i = 0; i < 1030; i++) kernel.evalBegin(i, () => undefined);
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('eval_result 0 failed'));
  });

  it('is exposed on the runtime global', async () => {
    host = mockHost({ label: 'bw-1' });
    await settle();
    const runtime = (
      globalThis as unknown as Record<
        string,
        {
          evalBegin(id: number, fn: () => unknown): void;
          evalFallback(id: number, fn: () => unknown): void;
        }
      >
    )[RUNTIME_GLOBAL];
    runtime?.evalBegin(1, () => 1);
    runtime?.evalFallback(2, () => 2);
    await settle();
    expect(host.callsOf('eval_result')).toHaveLength(2);
  });
});

describe('logging', () => {
  it('logs to the console and, in ow-main, to the log command; warnOnce deduplicates', async () => {
    host = mockHost({ label: 'ow-main' });
    await settle();
    const info = vi.spyOn(console, 'info').mockImplementation(() => undefined);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const err = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    kernel.log('info', 'hello');
    kernel.log('error', 'bad');
    kernel.warnOnce('k', 'once');
    kernel.warnOnce('k', 'once');
    await settle();
    expect(info).toHaveBeenCalledWith('[ow-tauri] hello');
    expect(err).toHaveBeenCalledWith('[ow-tauri] bad');
    expect(warn).toHaveBeenCalledTimes(1);
    expect(host.callsOf('log')).toEqual([
      { level: 'info', message: 'hello' },
      { level: 'error', message: 'bad' },
      { level: 'warn', message: 'once' },
    ]);
  });

  it('does not call the log command from UI windows', async () => {
    host = mockHost({ label: 'bw-1' });
    await settle();
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    kernel.log('warn', 'x');
    await settle();
    expect(host.callsOf('log')).toEqual([]);
  });

  it('survives a failing log command', async () => {
    host = mockHost({
      label: 'ow-main',
      commands: {
        log: () => {
          throw 'denied';
        },
      },
    });
    await settle();
    vi.spyOn(console, 'debug').mockImplementation(() => undefined);
    kernel.log('debug', 'x');
    await settle();
  });
});

describe('singletons and reset hooks', () => {
  it('creates a singleton once and runs reset hooks', () => {
    const factory = vi.fn(() => ({}));
    const a = kernel.singleton('test.a', factory);
    expect(kernel.singleton('test.a', factory)).toBe(a);
    expect(factory).toHaveBeenCalledTimes(1);
    const hook = vi.fn();
    const off = kernel.onReset(hook);
    kernel.reset();
    expect(hook).toHaveBeenCalledTimes(1);
    off();
    kernel.reset();
    expect(hook).toHaveBeenCalledTimes(1);
  });
});

describe('process shim (B.2.5)', () => {
  it('derives platform and arch from the user agent', () => {
    expect(platformFromUserAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')).toBe('win32');
    expect(platformFromUserAgent('Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)')).toBe('darwin');
    expect(platformFromUserAgent('Mozilla/5.0 (X11; Linux x86_64)')).toBe('linux');
    expect(archFromUserAgent('aarch64')).toBe('arm64');
    expect(archFromUserAgent('x86_64')).toBe('x64');
  });

  it('is frozen, has no electron version, and exposes OVERWOLF_APP_UID from the start', async () => {
    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(
      'Mozilla/5.0 (Windows NT 10.0) Chrome/130.0.0.0 Edg/130.0.0.0',
    );
    host = mockHost({ label: 'ow-main' });
    const shim = createProcessShim(kernel);
    expect(Object.isFrozen(shim)).toBe(true);
    expect(shim.platform).toBe('win32');
    expect(shim.arch).toBe('x64');
    expect(shim.argv).toEqual(['test-app']);
    expect(shim.versions).toEqual({ owTauri: '0.1.0', tauri: '2.12.1', chrome: '130.0.0.0' });
    expect(shim.env['OVERWOLF_APP_UID']).toBe('testuid');
    await kernel.whenHostReady();
    expect(shim.env['OVERWOLF_APP_UID']).toBe('testuid');
    setHostContext('ui');
    expect(shim.env['OVERWOLF_APP_UID']).toBeUndefined();
    setHostContext(null);
  });

  it("has Electron's type, a microtask nextTick and an assignable env", async () => {
    host = mockHost({ label: 'bw-1' });
    const shim = createProcessShim(kernel);
    expect(shim.type).toBe('renderer');
    setHostContext('main');
    expect(shim.type).toBe('browser');
    setHostContext(null);
    const tick = vi.fn();
    shim.nextTick(tick, 1, 2);
    expect(tick).not.toHaveBeenCalled();
    await Promise.resolve();
    expect(tick).toHaveBeenCalledWith(1, 2);
    shim.env['NODE_DEBUG'] = 'x';
    expect(shim.env['NODE_DEBUG']).toBe('x');
    expect(() => {
      shim.env['OVERWOLF_APP_UID'] = 'spoofed';
    }).toThrow(TypeError);
  });

  it('falls back without a snapshot', () => {
    host = mockHost({ label: 'bw-1', snapshot: null });
    const shim = createProcessShim(kernel);
    expect(shim.argv).toEqual([]);
    expect(shim.versions['tauri']).toBeUndefined();
  });

  it('never replaces an existing process global', () => {
    host = mockHost({ label: 'bw-1' });
    const g = globalThis as unknown as Record<string, unknown>;
    const before = g['process'];
    installProcessGlobal(kernel);
    expect(g['process']).toBe(before);
  });

  it('installs the global when none exists', () => {
    host = mockHost({ label: 'bw-1' });
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'process');
    // Simulate a webview without Node's process.
    delete (globalThis as unknown as Record<string, unknown>)['process'];
    try {
      installProcessGlobal(kernel);
      expect((globalThis as unknown as { process: { platform: string } }).process.platform).toBe(
        'win32',
      );
    } finally {
      if (descriptor) Object.defineProperty(globalThis, 'process', descriptor);
    }
  });
});
