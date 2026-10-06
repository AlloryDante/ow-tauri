import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
import { encode } from '../shared/otj.js';
import { mockHost, settle, type MockHost } from '../testing/index.js';
import { attachRuntime } from './install.js';
import { IpcClient, remoteInvokeError } from './ipc-renderer.js';
import type { KernelServices } from './services.js';

const kernel = attachRuntime();
const ipcRenderer = kernel.ipcRenderer;
let host: MockHost;

beforeEach(async () => {
  host = mockHost({ label: 'bw-1' });
  await settle();
});

afterEach(() => {
  host.dispose();
});

function sync(error: () => unknown): OwTauriError {
  try {
    error();
  } catch (e) {
    return e as OwTauriError;
  }
  throw new Error('expected a synchronous throw');
}

describe('ipcRenderer.invoke (C.2)', () => {
  it('sends channel, encoded args, epoch and increasing seq, and resolves the ipc-result value', async () => {
    const p1 = ipcRenderer.invoke('a', 1, undefined, new Date(0));
    const p2 = ipcRenderer.invoke('b');
    await settle();
    expect(host.callsOf('ipc_invoke')).toEqual([
      {
        channel: 'a',
        args: [1, { $otj: 'undefined' }, { $otj: 'date', v: '1970-01-01T00:00:00.000Z' }],
        epoch: 'epoch-1',
        seq: 1,
      },
      { channel: 'b', args: [], epoch: 'epoch-1', seq: 2 },
    ]);
    host.push(
      { type: 'ipc-result', id: 2, ok: true },
      { type: 'ipc-result', id: 1, ok: true, value: encode(new Map([[1, 2]])) },
    );
    await expect(p1).resolves.toEqual(new Map([[1, 2]]));
    await expect(p2).resolves.toBeUndefined();
  });

  it('queues calls made before ipc_subscribe returns and issues them in order', async () => {
    let release!: (v: unknown) => void;
    host.setCommand('ipc_subscribe', () => new Promise((r) => (release = r)));
    kernel.reset();
    void kernel.start();
    const p = ipcRenderer.invoke('early', 'x');
    ipcRenderer.send('early-send');
    await settle();
    expect(host.callsOf('ipc_invoke')).toEqual([]);
    release({ epoch: 'late' });
    await settle();
    expect(host.callsOf('ipc_invoke')).toEqual([
      { channel: 'early', args: ['x'], epoch: 'late', seq: 1 },
    ]);
    expect(host.callsOf('ipc_send')).toEqual([
      { channel: 'early-send', args: [], epoch: 'late', seq: 2 },
    ]);
    expect(kernel.client.epoch).toBe('late');
    kernel.reset();
    await expect(p).rejects.toMatchObject({ code: 'not-ready' });
  });

  it('holds a result that overtakes its acknowledgement', async () => {
    let ack!: (v: unknown) => void;
    host.setCommand('ipc_invoke', () => new Promise((r) => (ack = r)));
    const p = ipcRenderer.invoke('race');
    await settle();
    host.push({ type: 'ipc-result', id: 7, ok: true, value: 'early' });
    ack({ id: 7 });
    await expect(p).resolves.toBe('early');
  });

  it('delivers messages the handler sent before its reply first (one ordered channel)', async () => {
    const order: string[] = [];
    ipcRenderer.on('progress', (_e, n: number) => order.push(`progress ${String(n)}`));
    const p = ipcRenderer.invoke('work').then(() => order.push('resolved'));
    await settle();
    host.push(
      { type: 'ipc', kind: 'message', channel: 'progress', args: [1] },
      { type: 'ipc', kind: 'message', channel: 'progress', args: [2] },
      { type: 'ipc-result', id: 1, ok: true },
    );
    await p;
    expect(order).toEqual(['progress 1', 'progress 2', 'resolved']);
  });

  it('maps a missing handler and a handler error to Electron messages (C.8)', async () => {
    const missing = ipcRenderer.invoke('nope');
    const thrown = ipcRenderer.invoke('boom');
    const other = ipcRenderer.invoke('slow');
    const bare = ipcRenderer.invoke('bare');
    await settle();
    host.push(
      {
        type: 'ipc-result',
        id: 1,
        ok: false,
        error: { code: 'ipc-no-handler', message: "No handler registered for 'nope'" },
      },
      {
        type: 'ipc-result',
        id: 2,
        ok: false,
        error: {
          code: 'ipc-remote-error',
          message: 'bad',
          data: { name: 'TypeError', message: 'bad' },
        },
      },
      {
        type: 'ipc-result',
        id: 3,
        ok: false,
        error: { code: 'ipc-timeout', message: 'no reply within 10 ms' },
      },
      { type: 'ipc-result', id: 4, ok: false },
    );
    await expect(missing).rejects.toMatchObject({
      code: 'ipc-no-handler',
      message: "Error invoking remote method 'nope': Error: No handler registered for 'nope'",
    });
    const error = await thrown.catch((e: unknown) => e);
    expect(error).toBeInstanceOf(OwTauriError);
    expect(error).toBeInstanceOf(Error);
    expect(error).toMatchObject({
      code: 'ipc-remote-error',
      message: "Error invoking remote method 'boom': TypeError: bad",
      data: { name: 'TypeError', message: 'bad' },
    });
    await expect(other).rejects.toMatchObject({
      code: 'ipc-timeout',
      message: "Error invoking remote method 'slow': no reply within 10 ms",
    });
    await expect(bare).rejects.toMatchObject({ code: 'backend' });
  });

  it('rejects a result whose value cannot be decoded', async () => {
    const p = ipcRenderer.invoke('x');
    await settle();
    host.push({
      type: 'ipc-result',
      id: 1,
      ok: true,
      value: { $otj: 'bigint', v: 'not a number' },
    });
    await expect(p).rejects.toBeInstanceOf(SyntaxError);
  });

  it('throws ipc-serialization and invalid-argument synchronously', () => {
    expect(sync(() => ipcRenderer.invoke('x', () => 1))).toMatchObject({
      code: 'ipc-serialization',
    });
    expect(
      sync(() => {
        ipcRenderer.send('x', Symbol('s'));
      }),
    ).toMatchObject({ code: 'ipc-serialization' });
    expect(sync(() => ipcRenderer.invoke(''))).toMatchObject({ code: 'invalid-argument' });
    expect(
      sync(() => {
        ipcRenderer.send('c'.repeat(257));
      }),
    ).toMatchObject({ code: 'invalid-argument' });
    expect(sync(() => ipcRenderer.invoke(5 as unknown as string))).toMatchObject({
      code: 'invalid-argument',
    });
  });

  it('enforces the encoded size cap at the sender', () => {
    const services = { command: vi.fn(), log: vi.fn() } as unknown as KernelServices;
    const client = new IpcClient(services, { maxMessageBytes: 10, startupQueueMax: 1 });
    expect(sync(() => client.invoke('x', ['a long string']))).toMatchObject({
      code: 'ipc-serialization',
      data: { bytes: 17, limit: 10 }, // ["a long string"]
    });
  });

  it('reports a call Tauri rejected before the command ran with ipc_skip', async () => {
    host.setCommand('ipc_invoke', () => {
      throw 'overwolf.ipc_invoke not allowed on window "bw-1", webview "bw-1", URL: tauri://localhost/';
    });
    const error = await ipcRenderer.invoke('denied').catch((e: unknown) => e);
    expect(error).toMatchObject({ code: 'forbidden' });
    expect((error as OwTauriError).message).toMatch(/^Error invoking remote method 'denied': /);
    await settle();
    expect(host.callsOf('ipc_skip')).toEqual([{ epoch: 'epoch-1', seq: 1 }]);
  });

  it('does not skip when the plugin itself rejected (the command ran)', async () => {
    host.setCommand('ipc_invoke', () => {
      throw { code: 'ipc-overloaded', message: '256 invokes in flight' };
    });
    await expect(ipcRenderer.invoke('busy')).rejects.toMatchObject({
      code: 'ipc-overloaded',
      message: "Error invoking remote method 'busy': 256 invokes in flight",
    });
    await settle();
    expect(host.callsOf('ipc_skip')).toEqual([]);
  });

  it('rejects a malformed acknowledgement', async () => {
    host.setCommand('ipc_invoke', () => ({}));
    await expect(ipcRenderer.invoke('x')).rejects.toMatchObject({ code: 'backend' });
  });

  it('send logs failures and skips pre-command rejections', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    host.setCommand('ipc_send', () => {
      throw 'invalid args `args` for command `ipc_send`';
    });
    ipcRenderer.send('x', 1);
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("ipcRenderer.send('x') failed"));
    expect(host.callsOf('ipc_skip')).toEqual([{ epoch: 'epoch-1', seq: 1 }]);
  });

  it('fails queued and later calls when the subscription fails', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    host.setCommand('ipc_subscribe', () => {
      throw { code: 'forbidden', message: 'not a UI window' };
    });
    kernel.reset();
    const queued = ipcRenderer.invoke('a');
    void kernel.start();
    await expect(queued).rejects.toMatchObject({ code: 'forbidden' });
    await expect(ipcRenderer.invoke('b')).rejects.toMatchObject({ code: 'forbidden' });
    ipcRenderer.send('c');
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("ipcRenderer.send('c') dropped"));
  });

  it('bounds the startup queue', async () => {
    const services = { command: vi.fn(), log: vi.fn() } as unknown as KernelServices;
    const client = new IpcClient(services, { maxMessageBytes: 1000, startupQueueMax: 1 });
    void client.invoke('a', []);
    await expect(client.invoke('b', [])).rejects.toMatchObject({ code: 'not-ready' });
    client.send('c', []);
    expect(services.log).toHaveBeenCalledWith(
      'warn',
      expect.stringContaining('startup queue is full'),
    );
    expect(client.pendingCount).toBe(0);
  });

  it('rejects pending invokes on reset', async () => {
    const p = ipcRenderer.invoke('x');
    await settle();
    expect(kernel.client.pendingCount).toBe(1);
    kernel.reset();
    await expect(p).rejects.toMatchObject({ code: 'not-ready' });
  });

  it('caps results held before their acknowledgement', () => {
    const services = { command: vi.fn(), log: vi.fn() } as unknown as KernelServices;
    const client = new IpcClient(services);
    for (let id = 1; id <= 1030; id++) client.onResult({ type: 'ipc-result', id, ok: true });
    expect(client.pendingCount).toBe(0);
  });
});

describe('ipcRenderer listeners (C.5)', () => {
  it('calls listeners with an IpcRendererEvent and decoded args', () => {
    const fn = vi.fn();
    ipcRenderer.on('news', fn);
    host.push({
      type: 'ipc',
      kind: 'message',
      channel: 'news',
      args: [{ $otj: 'number', v: 'NaN' }, 'x'],
    });
    expect(fn).toHaveBeenCalledWith({ sender: ipcRenderer, senderId: 0, ports: [] }, NaN, 'x');
    const once = vi.fn();
    ipcRenderer.once('news', once);
    ipcRenderer.addListener('news', fn);
    host.push({ type: 'ipc', kind: 'message', channel: 'news', args: [] });
    host.push({ type: 'ipc', kind: 'message', channel: 'news', args: [] });
    expect(once).toHaveBeenCalledTimes(1);
    ipcRenderer.removeAllListeners('news');
  });

  it('drops a message whose args cannot be decoded and keeps going', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const fn = vi.fn();
    ipcRenderer.on('n', fn);
    host.push(
      { type: 'ipc', kind: 'message', channel: 'n', args: [{ $otj: 'bigint', v: 'x' }] },
      { type: 'ipc', kind: 'message', channel: 'n', args: [1] },
    );
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("dropped a message on 'n'"));
    expect(fn).toHaveBeenCalledTimes(1);
    ipcRenderer.removeAllListeners();
  });

  it('isolates listener exceptions', () => {
    const report = vi.fn();
    vi.stubGlobal('reportError', report);
    const fn = vi.fn();
    ipcRenderer.on('a', () => {
      throw new Error('listener');
    });
    ipcRenderer.on('b', fn);
    host.push(
      { type: 'ipc', kind: 'message', channel: 'a', args: [] },
      { type: 'ipc', kind: 'message', channel: 'b', args: [] },
    );
    expect(report).toHaveBeenCalled();
    expect(fn).toHaveBeenCalled();
    ipcRenderer.removeAllListeners();
    vi.unstubAllGlobals();
  });
});

describe('ipcRenderer context and unsupported members', () => {
  it('is forbidden in the main webview', () => {
    kernel.setContextOverride('main');
    expect(sync(() => ipcRenderer.invoke('x'))).toMatchObject({ code: 'forbidden' });
    expect(
      sync(() => {
        ipcRenderer.send('x');
      }),
    ).toMatchObject({ code: 'forbidden' });
    expect(sync(() => ipcRenderer.on('x', vi.fn()))).toMatchObject({ code: 'forbidden' });
    expect(sync(() => ipcRenderer.once('x', vi.fn()))).toMatchObject({ code: 'forbidden' });
    expect(sync(() => ipcRenderer.addListener('x', vi.fn()))).toMatchObject({ code: 'forbidden' });
    kernel.setContextOverride(null);
  });

  it.each(['sendSync', 'sendTo', 'sendToHost', 'postMessage'])(
    '%s throws OwTauriUnsupportedError',
    (name) => {
      const fn = (ipcRenderer as unknown as Record<string, () => unknown>)[name] as () => unknown;
      const error = sync(() => fn.call(ipcRenderer));
      expect(error).toBeInstanceOf(OwTauriUnsupportedError);
      expect((error as OwTauriUnsupportedError).api).toBe(`ipcRenderer.${name}`);
    },
  );
});

describe('remoteInvokeError', () => {
  it('falls back to the wire message when data is missing', () => {
    expect(remoteInvokeError('c', { code: 'ipc-remote-error', message: 'm' }).message).toBe(
      "Error invoking remote method 'c': Error: m",
    );
  });
});
