import { afterEach, describe, expect, it, vi } from 'vitest';

import { ERROR_CODES, OverwolfError, isOverwolfErrorWire, toOverwolfError } from './errors.js';
import {
  clearUserEmailHashes,
  disableAdsFPD,
  disableAdsOptimization,
  disableAnonymousAnalytics,
  generateUserEmailHashes,
  getInfo,
  getMachineIds,
  isCMPRequired,
  openAdPrivacySettingsWindow,
  openCMPWindow,
  setAnalyticsUserEnabled,
  setAnonymousAnalyticsPreference,
  setExternalPaymentUserId,
  setUserEmailHashes,
  setWindowName,
} from './index.js';
import {
  RUNTIME_VERSION,
  call,
  currentWebviewLabel,
  inTauri,
  isReservedLabel,
  log,
  warnOnce,
} from './internal.js';
import { DEFAULT_INFO, mockOverwolf, settle, type MockOverwolf } from './testing/index.js';
import { Update, check } from './updater.js';

/** `version` of package.json, defined by vitest.config.ts (no Node modules in this package). */
declare const __PACKAGE_VERSION__: string;

let overwolf: MockOverwolf | undefined;

afterEach(() => {
  overwolf?.restore();
  overwolf = undefined;
});

describe('OverwolfError', () => {
  it('carries a code and data, and is recognised across bundles by its brand', () => {
    const error = new OverwolfError('not-found', 'gone', { data: { id: 1 }, cause: 'x' });
    expect(error).toBeInstanceOf(Error);
    expect(error.name).toBe('OverwolfError');
    expect(error.code).toBe('not-found');
    expect(error.data).toEqual({ id: 1 });
    expect(error.cause).toBe('x');
    // A copy of the class from another bundle brands its errors the same way.
    const foreign = Object.defineProperty(
      new Error('other copy'),
      Symbol.for('tauri-plugin-overwolf.error'),
      {
        value: true,
      },
    );
    expect(foreign instanceof OverwolfError).toBe(true);
    expect(new Error('plain') instanceof OverwolfError).toBe(false);
    const nothing: unknown = null;
    expect(nothing instanceof OverwolfError).toBe(false);
    expect(new OverwolfError('io', 'no cause').cause).toBeUndefined();
  });

  it('lists the wire codes', () => {
    expect(ERROR_CODES).toEqual([
      'unsupported',
      'invalid-argument',
      'not-found',
      'forbidden',
      'io',
      'network',
      'verification',
      'backend',
      'config',
      'tauri',
    ]);
  });
});

describe('isOverwolfErrorWire / toOverwolfError', () => {
  it('recognises the plugin error shape', () => {
    expect(isOverwolfErrorWire({ code: 'io', message: 'm' })).toBe(true);
    expect(isOverwolfErrorWire({ code: 'nope', message: 'm' })).toBe(false);
    expect(isOverwolfErrorWire({ code: 'io' })).toBe(false);
    expect(isOverwolfErrorWire('io')).toBe(false);
    expect(isOverwolfErrorWire(null)).toBe(false);
  });

  it('keeps the code, message and data of a plugin error', () => {
    const error = toOverwolfError(
      { code: 'config', message: 'bad', data: { key: 'x' } },
      'get_info',
    );
    expect(error).toMatchObject({ code: 'config', message: 'bad', data: { key: 'x' } });
    expect(toOverwolfError({ code: 'io', message: 'm' }, 'c').data).toBeUndefined();
    expect(toOverwolfError(error, 'other')).toBe(error);
  });

  it('maps a Tauri denial to forbidden and anything else to tauri', () => {
    const denied = toOverwolfError(
      'overwolf.get_machine_ids not allowed. Permissions associated with this command: overwolf:allow-get-machine-ids',
      'get_machine_ids',
    );
    expect(denied.code).toBe('forbidden');
    expect(denied.message).toContain('get_machine_ids was rejected by Tauri');
    expect(denied.data).toEqual({ raw: expect.stringContaining('not allowed') as unknown });
    expect(toOverwolfError(new Error('boom'), 'x')).toMatchObject({
      code: 'tauri',
      data: { raw: 'boom' },
    });
    expect(toOverwolfError({ odd: 1 }, 'x').data).toEqual({ raw: '{"odd":1}' });
    expect(toOverwolfError(undefined, 'x').data).toEqual({ raw: 'undefined' });
    const cyclic: Record<string, unknown> = {};
    cyclic['self'] = cyclic;
    expect(toOverwolfError(cyclic, 'x').data).toEqual({ raw: '[object Object]' });
  });
});

describe('internal helpers', () => {
  it('keeps the runtime version in step with the package version', () => {
    expect(RUNTIME_VERSION).toBe(__PACKAGE_VERSION__);
  });

  it('recognises the labels the plugin reserves', () => {
    expect(isReservedLabel('owad-1')).toBe(true);
    expect(isReservedLabel('ow-cmp')).toBe(true);
    expect(isReservedLabel('ow-cmp-2')).toBe(true);
    expect(isReservedLabel('main')).toBe(false);
    expect(isReservedLabel('owadview')).toBe(false);
  });

  it('reads the current webview label and whether Tauri is present', () => {
    expect(inTauri({})).toBe(false);
    expect(inTauri({ __TAURI_INTERNALS__: null })).toBe(false);
    expect(inTauri({ __TAURI_INTERNALS__: {} })).toBe(true);
    expect(currentWebviewLabel({})).toBeUndefined();
    expect(currentWebviewLabel({ __TAURI_INTERNALS__: { metadata: {} } })).toBeUndefined();
    expect(
      currentWebviewLabel({ __TAURI_INTERNALS__: { metadata: { currentWebview: { label: 7 } } } }),
    ).toBeUndefined();
    expect(
      currentWebviewLabel({
        __TAURI_INTERNALS__: { metadata: { currentWebview: { label: 'main' } } },
      }),
    ).toBe('main');
  });

  it('logs with the plugin prefix and warns once per key', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => undefined);
    log('debug', 'hello');
    expect(debug).toHaveBeenCalledWith('[tauri-plugin-overwolf] hello');
    warnOnce('test:key', 'first');
    warnOnce('test:key', 'second');
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalledWith('[tauri-plugin-overwolf] first');
  });

  it('call() prefixes the command and maps rejections', async () => {
    overwolf = mockOverwolf();
    await expect(call('get_info')).resolves.toEqual(DEFAULT_INFO);
    await expect(call('no_such_command')).rejects.toMatchObject({ code: 'unsupported' });
    expect(overwolf.calls.map((c) => c.command)).toEqual(['get_info', 'no_such_command']);
  });
});

describe('the plugin API', () => {
  it('tells undefined from null in setUserEmailHashes on the wire (L1)', async () => {
    overwolf = mockOverwolf();
    await setUserEmailHashes();
    await setUserEmailHashes(undefined);
    await setUserEmailHashes(null);
    await setUserEmailHashes('');
    await setUserEmailHashes({});
    expect(overwolf.calls.map((c) => JSON.stringify(c.args))).toEqual([
      '{"hashes":{}}',
      '{"hashes":{}}',
      '{"hashes":{"value":null}}',
      '{"hashes":{"value":""}}',
      '{"hashes":{"value":{}}}',
    ]);
  });

  it('passes each call through with its wire arguments', async () => {
    overwolf = mockOverwolf({
      info: { name: 'Sample' },
      machineIds: { muid: 'm1', muidV2: 'm2' },
      cmpRequired: true,
    });
    await expect(getInfo()).resolves.toMatchObject({ name: 'Sample', testAd: true });
    await expect(getMachineIds()).resolves.toEqual({ muid: 'm1', muidV2: 'm2' });
    await expect(isCMPRequired()).resolves.toBe(true);
    await openAdPrivacySettingsWindow();
    await openAdPrivacySettingsWindow({ tab: 'purposes' });
    await openCMPWindow({ tab: 'vendors' });
    await expect(generateUserEmailHashes('a@example.com')).resolves.toHaveProperty('sha256');
    await setUserEmailHashes();
    await setUserEmailHashes({ sha1: 'a', md5: 'b', sha256: 'c' });
    await clearUserEmailHashes();
    await disableAnonymousAnalytics();
    await disableAdsOptimization();
    await disableAdsFPD();
    await setAnonymousAnalyticsPreference(false);
    await setExternalPaymentUserId({ userId: 'u1' });
    await setAnalyticsUserEnabled(true);
    await setWindowName('Main window');
    expect(overwolf.calls).toEqual([
      { command: 'get_info', args: {} },
      { command: 'get_machine_ids', args: {} },
      { command: 'is_cmp_required', args: {} },
      { command: 'open_ad_privacy_settings_window', args: { options: null } },
      { command: 'open_ad_privacy_settings_window', args: { options: { tab: 'purposes' } } },
      { command: 'open_cmp_window', args: { options: { tab: 'vendors' } } },
      { command: 'generate_user_email_hashes', args: { email: 'a@example.com' } },
      { command: 'set_user_email_hashes', args: { hashes: { value: undefined } } },
      {
        command: 'set_user_email_hashes',
        args: { hashes: { value: { sha1: 'a', md5: 'b', sha256: 'c' } } },
      },
      { command: 'clear_user_email_hashes', args: {} },
      { command: 'disable_anonymous_analytics', args: {} },
      { command: 'disable_ads_optimization', args: {} },
      { command: 'disable_ads_fpd', args: {} },
      { command: 'set_anonymous_analytics_preference', args: { enabled: false } },
      { command: 'set_external_payment_user_id', args: { options: { userId: 'u1' } } },
      { command: 'set_analytics_user_enabled', args: { enabled: true } },
      { command: 'set_window_name', args: { name: 'Main window' } },
    ]);
  });

  it('isCMPRequired() answers true when the plugin cannot tell', async () => {
    overwolf = mockOverwolf({
      commands: {
        is_cmp_required: () => {
          throw { code: 'backend', message: 'no answer' };
        },
      },
    });
    await expect(isCMPRequired()).resolves.toBe(true);
    overwolf.setCommand('is_cmp_required', () => false);
    await expect(isCMPRequired()).resolves.toBe(false);
  });

  it('rejects with the OverwolfError of a denied command', async () => {
    overwolf = mockOverwolf({
      commands: {
        get_machine_ids: () => {
          throw new Error('overwolf.get_machine_ids not allowed. Command not found');
        },
      },
    });
    await expect(getMachineIds()).rejects.toMatchObject({ code: 'forbidden' });
  });
});

describe('updater', () => {
  const metadata = {
    rid: 42,
    version: '1.1.0',
    currentVersion: '1.0.0',
    date: '2026-01-02T00:00:00Z',
    body: 'notes',
    raw: { version: '1.1.0', path: 'Setup.exe' },
  };

  it('check() answers null when up to date and an Update otherwise', async () => {
    overwolf = mockOverwolf();
    await expect(check()).resolves.toBeNull();
    overwolf.setCommand('updater_check', () => metadata);
    const update = await check({ channel: 'beta', allowPrerelease: true });
    expect(update).toBeInstanceOf(Update);
    expect(update).toMatchObject({
      rid: 42,
      version: '1.1.0',
      currentVersion: '1.0.0',
      date: metadata.date,
      body: 'notes',
      raw: metadata.raw,
    });
    expect(overwolf.callsOf('updater_check')).toEqual([
      { options: null },
      { options: { channel: 'beta', allowPrerelease: true } },
    ]);
  });

  it('leaves date and body out when the feed has none', async () => {
    overwolf = mockOverwolf({
      commands: { updater_check: () => ({ ...metadata, date: null, body: null }) },
    });
    const update = await check();
    expect(update?.date).toBeUndefined();
    expect(update?.body).toBeUndefined();
  });

  it('downloads with progress over a channel, then installs', async () => {
    overwolf = mockOverwolf({ commands: { updater_check: () => metadata } });
    const internals = Reflect.get(globalThis, '__TAURI_INTERNALS__') as {
      runCallback(id: number, data: unknown): void;
    };
    const progress = (args: Record<string, unknown>): null => {
      const { id } = args['onEvent'] as { id: number };
      internals.runCallback(id, {
        message: { event: 'Started', data: { contentLength: 10 } },
        index: 0,
      });
      internals.runCallback(id, {
        message: { event: 'Progress', data: { chunkLength: 10 } },
        index: 1,
      });
      internals.runCallback(id, { message: { event: 'Finished' }, index: 2 });
      return null;
    };
    overwolf.setCommand('updater_download', progress);
    overwolf.setCommand('updater_download_and_install', progress);
    overwolf.setCommand('updater_install', () => null);
    const update = await check();
    const events: string[] = [];
    await update?.download((event) => events.push(event.event));
    await update?.install();
    await update?.downloadAndInstall();
    await settle();
    expect(events).toEqual(['Started', 'Progress', 'Finished']);
    expect(overwolf.calls.map((c) => c.command)).toEqual([
      'updater_check',
      'updater_download',
      'updater_install',
      'updater_download_and_install',
    ]);
    expect(overwolf.callsOf('updater_install')).toEqual([{ rid: 42 }]);
  });

  it('rejects with unsupported where the plugin has no updater', async () => {
    overwolf = mockOverwolf({
      commands: {
        updater_check: () => {
          throw { code: 'unsupported', message: 'the updater is Windows only' };
        },
      },
    });
    await expect(check()).rejects.toMatchObject({ code: 'unsupported' });
  });
});

describe('mockOverwolf', () => {
  it('rejects commands outside the plugin and unmocked plugin commands', async () => {
    overwolf = mockOverwolf();
    const { invoke } = await import('@tauri-apps/api/core');
    await expect(invoke('plugin:other|x')).rejects.toBeDefined();
    await expect(call('adview_event')).rejects.toMatchObject({ code: 'unsupported' });
  });

  it('tracks mounts, emits on their channel and forgets them on unmount', async () => {
    overwolf = mockOverwolf();
    const { Channel } = await import('@tauri-apps/api/core');
    const channel = new Channel<unknown>();
    const seen: unknown[] = [];
    channel.onmessage = (message) => seen.push(message);
    await expect(
      call('adview_mount', { request: { elementId: 'e1' }, onEvent: channel }),
    ).resolves.toEqual({ guestLabel: 'owad-1' });
    await expect(call('adview_mount', { request: { elementId: 'e2' } })).rejects.toMatchObject({
      code: 'invalid-argument',
    });
    expect(overwolf.mounts()).toEqual([
      { elementId: 'e1', guestLabel: 'owad-1', request: { elementId: 'e1' } },
    ]);
    overwolf.emit('e1', 'impression', { n: 1 });
    overwolf.emit('e1', 'destroyed', undefined, 'host');
    await settle();
    expect(seen).toEqual([
      { name: 'impression', data: { n: 1 }, source: 'guest' },
      { name: 'destroyed', source: 'host' },
    ]);
    await call('adview_unmount', { elementId: 'e1' });
    expect(overwolf.mounts()).toEqual([]);
    expect(() => {
      overwolf?.emit('e1', 'impression');
    }).toThrow('no mounted <owadview>');
    await expect(call('adview_update', { request: {} })).resolves.toBeNull();
    await expect(call('adview_command', {})).resolves.toBeNull();
  });
});
