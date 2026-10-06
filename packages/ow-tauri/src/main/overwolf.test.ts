import { afterEach, describe, expect, it, vi } from 'vitest';

import { attachRuntime } from '../bootstrap/install.js';
import { MAIN_READY_HOLD_MS } from '../bootstrap/facade-kernel.js';
import { BrowserWindow, app } from '../electron/index.js';
import { OwTauriError } from '../shared/errors.js';
import { mockHost, setHostContext, settle, type MockHost } from '../testing/index.js';
import { overwolf, whenHostReady } from './index.js';
import { NOT_READY_MESSAGE, Overwolf, PAYMENT_ID_MANDATORY } from './overwolf.js';
import { syntheticEvent } from './packages.js';

let host: MockHost | undefined;
const kernel = attachRuntime();

afterEach(() => {
  vi.useRealTimers();
  setHostContext(null);
  host?.dispose();
  host = undefined;
});

/** Resolves when `release()` is called: a command answer the test controls. */
function gate(): { promise: Promise<null>; release: () => void } {
  let release!: () => void;
  const promise = new Promise<null>((resolve) => {
    release = () => {
      resolve(null);
    };
  });
  return { promise, release };
}

function commandOrder(h: MockHost): string[] {
  return h.calls.map((c) => c.command);
}

describe('app.overwolf synchronous members (B.1.1, B.1.6)', () => {
  it('reads identity from the state cache', async () => {
    host = mockHost({
      snapshot: {
        identity: { uid: 'u1', cuid: 'c1', muid: 'm1', muidV2: 'm1', phasePercent: 24 },
        utmParams: { utm_source: 'installer' },
      },
    });
    expect(overwolf.uid).toBe('u1');
    expect(overwolf.muid).toBe('m1');
    expect(overwolf.phasePercent).toBe(24);
    expect(overwolf.utmParams).toEqual({ utm_source: 'installer' });
    host.push({
      type: 'state',
      seq: 1,
      patches: [{ path: 'identity.phasePercent', value: 51 }],
    });
    expect(overwolf.phasePercent).toBe(51);
    await settle();
  });

  it('reports utmParams as undefined, not null, when there are none [OBS]', () => {
    host = mockHost();
    expect(host.snapshot.utmParams).toBeNull();
    expect(overwolf.utmParams).toBeUndefined();
  });

  it('falls back to neutral values when the cache has no identity', () => {
    host = mockHost({ snapshot: { identity: undefined as never } });
    expect(overwolf.uid).toBe('');
    expect(overwolf.muid).toBe('');
    expect(overwolf.phasePercent).toBe(0);
  });

  it('has the members ow-electron has beyond its typings [OBS]', async () => {
    host = mockHost();
    const api = overwolf as unknown as Record<string, unknown>;
    expect(api['enableAdsOptimization']).toBe(false);
    overwolf.disableAdsOptimization();
    expect(api['enableAdsOptimization']).toBe(false);
    expect(overwolf.assureOWElectronIsReady.length).toBe(0);
    expect(overwolf.overrideAdViewUrl.length).toBe(1);
    expect(overwolf.storeEmailHashes.length).toBe(1);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    overwolf.overrideAdViewUrl('https://example.com/');
    overwolf.storeEmailHashes({ sha256: 'a' });
    overwolf.storeEmailHashes({ sha256: 'b' });
    expect(warn).toHaveBeenCalledTimes(2);
    expect(host.callsOf('set_user_email_hashes')).toHaveLength(0);
    const ready = overwolf.assureOWElectronIsReady();
    await whenHostReady();
    await expect(ready).resolves.toBeUndefined();
  });

  it('is the same object as app.overwolf', () => {
    host = mockHost();
    expect(app.overwolf).toBe(overwolf);
  });

  it('throws forbidden outside the main webview, when used', () => {
    host = mockHost({ label: 'bw-1' });
    expect(() => overwolf.uid).toThrow(OwTauriError);
    expect(() => app.overwolf).toThrow(/only available in the main webview/);
    expect(() => {
      overwolf.disableAdsFPD();
    }).toThrow(expect.objectContaining({ code: 'forbidden' }) as Error);
  });
});

describe('__settings__ [OBS]', () => {
  it("has ow-electron's keys, order and values", () => {
    host = mockHost();
    const settings = overwolf.__settings__;
    expect(Object.keys(settings)).toEqual([
      'src',
      'forceSandboxMode',
      'adsOptimization',
      'adsSetting',
      'logger',
      'analytics',
      'firstLaunch',
    ]);
    expect(JSON.parse(JSON.stringify(settings))).toEqual({
      src: 'https://www.overwolf.com/monsdk/electron/latest/adview.html',
      forceSandboxMode: false,
      adsOptimization: {},
      adsSetting: {
        gvlUrlV1: 'https://content.overwolf.com/cmp',
        gvlUrl: 'https://content.overwolf.com/cmp/v3',
        cmpFeatureUrl: 'https://features.overwolf.com/experiments/cmp-eu-only',
        cmpUrl: 'https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/ow-cmp-v2.html',
        cmpSettingUrl: 'https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/cmp.html',
        cmpWindowUrl: '',
      },
      logger: { enabled: false },
      analytics: {
        analyticsUrl: 'https://analyticsnew.overwolf.com/analytics/Counter',
        trackingUrl: 'https://tracking.overwolf.com',
      },
      firstLaunch: false,
    });
    expect(Object.isFrozen(settings)).toBe(true);
    expect(Object.isFrozen(settings.adsSetting)).toBe(true);
    expect(() => {
      (settings as { src: string }).src = 'x';
    }).toThrow(TypeError);
  });

  it('reads firstLaunch from the snapshot', () => {
    host = mockHost({ snapshot: { firstLaunch: true } });
    expect(new Overwolf(kernel).__settings__.firstLaunch).toBe(true);
  });

  it('changes adsOptimization as ow-electron does: FPD sets anonymous, optimisation sets disable', async () => {
    host = mockHost();
    await whenHostReady();
    expect(overwolf.__settings__.adsOptimization).toEqual({});
    overwolf.disableAdsFPD();
    expect(overwolf.__settings__.adsOptimization).toEqual({ anonymous: true });
    overwolf.disableAdsOptimization();
    expect(overwolf.__settings__.adsOptimization).toEqual({ anonymous: true, disable: true });
    expect(Object.isFrozen(overwolf.__settings__.adsOptimization)).toBe(true);
    expect(kernel.state.get('flags.adsFpdDisabled')).toBe(true);
    expect(kernel.state.get('flags.adsOptimizationDisabled')).toBe(true);
    await settle();
    expect(host.callsOf('disable_ads_fpd')).toHaveLength(1);
    expect(host.callsOf('disable_ads_optimization')).toHaveLength(1);
  });
});

describe('opt-outs before app.ready (A.2.2, E.3)', () => {
  it('holds main_ready until the opt-out was acknowledged', async () => {
    const answer = gate();
    host = mockHost({ commands: { disable_anonymous_analytics: () => answer.promise } });
    overwolf.disableAnonymousAnalytics();
    expect(kernel.state.get('flags.anonymousAnalyticsDisabled')).toBe(true);
    await settle(10);
    expect(host.callsOf('main_ready')).toHaveLength(0);
    answer.release();
    await whenHostReady();
    const order = commandOrder(host);
    expect(order.indexOf('disable_anonymous_analytics')).toBeLessThan(order.indexOf('main_ready'));
  });

  it('sends main_ready anyway after the hold limit, with a warning', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    host = mockHost({ commands: { disable_ads_fpd: () => new Promise(() => undefined) } });
    overwolf.disableAdsFPD();
    await vi.advanceTimersByTimeAsync(MAIN_READY_HOLD_MS - 100);
    expect(host.callsOf('main_ready')).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(200);
    expect(host.callsOf('main_ready')).toHaveLength(1);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('main_ready was held'));
  });

  it('does not hold main_ready for calls made after app.ready, and logs failures', async () => {
    host = mockHost({
      commands: {
        disable_anonymous_analytics: () => {
          throw { code: 'backend', message: 'nope' };
        },
      },
    });
    await whenHostReady();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    overwolf.disableAnonymousAnalytics();
    await settle();
    expect(warn).toHaveBeenCalledWith(
      expect.stringContaining('disable_anonymous_analytics failed: nope'),
    );
  });
});

describe('consent (A.2.2, D.6)', () => {
  it('isCMPRequired never rejects', async () => {
    host = mockHost({ commands: { is_cmp_required: () => false } });
    expect(await overwolf.isCMPRequired()).toBe(false);
    host.setCommand('is_cmp_required', () => true);
    expect(await overwolf.isCMPRequired()).toBe(true);
    host.setCommand('is_cmp_required', () => 'unexpected');
    expect(await overwolf.isCMPRequired()).toBe(true);
    host.setCommand('is_cmp_required', () => {
      throw { code: 'network', message: 'offline' };
    });
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    expect(await overwolf.isCMPRequired()).toBe(true);
    setHostContext('ui');
    expect(await overwolf.isCMPRequired()).toBe(true);
  });

  it('opens the settings window with the wire options, parent as its host id', async () => {
    host = mockHost();
    await whenHostReady();
    const parent = new BrowserWindow({ show: false });
    await parent.whenCreated();
    await overwolf.openAdPrivacySettingsWindow({
      tab: 'vendors',
      modal: true,
      parent,
      width: 640,
      language: 'de',
      cmpURL: 'https://content.overwolf.com/x.html',
      unknown: 1,
    } as never);
    await overwolf.openCMPWindow();
    expect(host.callsOf('open_ad_privacy_settings_window')).toEqual([
      {
        options: {
          tab: 'vendors',
          modal: true,
          width: 640,
          cmpURL: 'https://content.overwolf.com/x.html',
          language: 'de',
          parentId: 1,
        },
      },
    ]);
    expect(host.callsOf('open_cmp_window')).toEqual([{ options: null }]);
  });

  it('opens without a parent it cannot resolve, and rejects with plugin errors', async () => {
    host = mockHost();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    await overwolf.openCMPWindow({ parent: { id: 999 } });
    expect(host.callsOf('open_cmp_window')).toEqual([{ options: {} }]);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('opens without a parent'));
    host.setCommand('open_ad_privacy_settings_window', () => {
      throw { code: 'invalid-argument', message: 'cmpURL must be https' };
    });
    await expect(
      overwolf.openAdPrivacySettingsWindow({ cmpURL: 'http://x' }),
    ).rejects.toMatchObject({ code: 'invalid-argument' });
  });
});

describe('email hashes (A.2.2, D.5)', () => {
  it('generateUserEmailHashes is synchronous and broadcasts the result [OBS]', async () => {
    host = mockHost();
    const hashes = overwolf.generateUserEmailHashes('  Test.Email@Overwolf.COM ');
    expect(hashes).toEqual({
      sha1: '2c44f8a418bbfa88e80e3ce17d56cb30944f7675',
      md5: '170d78feecf2b8e7b804ba6b45af7ac2',
      sha256: 'ac43b559f15c2eb262ea8d5d4921f639aaf1cde84bc280bad2e1879d0ded68c2',
    });
    await settle();
    expect(host.callsOf('set_user_email_hashes')).toEqual([{ hashes }]);
  });

  it('returns {} and sends nothing for blank input', async () => {
    host = mockHost();
    expect(overwolf.generateUserEmailHashes('   ')).toEqual({});
    expect(overwolf.generateUserEmailHashes(undefined as never)).toHaveProperty('md5');
    await settle();
    expect(host.callsOf('set_user_email_hashes')).toHaveLength(1);
  });

  it('setUserEmailHashes forwards only the hash fields', async () => {
    host = mockHost();
    overwolf.setUserEmailHashes({ sha256: 'abc', md5: 'def', other: 1 } as never);
    overwolf.setUserEmailHashes();
    await settle();
    expect(host.callsOf('set_user_email_hashes')).toEqual([
      { hashes: { md5: 'def', sha256: 'abc' } },
      { hashes: null },
    ]);
  });
});

describe('setExternalPaymentUserId (A.2.2, E.2) [OBS]', () => {
  it('rejects, never throws, without a user id or before app.ready', async () => {
    host = mockHost();
    const missing = overwolf.setExternalPaymentUserId({ providerName: 'tebex' } as never);
    expect(missing).toBeInstanceOf(Promise);
    await expect(missing).rejects.toThrow(PAYMENT_ID_MANDATORY);
    await expect(missing).rejects.not.toBeInstanceOf(OwTauriError);
    await expect(overwolf.setExternalPaymentUserId(null as never)).rejects.toThrow(
      PAYMENT_ID_MANDATORY,
    );
    await expect(
      overwolf.setExternalPaymentUserId({ providerName: 'tebex', userId: {} as never }),
    ).rejects.toThrow(PAYMENT_ID_MANDATORY);
    await expect(
      overwolf.setExternalPaymentUserId({ providerName: 'tebex', userId: 'u' }),
    ).rejects.toThrow(NOT_READY_MESSAGE);
    expect(host.callsOf('set_external_payment_user_id')).toHaveLength(0);
  });

  it('sends the options and resolves after the report, even when it fails', async () => {
    host = mockHost();
    await whenHostReady();
    await overwolf.setExternalPaymentUserId({ userId: 'u1', paymentId: 'p1' } as never);
    await overwolf.setExternalPaymentUserId({ providerName: 'other', userId: 42 as never });
    await overwolf.setExternalPaymentUserId({
      providerName: '',
      userId: 'u2',
      extra: { a: 1 },
      skipped: () => undefined,
      big: 1n,
      none: undefined,
    } as never);
    await overwolf.setExternalPaymentUserId(JSON.parse('{"__proto__":"x","userId":"u3"}') as never);
    const sent = host
      .callsOf('set_external_payment_user_id')
      .map((args) => (args as { options: Record<string, unknown> }).options);
    expect(sent).toEqual([
      { userId: 'u1', paymentId: 'p1' },
      { providerName: 'other', userId: 42 },
      { userId: 'u2', extra: { a: 1 } },
      JSON.parse('{"__proto__":"x","userId":"u3"}'),
    ]);
    // The app's key order reaches the report [OBS R2-3].
    expect(sent.map((options) => Object.keys(options))).toEqual([
      ['userId', 'paymentId'],
      ['providerName', 'userId'],
      ['userId', 'extra'],
      ['__proto__', 'userId'],
    ]);
    expect(Object.getPrototypeOf(sent[3])).toBe(Object.prototype);
    host.setCommand('set_external_payment_user_id', () => {
      throw { code: 'network', message: 'offline' };
    });
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    await expect(
      overwolf.setExternalPaymentUserId({ providerName: 'tebex', userId: 'u' }),
    ).resolves.toBeUndefined();
  });

  it("turns the plugin's argument and readiness errors into ow-electron's plain errors", async () => {
    host = mockHost();
    await whenHostReady();
    host.setCommand('set_external_payment_user_id', () => {
      throw { code: 'invalid-argument', message: 'bad', data: { message: 'custom text' } };
    });
    const rejected = overwolf.setExternalPaymentUserId({ providerName: 'tebex', userId: 'u' });
    await expect(rejected).rejects.toThrow('custom text');
    await expect(rejected).rejects.not.toBeInstanceOf(OwTauriError);
    host.setCommand('set_external_payment_user_id', () => {
      throw { code: 'not-ready', message: 'x' };
    });
    await expect(
      overwolf.setExternalPaymentUserId({ providerName: 'tebex', userId: 'u' }),
    ).rejects.toThrow(NOT_READY_MESSAGE);
    host.setCommand('set_external_payment_user_id', () => {
      throw { code: 'invalid-argument', message: 'x' };
    });
    await expect(
      overwolf.setExternalPaymentUserId({ providerName: 'tebex', userId: 'u' }),
    ).rejects.toThrow(PAYMENT_ID_MANDATORY);
  });
});

describe('packages (B.1.3, H) [OBS]', () => {
  it('reports unavailable packages exactly as ow-electron does', async () => {
    host = mockHost({
      snapshot: {
        packages: {
          backend: 'none',
          logsFolderPath: '/data/Test App/..\\ow-electron/testuid/logs',
          phasePercent: 24,
          listed: ['gep', 'overlay'],
          pendingUpdates: { hasPendingUpdate: false, details: [] },
        },
      },
    });
    const { packages } = overwolf;
    const pending = packages.hasPendingUpdates();
    expect(pending).toEqual({ hasPendingUpdate: false, details: [] });
    expect(pending).not.toBeInstanceOf(Promise);
    expect(packages.hasPendingUpdates()).not.toBe(pending);
    expect(packages.logsFolderPath).toBe('/data/Test App/..\\ow-electron/testuid/logs');
    expect(packages.phasePercent).toBe(24);
    for (const name of ['gep', 'overlay', 'recorder', 'utility', 'crn'])
      expect((packages as unknown as Record<string, unknown>)[name]).toBeUndefined();
    packages.relaunch();
    expect(await packages.getChannel('gep')).toEqual({});
    expect(await packages.getChannel()).toEqual({});
    const setChannel = packages.setChannel('gep', 'beta');
    expect(setChannel).toBeInstanceOf(Promise);
    await expect(setChannel).rejects.toThrow(
      "setChannel - package 'gep' is not registered in this app",
    );
    await expect(setChannel).rejects.not.toBeInstanceOf(OwTauriError);
    await expect(packages.getAvailableChannels('overlay', 'gep')).rejects.toThrow(
      "getAvailableChannels - package 'overlay' is not registered in this app",
    );
    expect(await packages.getAvailableChannels()).toEqual({});
    await settle();
    expect(host.callsOf('packages_relaunch')).toHaveLength(1);
    expect(host.callsOf('packages_get_available_channels')).toContainEqual({
      names: ['overlay', 'gep'],
    });
  });

  it("uses the plugin's error text when it gives one", async () => {
    host = mockHost({
      commands: {
        packages_set_channel: () => {
          throw { code: 'not-found', message: 'm', data: { message: 'from the plugin' } };
        },
        packages_get_available_channels: () => {
          throw { code: 'not-found', message: 'm', data: { message: 'also from the plugin' } };
        },
        packages_get_channel: () => {
          throw { code: 'backend', message: 'm' };
        },
      },
    });
    const { packages } = overwolf;
    await expect(packages.setChannel('crn')).rejects.toThrow('from the plugin');
    await expect(packages.getAvailableChannels('crn')).rejects.toThrow('also from the plugin');
    expect(await packages.getAvailableChannels()).toEqual({});
    expect(await packages.getChannel('gep')).toEqual({});
  });

  it('passes through the results of a future package runtime', async () => {
    host = mockHost({
      commands: {
        packages_set_channel: () => ({ success: true }),
        packages_get_available_channels: () => ({ gep: ['beta'] }),
        packages_get_channel: () => ({ gep: 'beta' }),
      },
      snapshot: {
        packages: {
          pendingUpdates: {
            hasPendingUpdate: true,
            details: [{ name: 'gep', version: '1.2.3' }, { bad: 1 }],
          },
        },
      },
    });
    const { packages } = overwolf;
    expect(await packages.setChannel('gep', 'beta')).toEqual({ success: true });
    expect(await packages.getAvailableChannels('gep')).toEqual({ gep: ['beta'] });
    expect(await packages.getChannel('gep')).toEqual({ gep: 'beta' });
    expect(packages.hasPendingUpdates()).toEqual({
      hasPendingUpdate: true,
      details: [{ name: 'gep', version: '1.2.3' }],
    });
    host.setCommand('packages_get_available_channels', () => null);
    await expect(packages.getAvailableChannels('gep')).rejects.toThrow(/not registered/);
    host.setCommand('packages_set_channel', () => null);
    await expect(packages.setChannel('gep')).rejects.toThrow(/not registered/);
  });

  it('computes logsFolderPath and phasePercent when the cache lacks them', () => {
    host = mockHost();
    expect(overwolf.packages.logsFolderPath).toBe('/data/Test App/..\\ow-electron/testuid/logs');
    expect(overwolf.packages.phasePercent).toBe(50);
    host.dispose();
    host = mockHost({ snapshot: { paths: {}, identity: undefined as never } });
    expect(overwolf.packages.logsFolderPath).toBe('/..\\ow-electron//logs');
    expect(overwolf.packages.phasePercent).toBe(0);
  });

  it('accepts listeners and emits nothing while no runtime exists', async () => {
    host = mockHost();
    const listener = vi.fn();
    for (const name of ['loading', 'ready', 'failed-to-initialize', 'crashed', 'updated'])
      overwolf.packages.on(name, listener);
    await whenHostReady();
    await settle();
    expect(listener).not.toHaveBeenCalled();
    expect(overwolf.packages.listenerCount('ready')).toBe(1);
  });

  it('emits package runtime messages with a synthetic Event first (B.1.2, P.7)', async () => {
    host = mockHost();
    const seen: unknown[][] = [];
    for (const name of [
      'loading',
      'ready',
      'failed-to-initialize',
      'crashed',
      'package-update-pending',
      'updated',
    ])
      overwolf.packages.on(name, (...args: unknown[]) => seen.push([name, ...args]));
    overwolf.packages.on('crashed', (event: { preventDefault(): void }) => {
      event.preventDefault();
      event.preventDefault();
    });
    host.push(
      { type: 'packages', event: 'loading', name: 'gep' },
      { type: 'packages', event: 'ready', name: 'gep', version: '1.0.0' },
      { type: 'packages', event: 'failed-to-initialize', name: 'overlay' },
      { type: 'packages', event: 'crashed', canRecover: true, eventId: 7 },
      { type: 'packages', event: 'package-update-pending', info: [{ name: 'gep' }] },
      { type: 'packages', event: 'package-update-pending' },
      { type: 'packages', event: 'updated', name: 'gep', version: '1.0.1' },
      { type: 'packages', event: 'mystery' },
      { type: 'packages' },
    );
    expect(seen.map((s) => s[0])).toEqual([
      'loading',
      'ready',
      'failed-to-initialize',
      'crashed',
      'package-update-pending',
      'package-update-pending',
      'updated',
    ]);
    const [, event, ...rest] = seen[1]!;
    expect(event).toMatchObject({ defaultPrevented: false });
    expect(rest).toEqual(['gep', '1.0.0']);
    expect(seen[3]![1]).toMatchObject({ defaultPrevented: true });
    expect(seen[3]![2]).toBe(true);
    expect(seen[5]![2]).toEqual([]);
    await settle();
    expect(host.callsOf('package_event_action')).toEqual([
      { eventId: 7, action: 'prevent-default' },
    ]);
  });

  it('syntheticEvent works without a callback', () => {
    const event = syntheticEvent();
    event.preventDefault();
    expect(event.defaultPrevented).toBe(true);
  });
});

describe('browser switches for the next launch (A.1.1, B.2.1)', () => {
  it('records appendSwitch and disableHardwareAcceleration before main_ready', async () => {
    host = mockHost();
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    app.commandLine.appendSwitch('remote-debugging-port', '9222');
    app.commandLine.appendSwitch('enable-logging');
    app.commandLine.appendArgument('file.txt');
    app.disableHardwareAcceleration();
    app.commandLine.removeSwitch('enable-logging');
    await whenHostReady();
    const recorded = host.callsOf('app_record_browser_args');
    expect(recorded.at(-1)).toEqual({ args: ['--remote-debugging-port=9222', '--disable-gpu'] });
    expect(host.callsOf('main_ready')).toEqual([
      { pendingBrowserArgs: ['--remote-debugging-port=9222', '--disable-gpu'] },
    ]);
    const order = commandOrder(host);
    expect(order.lastIndexOf('app_record_browser_args')).toBeLessThan(order.indexOf('main_ready'));
  });

  it('sends main_ready without arguments when nothing was recorded', async () => {
    host = mockHost();
    await whenHostReady();
    expect(host.callsOf('main_ready')).toEqual([{}]);
    expect(host.callsOf('app_record_browser_args')).toHaveLength(0);
  });

  it('keeps going when the plugin has no app_record_browser_args command', async () => {
    host = mockHost({
      commands: {
        app_record_browser_args: () => {
          throw 'Command app_record_browser_args not found';
        },
      },
    });
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => undefined);
    app.disableHardwareAcceleration();
    await whenHostReady();
    expect(host.callsOf('main_ready')).toEqual([{ pendingBrowserArgs: ['--disable-gpu'] }]);
    expect(debug).toHaveBeenCalledWith(expect.stringContaining('app_record_browser_args failed'));
  });
});
