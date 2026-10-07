import { describe, expect, it } from 'vitest';

import { OwTauriError, OwTauriUnsupportedError } from './errors.js';
import {
  describeThrown,
  fromWireError,
  isErrorWire,
  isPreCommandRejection,
  v8Stack,
} from './wire-error.js';

describe('wire errors (A.4, C.8)', () => {
  it('recognises the wire shape', () => {
    expect(isErrorWire({ code: 'io', message: 'x' })).toBe(true);
    expect(isErrorWire({ code: 'weird', message: 'x' })).toBe(false);
    expect(isErrorWire({ code: 'io' })).toBe(false);
    expect(isErrorWire(null)).toBe(false);
    expect(isErrorWire('io')).toBe(false);
  });

  it('maps wire errors and keeps data', () => {
    const io = fromWireError(
      { code: 'io', message: 'disk full', data: { path: '/x' } },
      'fs_write_text',
    );
    expect(io).toBeInstanceOf(OwTauriError);
    expect(io).toMatchObject({ code: 'io', message: 'disk full', data: { path: '/x' } });
    expect(isPreCommandRejection(io)).toBe(false);
    const plain = fromWireError({ code: 'not-found', message: 'gone' }, 'window_load');
    expect(plain.data).toBeUndefined();
    const unsupported = fromWireError(
      { code: 'unsupported', message: 'not on this OS' },
      'dialog_open',
    );
    expect(unsupported).toBeInstanceOf(OwTauriUnsupportedError);
    expect((unsupported as OwTauriUnsupportedError).api).toBe('dialog_open');
    const same = new OwTauriError('backend', 'b');
    expect(fromWireError(same, 'x')).toBe(same);
  });

  it('maps rejections Tauri produced before the command ran', () => {
    const denied = fromWireError(
      'overwolf.window_create not allowed. Command not found',
      'window_create',
    );
    expect(denied.code).toBe('forbidden');
    expect(isPreCommandRejection(denied)).toBe(true);
    expect(denied.message).toContain('window_create was rejected by Tauri');
    const invalid = fromWireError({ some: 'thing' }, 'ipc_invoke');
    expect(invalid.code).toBe('invalid-argument');
    expect(invalid.data).toEqual({ raw: '{"some":"thing"}' });
    expect(fromWireError(new Error('missing field `id`'), 'x').data).toEqual({
      raw: 'missing field `id`',
    });
    const cyclic: Record<string, unknown> = {};
    cyclic['self'] = cyclic;
    expect(fromWireError(cyclic, 'x').data).toEqual({ raw: '[object Object]' });
    expect(fromWireError(undefined, 'x').data).toEqual({ raw: 'undefined' });
  });

  it('describes thrown values', () => {
    expect(describeThrown(new TypeError('t'))).toEqual({ name: 'TypeError', message: 't' });
    expect(describeThrown({ name: 'Custom', message: 'c' })).toEqual({
      name: 'Custom',
      message: 'c',
    });
    expect(describeThrown({ message: 'm' })).toEqual({ name: 'Error', message: 'm' });
    expect(describeThrown(42)).toEqual({ name: 'Error', message: '42' });
    expect(describeThrown('s')).toEqual({ name: 'Error', message: 's' });
  });
});

describe('v8Stack', () => {
  it('keeps a V8 stack, which starts with the message line', () => {
    const error = new Error('offline');
    expect(v8Stack(error)).toBe(error.stack);
    expect(v8Stack(error).split('\n')[0]).toBe('Error: offline');
  });

  it('adds the message line to a JavaScriptCore stack (frames only)', () => {
    const error = new OwTauriError('network', 'offline');
    for (const frames of [
      'Re@user-script:11:3:28349\nraw@user-script:11:3:44746',
      '@tauri://localhost/index.js:1:20',
      'forEach@[native code]\nglobal code@tauri://localhost/a.js:3:4',
      'checkForUpdates@\nmain@tauri://localhost/a.js:3:4',
    ]) {
      error.stack = frames;
      expect(v8Stack(error)).toBe(`OwTauriError: offline\n${frames}`);
    }
  });

  it('uses the string form without a stack', () => {
    const error = new TypeError('bad');
    Reflect.deleteProperty(error, 'stack');
    expect(v8Stack(error)).toBe('TypeError: bad');
    error.stack = '';
    expect(v8Stack(error)).toBe('TypeError: bad');
  });
});
