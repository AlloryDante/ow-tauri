import { afterEach, describe, expect, it } from 'vitest';

import { attachTo } from '../bootstrap/install.js';
import * as electronEntry from '../electron/index.js';
import * as rendererEntry from '../renderer/index.js';
import { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
import { CONTRACT_VERSION } from '../shared/protocol.js';
import { mockHost, setHostContext, settle, type MockHost } from '../testing/index.js';
import { RecorderError, files, whenHostReady } from './index.js';
import { rebuildPackageError } from './recorder-error.js';

let host: MockHost | undefined;

afterEach(() => {
  setHostContext(null);
  host?.dispose();
  host = undefined;
});

describe('ow-tauri/main (B.1)', () => {
  it('resolves whenHostReady after the readiness commands', async () => {
    host = mockHost();
    await whenHostReady();
    expect(host.callsOf('ipc_main_ready')).toHaveLength(1);
    expect(host.callsOf('main_ready')).toHaveLength(1);
  });

  it('maps files to the fs_* commands (B.1.7)', async () => {
    host = mockHost({
      commands: {
        fs_read_text: (args) => (args['path'] === '/data/a.txt' ? 'text' : null),
        fs_exists: () => true,
      },
    });
    await settle();
    expect(await files.readText('/data/a.txt')).toBe('text');
    expect(await files.readText('/data/none.txt')).toBeNull();
    await files.writeText('/data/b.txt', 'b');
    expect(await files.exists('/data')).toBe(true);
    await files.mkdir('/data/dir', { recursive: true });
    await files.mkdir('/data/dir2');
    expect(host.callsOf('fs_write_text')).toEqual([{ path: '/data/b.txt', data: 'b' }]);
    expect(host.callsOf('fs_mkdir')).toEqual([
      { path: '/data/dir', recursive: true },
      { path: '/data/dir2' },
    ]);
    setHostContext('ui');
    await expect(files.exists('/x')).rejects.toMatchObject({ code: 'forbidden' });
    expect(() => whenHostReady()).toThrow(OwTauriError);
  });

  it('rebuilds package errors (B.1.5)', () => {
    const recorder = rebuildPackageError({
      $kind: 'RecorderError',
      message: 'busy',
      code: 3,
      codeStr: 'BUSY',
      internalError: { name: 'TypeError', message: 'inner' },
    }) as RecorderError;
    expect(recorder).toBeInstanceOf(RecorderError);
    expect(recorder).toBeInstanceOf(Error);
    expect(recorder).toMatchObject({
      name: 'RecorderError',
      message: 'busy',
      code: 3,
      codeStr: 'BUSY',
    });
    expect(recorder.internalError?.name).toBe('TypeError');
    expect(
      (rebuildPackageError({ $kind: 'RecorderError' }) as RecorderError).internalError,
    ).toBeUndefined();
    const utility = rebuildPackageError({ $kind: 'UtilityApiError', message: 'm', exitCode: 2 });
    expect(utility).toEqual({ message: 'm', exitCode: 2 });
    expect(Object.isFrozen(utility)).toBe(true);
    expect(utility).not.toBeInstanceOf(Error);
    expect(rebuildPackageError({ $kind: 'UtilityApiError' })).toEqual({ message: '' });
    const plain = rebuildPackageError({ name: 'RangeError', message: 'r' }) as Error;
    expect(plain).toBeInstanceOf(Error);
    expect(plain.name).toBe('RangeError');
    expect((rebuildPackageError('boom') as Error).message).toBe('boom');
  });
});

describe('entry points share one runtime (B, ADR 0012)', () => {
  it('exports the same objects and the error classes everywhere', () => {
    expect(rendererEntry.ipcRenderer).toBe(electronEntry.ipcRenderer);
    expect(rendererEntry.contextBridge).toBe(electronEntry.contextBridge);
    expect(rendererEntry.OwTauriError).toBe(OwTauriError);
    expect(electronEntry.OwTauriUnsupportedError).toBe(OwTauriUnsupportedError);
  });

  it('refuses a runtime with another contract', () => {
    const kernel = attachTo({
      version: '9.9.9',
      contract: CONTRACT_VERSION + 1,
      api: 1,
      context: 'main',
      evalBegin: () => undefined,
      evalFallback: () => undefined,
    });
    expect(() => {
      kernel.require('main', 'app.quit');
    }).toThrow(/ow-tauri runtime 9\.9\.9 \(contract 2, api 1\) does not match package/);
  });

  it('refuses a runtime whose facade API differs, even with the same contract', () => {
    const kernel = attachTo({
      version: '0.1.1',
      contract: CONTRACT_VERSION,
      api: 99,
      context: 'main',
      evalBegin: () => undefined,
      evalFallback: () => undefined,
    });
    expect(() => {
      kernel.require('main', 'app.quit');
    }).toThrow(/\(contract 1, api 99\) does not match package .* api 1\)/);
  });
});
