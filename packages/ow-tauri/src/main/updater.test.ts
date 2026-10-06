import { afterEach, describe, expect, it, vi } from 'vitest';

import { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
import {
  defaultSnapshot,
  mockHost,
  setHostContext,
  settle,
  type MockHost,
} from '../testing/index.js';
import { autoUpdater } from './index.js';
import type { UpdateInfo, UpdaterLogger } from './updater.js';

let host: MockHost | undefined;

afterEach(() => {
  setHostContext(null);
  host?.dispose();
  host = undefined;
});

const FEED = 'https://updates.example.com/feed';

const info: UpdateInfo = {
  version: '2.0.0',
  files: [{ url: 'setup.exe', sha512: 'abc', size: 10 }],
  releaseDate: '2026-01-01T00:00:00.000Z',
};

function logger(): UpdaterLogger & { lines: string[] } {
  const lines: string[] = [];
  return {
    lines,
    info: (m?: unknown) => lines.push(`info ${String(m)}`),
    warn: (m?: unknown) => lines.push(`warn ${String(m)}`),
    error: (m?: unknown) => lines.push(`error ${String(m)}`),
  };
}

async function start(
  options: Parameters<typeof mockHost>[0] = {},
  packaged = true,
): Promise<MockHost> {
  host = mockHost({ ...options, snapshot: { isPackaged: packaged, ...options.snapshot } });
  await settle();
  host.clearCalls();
  return host;
}

describe('autoUpdater properties (I.1, I.5)', () => {
  it('has electron-updater defaults and the channel side effect', async () => {
    await start();
    expect(autoUpdater.autoDownload).toBe(true);
    expect(autoUpdater.autoInstallOnAppQuit).toBe(true);
    expect(autoUpdater.allowDowngrade).toBe(false);
    expect(autoUpdater.allowPrerelease).toBe(false);
    expect(autoUpdater.forceDevUpdateConfig).toBe(false);
    expect(autoUpdater.channel).toBeNull();
    expect(autoUpdater.logger).toBe(console);
    autoUpdater.channel = 'beta';
    expect(autoUpdater.allowDowngrade).toBe(true);
    autoUpdater.allowDowngrade = false;
    expect(autoUpdater.allowDowngrade).toBe(false);
    expect(() => {
      autoUpdater.channel = '';
    }).toThrow(OwTauriError);
    expect(() => {
      autoUpdater.channel = null;
    }).toThrow(/Channel must be a string/);
    autoUpdater.logger = null;
    expect(autoUpdater.logger).toBeNull();
  });

  it('reports currentVersion from the manifest', async () => {
    await start();
    const current = autoUpdater.currentVersion;
    expect(current.version).toBe('1.0.0');
    expect(String(current)).toBe('1.0.0');
    expect([current.major, current.minor, current.patch]).toEqual([1, 0, 0]);
    host!.dispose();
    host = mockHost({
      snapshot: {
        manifest: { ...defaultSnapshot().manifest, version: '3.4.5-beta.2' },
      },
    });
    expect(autoUpdater.currentVersion).toMatchObject({
      major: 3,
      minor: 4,
      patch: 5,
      prerelease: ['beta', 2],
    });
  });

  it('rejects providers other than generic', async () => {
    await start();
    expect(() => {
      autoUpdater.setFeedURL({ provider: 'github' } as never);
    }).toThrow(OwTauriUnsupportedError);
    expect(() => {
      autoUpdater.setFeedURL({ provider: 'generic', url: '' });
    }).toThrow(OwTauriError);
  });
});

describe('autoUpdater commands (A.2.8)', () => {
  it('skips the check in an unpackaged build unless forceDevUpdateConfig is set', async () => {
    await start({ commands: { updater_check: () => ({ isUpdateAvailable: false }) } }, false);
    const log = logger();
    autoUpdater.logger = log;
    autoUpdater.setFeedURL(FEED);
    expect(await autoUpdater.checkForUpdates()).toBeNull();
    expect(log.lines).toEqual([
      'info Skip checkForUpdates because application is not packed and dev update config is not forced',
    ]);
    expect(host!.callsOf('updater_check')).toHaveLength(0);
    autoUpdater.forceDevUpdateConfig = true;
    expect(await autoUpdater.checkForUpdates()).toEqual({ isUpdateAvailable: false });
    expect(host!.callsOf('updater_configure').at(-1)).toMatchObject({
      forceDevUpdateConfig: true,
      url: FEED,
    });
  });

  it('sends the configuration before checking, downloading and installing', async () => {
    const result = { isUpdateAvailable: true, updateInfo: info, versionInfo: info };
    await start({
      commands: {
        updater_check: () => result,
        updater_download: () => ['/cache/setup.exe', 3],
      },
    });
    autoUpdater.logger = null;
    autoUpdater.autoDownload = false;
    autoUpdater.channel = 'testing';
    autoUpdater.allowDowngrade = false;
    autoUpdater.setFeedURL({
      provider: 'generic',
      url: FEED,
      channel: 'ignored',
      requestHeaders: { 'X-Test': '1' },
    });
    expect(await autoUpdater.checkForUpdatesAndNotify()).toEqual(result);
    expect(await autoUpdater.downloadUpdate()).toEqual(['/cache/setup.exe']);
    autoUpdater.quitAndInstall(true, true);
    await settle();
    expect(host!.callsOf('updater_configure')).toEqual([
      {
        provider: 'generic',
        url: FEED,
        channel: 'testing',
        allowDowngrade: false,
        allowPrerelease: false,
        autoDownload: false,
        autoInstallOnAppQuit: true,
        forceDevUpdateConfig: false,
        requestHeaders: { 'X-Test': '1' },
      },
    ]);
    expect(host!.callsOf('updater_quit_and_install')).toEqual([
      { isSilent: true, isForceRunAfter: true },
    ]);
    const order = host!.calls.map((c) => c.command).filter((c) => c.startsWith('updater_'));
    expect(order).toEqual([
      'updater_configure',
      'updater_check',
      'updater_download',
      'updater_quit_and_install',
    ]);
  });

  it('uses the feed channel when channel is not set and re-sends changed properties', async () => {
    await start({ commands: { updater_check: () => null } });
    autoUpdater.logger = null;
    autoUpdater.setFeedURL({ provider: 'generic', url: FEED, channel: 'feed' });
    await settle();
    expect(host!.callsOf('updater_configure')).toEqual([
      expect.objectContaining({ channel: 'feed' }),
    ]);
    autoUpdater.autoInstallOnAppQuit = false;
    autoUpdater.allowPrerelease = true;
    await settle();
    expect(host!.callsOf('updater_configure')).toHaveLength(2);
    expect(host!.callsOf('updater_configure')[1]).toMatchObject({
      autoInstallOnAppQuit: false,
      allowPrerelease: true,
    });
    expect(await autoUpdater.checkForUpdates()).toBeNull();
    expect(host!.callsOf('updater_configure')).toHaveLength(2);
  });

  it('fails without a feed and emits error', async () => {
    await start();
    const log = logger();
    autoUpdater.logger = log;
    const errors: unknown[] = [];
    autoUpdater.on('error', (e: unknown) => errors.push(e));
    await expect(autoUpdater.checkForUpdates()).rejects.toMatchObject({
      code: 'invalid-argument',
    });
    await expect(autoUpdater.downloadUpdate()).rejects.toThrow(/setFeedURL/);
    expect(errors).toHaveLength(2);
    expect(log.lines[0]).toBe('info Checking for update');
    expect(log.lines[1]).toMatch(/^error Error: /);
  });

  it('emits error for a call Tauri rejected, not for one the plugin reported', async () => {
    await start({
      commands: {
        updater_check: () => Promise.reject('Command updater_check not found'),
        updater_download: () => Promise.reject({ code: 'network', message: 'offline' }),
        updater_quit_and_install: () =>
          Promise.reject({ code: 'unsupported', message: 'no pubkey' }),
      },
    });
    autoUpdater.logger = null;
    autoUpdater.setFeedURL(FEED);
    const errors: Error[] = [];
    autoUpdater.on('error', (e: Error) => errors.push(e));
    await expect(autoUpdater.checkForUpdates()).rejects.toMatchObject({
      code: 'invalid-argument',
    });
    expect(errors).toHaveLength(1);
    await expect(autoUpdater.downloadUpdate()).rejects.toMatchObject({ code: 'network' });
    expect(errors).toHaveLength(1);
    autoUpdater.quitAndInstall();
    await settle();
    expect(errors).toHaveLength(2);
    expect(errors[1]).toBeInstanceOf(OwTauriUnsupportedError);
  });

  it('emits error and rejects when updater_configure fails', async () => {
    await start({
      commands: {
        updater_configure: () => Promise.reject({ code: 'invalid-argument', message: 'bad url' }),
      },
    });
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    autoUpdater.logger = null;
    autoUpdater.setFeedURL('http://insecure.example.com');
    const errors: unknown[] = [];
    autoUpdater.on('error', (e: unknown) => errors.push(e));
    await expect(autoUpdater.checkForUpdates()).rejects.toMatchObject({ message: 'bad url' });
    expect(errors).toHaveLength(1);
    host!.setCommand('updater_configure', () => null);
    host!.setCommand('updater_check', () => null);
    expect(await autoUpdater.checkForUpdates()).toBeNull();
  });

  it('returns the check in progress instead of checking twice', async () => {
    let resolve: (value: unknown) => void = () => undefined;
    const result = { isUpdateAvailable: false, updateInfo: info, versionInfo: info };
    await start({
      commands: {
        updater_check: () =>
          new Promise((r) => {
            resolve = r;
          }),
      },
    });
    const log = logger();
    autoUpdater.logger = log;
    autoUpdater.setFeedURL(FEED);
    const first = autoUpdater.checkForUpdates();
    const second = autoUpdater.checkForUpdatesAndNotify();
    expect(second).toBe(first);
    await settle();
    expect(host!.callsOf('updater_check')).toHaveLength(1);
    resolve(result);
    expect(await first).toEqual(result);
    expect(log.lines).toEqual([
      'info Checking for update',
      'info Checking for update (already in progress)',
    ]);
    host!.setCommand('updater_check', () => result);
    await autoUpdater.checkForUpdates();
    expect(host!.callsOf('updater_check')).toHaveLength(2);
  });

  it('prefixes a failed check with "Cannot check for updates: "', async () => {
    await start({
      commands: {
        updater_check: () => Promise.reject({ code: 'network', message: 'offline' }),
        updater_download: () => Promise.reject({ code: 'network', message: 'offline' }),
      },
    });
    autoUpdater.logger = null;
    const messages: string[] = [];
    autoUpdater.on('error', (_e: Error, message: string) => messages.push(message));
    // No feed: the facade reports the failure itself.
    await expect(autoUpdater.checkForUpdates()).rejects.toThrow(/setFeedURL/);
    expect(messages[0]).toMatch(/^Cannot check for updates: .*setFeedURL/s);
    // The plugin reports the failure as an `error` message, after or before
    // the rejection.
    autoUpdater.setFeedURL(FEED);
    await expect(autoUpdater.checkForUpdates()).rejects.toMatchObject({ code: 'network' });
    host!.push({ type: 'updater', event: 'error', error: { code: 'network', message: 'offline' } });
    await settle();
    expect(messages[1]).toMatch(/^Cannot check for updates: .*offline/s);
    // Later errors (a download) keep electron-updater's plain message.
    await expect(autoUpdater.downloadUpdate()).rejects.toMatchObject({ code: 'network' });
    host!.push({ type: 'updater', event: 'error', error: { code: 'network', message: 'offline' } });
    await settle();
    expect(messages).toHaveLength(3);
    expect(messages[2]).not.toMatch(/^Cannot check/);
    // A check that found an update: a following download error is plain.
    host!.setCommand('updater_check', () => ({ isUpdateAvailable: true }));
    await autoUpdater.checkForUpdates();
    host!.push(
      { type: 'updater', event: 'update-available', info },
      { type: 'updater', event: 'error', error: { code: 'backend', message: 'bad hash' } },
    );
    await settle();
    expect(messages[3]).not.toMatch(/^Cannot check/);
    // A disabled updater (`null`, no event) leaves nothing pending.
    host!.setCommand('updater_check', () => null);
    expect(await autoUpdater.checkForUpdates()).toBeNull();
    host!.push({ type: 'updater', event: 'error', error: { code: 'backend', message: 'x' } });
    await settle();
    expect(messages[4]).not.toMatch(/^Cannot check/);
  });

  it('is main-only', async () => {
    await start();
    setHostContext('ui');
    expect(() => {
      autoUpdater.setFeedURL(FEED);
    }).toThrow(OwTauriError);
    await expect(autoUpdater.checkForUpdates()).rejects.toMatchObject({ code: 'forbidden' });
  });
});

describe('autoUpdater events (I.3)', () => {
  it('re-emits updater host messages and logs them', async () => {
    await start();
    const log = logger();
    autoUpdater.logger = log;
    const seen: unknown[][] = [];
    for (const name of [
      'checking-for-update',
      'update-available',
      'update-not-available',
      'download-progress',
      'update-downloaded',
      'error',
    ]) {
      autoUpdater.on(name, (...args: unknown[]) => seen.push([name, ...args]));
    }
    const progress = { percent: 50, bytesPerSecond: 10, total: 100, transferred: 50 };
    host!.push(
      { type: 'updater', event: 'checking-for-update' },
      { type: 'updater', event: 'update-available', info },
      { type: 'updater', event: 'update-not-available', info },
      { type: 'updater', event: 'download-progress', progress },
      {
        type: 'updater',
        event: 'update-downloaded',
        info: { ...info, downloadedFile: '/c/s.exe' },
      },
      { type: 'updater', event: 'error', error: { code: 'backend', message: 'bad signature' } },
      { type: 'updater', event: 'unknown' },
    );
    await settle();
    expect(seen.map((s) => s[0])).toEqual([
      'checking-for-update',
      'update-available',
      'update-not-available',
      'download-progress',
      'update-downloaded',
      'error',
    ]);
    expect(seen[1]![1]).toEqual(info);
    expect(seen[3]![1]).toEqual(progress);
    const error = seen[5]![1] as OwTauriError;
    expect(error).toBeInstanceOf(OwTauriError);
    expect(error.code).toBe('backend');
    expect(typeof seen[5]![2]).toBe('string');
    expect(log.lines).toEqual([
      'info Found version 2.0.0 (url: setup.exe)',
      'info Update for version 1.0.0 is not available (latest version: 2.0.0, downgrade is disallowed).',
      'info New version 2.0.0 has been downloaded to /c/s.exe',
      expect.stringMatching(/^error Error: .*bad signature/) as string,
    ]);
  });

  it('does not throw for an unhandled error event', async () => {
    await start();
    autoUpdater.logger = null;
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    host!.push({ type: 'updater', event: 'error', error: { code: 'network', message: 'x' } });
    await settle();
    expect(warn).toHaveBeenCalled();
  });
});
