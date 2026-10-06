import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
import type { IpcSender } from '../shared/protocol.js';
import { mockHost, settle, type MockHost } from '../testing/index.js';
import { attachRuntime } from './install.js';
import type { IpcMainEvent } from './ipc-main.js';

const kernel = attachRuntime();
const ipcMain = kernel.server.ipcMain;
let host: MockHost;

const sender = (windowId = 1): IpcSender => ({
  windowId,
  label: `bw-${String(windowId)}`,
  url: 'tauri://localhost/index.html',
  frameId: 0,
});

function invoke(id: number, channel: string, args: unknown[] = [], windowId = 1): void {
  host.push({
    type: 'ipc',
    kind: 'invoke',
    id,
    channel,
    args: args as never,
    sender: sender(windowId),
  });
}

beforeEach(async () => {
  host = mockHost({ label: 'ow-main' });
  await settle();
});

afterEach(() => {
  host.dispose();
});

describe('ipcMain.handle (C.2)', () => {
  it('replies with the encoded return value and a per-target seq', async () => {
    ipcMain.handle('sum', (_e, a: number, b: number) => a + b);
    ipcMain.handle('nothing', () => undefined);
    invoke(1, 'sum', [2, 3]);
    invoke(2, 'nothing');
    invoke(3, 'sum', [1, { $otj: 'number', v: 'Infinity' }], 2);
    await settle();
    expect(host.callsOf('ipc_reply')).toEqual([
      { id: 1, ok: true, value: 5, seq: 1 },
      { id: 2, ok: true, seq: 2 },
      { id: 3, ok: true, value: { $otj: 'number', v: 'Infinity' }, seq: 1 },
    ]);
  });

  it('awaits async handlers', async () => {
    ipcMain.handle('later', async () => {
      await Promise.resolve();
      return new Set([1]);
    });
    invoke(1, 'later');
    await settle();
    expect(host.callsOf('ipc_reply')).toEqual([
      { id: 1, ok: true, value: { $otj: 'set', values: [1] }, seq: 1 },
    ]);
  });

  it('orders webContents.send messages from the handler before its reply', async () => {
    ipcMain.handle('work', (event) => {
      const wc = event.sender as { send(channel: string, ...args: unknown[]): void };
      wc.send('progress', 1);
      wc.send('progress', 2);
      return 'done';
    });
    invoke(1, 'work');
    await settle();
    expect(host.callsOf('ipc_emit')).toEqual([
      { target: 1, channel: 'progress', args: [1], seq: 1 },
      { target: 1, channel: 'progress', args: [2], seq: 2 },
    ]);
    expect(host.callsOf('ipc_reply')).toEqual([{ id: 1, ok: true, value: 'done', seq: 3 }]);
  });

  it('replies ipc-no-handler for unknown channels', async () => {
    invoke(4, 'missing');
    await settle();
    expect(host.callsOf('ipc_reply')).toEqual([
      {
        id: 4,
        ok: false,
        error: {
          code: 'ipc-no-handler',
          message: "No handler registered for 'missing'",
          data: { channel: 'missing' },
        },
        seq: 1,
      },
    ]);
  });

  it('replies ipc-remote-error with name and message when the handler throws or rejects', async () => {
    ipcMain.handle('throws', () => {
      throw new TypeError('bad input');
    });
    ipcMain.handle('rejects', () => Promise.reject(new OwTauriError('io', 'disk')));
    ipcMain.handle('string', () => Promise.reject(new Error('plain')));
    invoke(1, 'throws');
    invoke(2, 'rejects');
    invoke(3, 'string');
    await settle();
    expect(host.callsOf('ipc_reply')).toEqual([
      {
        id: 1,
        ok: false,
        error: {
          code: 'ipc-remote-error',
          message: 'bad input',
          data: { name: 'TypeError', message: 'bad input', text: 'TypeError: bad input' },
        },
        seq: 1,
      },
      {
        id: 2,
        ok: false,
        error: {
          code: 'ipc-remote-error',
          message: 'disk',
          data: { name: 'OwTauriError', message: 'disk', text: 'OwTauriError: disk' },
        },
        seq: 2,
      },
      {
        id: 3,
        ok: false,
        error: {
          code: 'ipc-remote-error',
          message: 'plain',
          data: { name: 'Error', message: 'plain', text: 'Error: plain' },
        },
        seq: 3,
      },
    ]);
  });

  it('replies ipc-serialization when the return value cannot be encoded', async () => {
    ipcMain.handle('fn', () => () => 1);
    invoke(1, 'fn');
    await settle();
    const [reply] = host.callsOf('ipc_reply');
    expect(reply).toMatchObject({ id: 1, ok: false, error: { code: 'ipc-serialization' }, seq: 1 });
  });

  it('replies an error when the arguments cannot be decoded', async () => {
    ipcMain.handle('x', vi.fn());
    invoke(1, 'x', [{ $otj: 'bigint', v: 'zz' }]);
    await settle();
    expect(host.callsOf('ipc_reply')[0]).toMatchObject({
      ok: false,
      error: { code: 'ipc-remote-error', data: { name: 'SyntaxError' } },
    });
  });

  it('passes an IpcMainInvokeEvent', async () => {
    let seen: IpcMainEvent | undefined;
    ipcMain.handle('ev', (event) => {
      seen = event as IpcMainEvent;
    });
    invoke(1, 'ev');
    await settle();
    expect(seen).toMatchObject({
      type: 'frame',
      frameId: 0,
      processId: 0,
      senderFrame: { url: 'tauri://localhost/index.html' },
    });
    expect((seen?.sender as { id: number }).id).toBe(1);
    expect(seen?.defaultPrevented).toBe(false);
    seen?.preventDefault();
    expect(seen?.defaultPrevented).toBe(true);
    // Electron's IpcMainInvokeEvent has no reply, returnValue or ports.
    expect('reply' in seen!).toBe(false);
    expect('returnValue' in seen!).toBe(false);
    expect('ports' in seen!).toBe(false);
  });

  it("sends Electron's toString() text for thrown non-errors and empty messages", async () => {
    ipcMain.handle('str', () => {
      throw 'boom';
    });
    ipcMain.handle('empty', () => {
      throw new Error('');
    });
    invoke(1, 'str');
    invoke(2, 'empty');
    await settle();
    expect(host.callsOf('ipc_reply').map((r) => (r['error'] as { data: unknown }).data)).toEqual([
      { name: 'Error', message: 'boom', text: 'boom' },
      { name: 'Error', message: '', text: 'Error' },
    ]);
  });

  it('replies ipc-serialization for a return value over ipc.maxMessageBytes', async () => {
    ipcMain.handle('big', () => 'x'.repeat(kernel.server.maxMessageBytes));
    invoke(1, 'big');
    await settle();
    expect(host.callsOf('ipc_reply')).toEqual([
      {
        id: 1,
        ok: false,
        error: {
          code: 'ipc-serialization',
          message: expect.stringContaining('more than the 8388608-byte limit') as string,
          data: { bytes: kernel.server.maxMessageBytes + 2, limit: kernel.server.maxMessageBytes },
        },
        seq: 1,
      },
    ]);
  });

  it('replaces an undeliverable reply with an error under the same seq', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    host.setCommand('ipc_reply', (args) => {
      if (args['ok'] === true) throw 'invalid args `value` for command `ipc_reply`';
      return null;
    });
    ipcMain.handle('v', () => 1);
    invoke(1, 'v');
    await settle();
    const replies = host.callsOf('ipc_reply');
    expect(replies).toHaveLength(2);
    expect(replies[1]).toMatchObject({
      id: 1,
      ok: false,
      seq: 1,
      error: {
        code: 'backend',
        message: expect.stringContaining('could not be delivered') as string,
      },
    });
    expect(host.callsOf('ipc_emit_skip')).toEqual([]);
    expect(warn).toHaveBeenCalled();
  });

  it('skips the seq when the error reply fails too', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    host.setCommand('ipc_reply', () => {
      throw { code: 'ipc-serialization', message: 'too large' };
    });
    ipcMain.handle('v', () => 1);
    invoke(1, 'v');
    await settle();
    expect(host.callsOf('ipc_reply')[1]).toMatchObject({
      ok: false,
      error: { code: 'ipc-serialization' },
    });
    expect(host.callsOf('ipc_emit_skip')).toEqual([{ target: 1, seq: 1 }]);
  });

  it('refuses a second handler, like Electron', () => {
    ipcMain.handle('dup', vi.fn());
    expect(() => {
      ipcMain.handle('dup', vi.fn());
    }).toThrow("Attempted to register a second handler for 'dup'");
    expect(() => {
      ipcMain.handle('h', 'nope' as never);
    }).toThrow(TypeError);
    ipcMain.removeHandler('dup');
    ipcMain.handle('dup', vi.fn());
  });

  it('handleOnce removes itself after the first call', async () => {
    const fn = vi.fn(() => 1);
    ipcMain.handleOnce('once', fn);
    invoke(1, 'once');
    invoke(2, 'once');
    await settle();
    expect(fn).toHaveBeenCalledTimes(1);
    const replies = host.callsOf('ipc_reply').sort((a, b) => Number(a['id']) - Number(b['id']));
    expect(replies.map((r) => r['ok'])).toEqual([true, false]);
    ipcMain.handleOnce('once', fn);
    ipcMain.removeHandler('once');
    ipcMain.handle('once', fn);
  });

  it('consults the sender window scope (webContents.ipc) first', async () => {
    ipcMain.handle('who', () => 'global');
    kernel.server.scoped(2).handle('who', () => 'window 2');
    invoke(1, 'who', [], 1);
    invoke(2, 'who', [], 2);
    await settle();
    expect(host.callsOf('ipc_reply').map((r) => r['value'])).toEqual(['global', 'window 2']);
    kernel.server.dropScope(2);
    invoke(3, 'who', [], 2);
    await settle();
    expect(host.callsOf('ipc_reply')[2]?.['value']).toBe('global');
  });

  it('logs failed replies and emits', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    host.setCommand('ipc_reply', () => {
      throw { code: 'not-ready', message: 'gone' };
    });
    host.setCommand('ipc_emit', () => {
      throw { code: 'ipc-overloaded', message: 'full' };
    });
    ipcMain.handle('x', (e) => {
      (e.sender as { send(c: string): void }).send('y');
    });
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => undefined);
    host.setCommand('ipc_emit_skip', () => {
      throw 'command ipc_emit_skip not found';
    });
    invoke(1, 'x');
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('ipc_reply for request 1 failed'));
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("webContents.send('y') failed"));
    // Both rejected sequence numbers are reported (emit 1, reply 2).
    expect(host.callsOf('ipc_emit_skip')).toEqual([
      { target: 1, seq: 1 },
      { target: 1, seq: 2 },
    ]);
    expect(debug).toHaveBeenCalledWith(expect.stringContaining('ipc_emit_skip'));
  });
});

describe('IPC from windows being created or closed', () => {
  it('holds IPC from an unknown window while a create is pending', async () => {
    const id = kernel.windowIds.reserve();
    const scoped = vi.fn(() => 'scoped');
    kernel.server.scoped(id).handle('who', scoped);
    const sent = vi.fn();
    kernel.server.scoped(id).on('note', sent);
    invoke(1, 'who', [], 7);
    host.push({ type: 'ipc', kind: 'send', channel: 'note', args: [1], sender: sender(7) });
    await settle();
    expect(scoped).not.toHaveBeenCalled();
    expect(host.callsOf('ipc_reply')).toEqual([]);
    kernel.windowIds.bind(id, 7);
    await settle();
    expect(scoped).toHaveBeenCalledTimes(1);
    expect(sent).toHaveBeenCalledWith(expect.anything(), 1);
    expect(host.callsOf('ipc_reply')).toEqual([{ id: 1, ok: true, value: 'scoped', seq: 1 }]);
  });

  it('releases held IPC to a new id when the pending create fails', async () => {
    const id = kernel.windowIds.reserve();
    ipcMain.handle('who', (event) => (event.sender as { id: number }).id);
    invoke(1, 'who', [], 8);
    await settle();
    expect(host.callsOf('ipc_reply')).toEqual([]);
    kernel.windowIds.forget(id);
    await settle();
    expect(host.callsOf('ipc_reply')).toEqual([
      { id: 1, ok: true, value: kernel.windowIds.peekHost(8), seq: 1 },
    ]);
  });

  it('drops IPC from a window that was closed instead of re-binding it', async () => {
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => undefined);
    const id = kernel.windowIds.reserve();
    kernel.windowIds.bind(id, 9);
    kernel.windowIds.forget(id);
    const handler = vi.fn();
    ipcMain.handle('late', handler);
    ipcMain.on('late-send', handler);
    invoke(1, 'late', [], 9);
    host.push({ type: 'ipc', kind: 'send', channel: 'late-send', args: [], sender: sender(9) });
    await settle();
    expect(handler).not.toHaveBeenCalled();
    expect(kernel.windowIds.knowsHost(9)).toBe(false);
    expect(debug).toHaveBeenCalledWith(expect.stringContaining('closed window 9'));
  });
});

describe('ipcMain.on (C.3)', () => {
  it('dispatches to the window scope first, then ipcMain, with reply()', async () => {
    const order: string[] = [];
    kernel.server.scoped(1).on('ping', () => order.push('scope'));
    ipcMain.on('ping', (event: IpcMainEvent, n: number) => {
      order.push(`global ${String(n)}`);
      event.reply('pong', n + 1);
    });
    host.push({ type: 'ipc', kind: 'send', channel: 'ping', args: [1], sender: sender(1) });
    await settle();
    expect(order).toEqual(['scope', 'global 1']);
    expect(host.callsOf('ipc_emit')).toEqual([{ target: 1, channel: 'pong', args: [2], seq: 1 }]);
  });

  it('supports once and addListener', () => {
    const once = vi.fn();
    const add = vi.fn();
    ipcMain.once('m', once);
    ipcMain.addListener('m', add);
    host.push({ type: 'ipc', kind: 'send', channel: 'm', args: [], sender: sender() });
    host.push({ type: 'ipc', kind: 'send', channel: 'm', args: [], sender: sender() });
    expect(once).toHaveBeenCalledTimes(1);
    expect(add).toHaveBeenCalledTimes(2);
  });

  it('drops undecodable messages', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const fn = vi.fn();
    ipcMain.on('m', fn);
    host.push({
      type: 'ipc',
      kind: 'send',
      channel: 'm',
      args: [{ $otj: 'bigint', v: '?' }],
      sender: sender(),
    });
    expect(fn).not.toHaveBeenCalled();
    expect(warn).toHaveBeenCalled();
  });

  it('makes returnValue and ports unsupported', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    let event: IpcMainEvent | undefined;
    ipcMain.on('sync', (e: IpcMainEvent) => (event = e));
    host.push({ type: 'ipc', kind: 'send', channel: 'sync', args: [], sender: sender() });
    expect(event?.returnValue).toBeUndefined();
    expect(event?.ports).toBeUndefined();
    expect(warn).toHaveBeenCalledTimes(2);
    expect(() => {
      if (event) event.returnValue = 1;
    }).toThrow(OwTauriUnsupportedError);
  });

  it('ignores unknown ipc kinds', () => {
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => undefined);
    host.push({ type: 'ipc', kind: 'weird', channel: 'x' });
    expect(debug).toHaveBeenCalledWith(expect.stringContaining('unknown ipc message kind'));
  });
});

describe('webContents.send through the server (C.5)', () => {
  it('validates, encodes, and drops messages to windows that do not exist yet', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const id = kernel.windowIds.reserve();
    kernel.server.emit(id, 'x', []);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('does not exist yet'));
    expect(() => {
      kernel.server.emit(id, 'x', [() => 1]);
    }).toThrow(OwTauriError);
    expect(() => {
      kernel.server.emit(id, '', []);
    }).toThrow(/channel/);
  });

  it('translates app-visible ids to plugin ids', async () => {
    const id = kernel.windowIds.reserve();
    kernel.windowIds.bind(id, 41);
    kernel.server.emit(id, 'x', [1]);
    await settle();
    expect(host.callsOf('ipc_emit')).toEqual([{ target: 41, channel: 'x', args: [1], seq: 1 }]);
    expect(kernel.windowIds.fromHost(41)).toBe(id);
  });
});

describe('ipcMain context', () => {
  it('is forbidden in UI windows', () => {
    kernel.setContextOverride('ui');
    expect(() => {
      ipcMain.handle('x', vi.fn());
    }).toThrow(expect.objectContaining({ code: 'forbidden' }) as Error);
    expect(() => ipcMain.on('x', vi.fn())).toThrow(OwTauriError);
    expect(() => ipcMain.once('x', vi.fn())).toThrow(OwTauriError);
    expect(() => ipcMain.addListener('x', vi.fn())).toThrow(OwTauriError);
    expect(() => kernel.server.scoped(9).on('x', vi.fn())).toThrow(/webContents\.ipc\.on/);
    kernel.setContextOverride(null);
  });
});
