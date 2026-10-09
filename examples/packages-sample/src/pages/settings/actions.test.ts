import { mockOverwolf, type MockOverwolf } from 'tauri-plugin-overwolf-api/testing';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { createLogStore } from '../../log/store';
import { PERMISSIONS, looksLikeEmail, mask, settingsActions, utmText } from './actions';

let overwolf: MockOverwolf;
beforeEach(() => {
  overwolf = mockOverwolf({ machineIds: { muid: 'machine-one', muidV2: 'machine-two' } });
});
afterEach(() => {
  overwolf.restore();
});

describe('settingsActions', () => {
  it('calls the plugin command of each function with its arguments', async () => {
    const actions = settingsActions(createLogStore());
    const hashes = { sha1: 'a', md5: 'b', sha256: 'c' };
    const cases: [() => Promise<unknown>, string, Record<string, unknown>][] = [
      [() => actions.getInfo(), 'get_info', {}],
      [() => actions.isCMPRequired(), 'is_cmp_required', {}],
      [
        () => actions.openAdPrivacySettingsWindow(),
        'open_ad_privacy_settings_window',
        { options: null },
      ],
      [
        () => actions.openAdPrivacySettingsWindow('vendors'),
        'open_ad_privacy_settings_window',
        { options: { tab: 'vendors' } },
      ],
      [() => actions.openCMPWindow(), 'open_cmp_window', { options: null }],
      [() => actions.disableAdsFPD(), 'disable_ads_fpd', {}],
      [() => actions.disableAdsOptimization(), 'disable_ads_optimization', {}],
      [() => actions.disableAnonymousAnalytics(), 'disable_anonymous_analytics', {}],
      [
        () => actions.generateUserEmailHashes(' player@example.com '),
        'generate_user_email_hashes',
        { email: 'player@example.com' },
      ],
      // The API tells "no argument" from a value on the wire (as ow-electron
      // stores a given value as is).
      [
        () => actions.setUserEmailHashes(hashes),
        'set_user_email_hashes',
        { hashes: { value: hashes } },
      ],
      [() => actions.setUserEmailHashes(), 'set_user_email_hashes', { hashes: {} }],
      [() => actions.clearUserEmailHashes(), 'clear_user_email_hashes', {}],
      [() => actions.getMachineIds(), 'get_machine_ids', {}],
      [
        () => actions.setAnonymousAnalyticsPreference(false),
        'set_anonymous_analytics_preference',
        { enabled: false },
      ],
      [
        () => actions.setAnalyticsUserEnabled(true),
        'set_analytics_user_enabled',
        { enabled: true },
      ],
    ];
    for (const [run, command, args] of cases) {
      const before = overwolf.calls.length;
      await run();
      const call = overwolf.calls[before];
      expect(call?.command, command).toBe(command);
      expect(call?.args, command).toMatchObject(args);
    }
  });

  it('resolves with the outcome and logs every call', async () => {
    const log = createLogStore();
    const actions = settingsActions(log);
    expect(await actions.isCMPRequired()).toEqual({ ok: true, value: false });
    expect(log.entries().map((e) => e.message)).toEqual(['isCMPRequired()', 'isCMPRequired() →']);
  });

  it('never logs the e-mail address', async () => {
    const log = createLogStore();
    await settingsActions(log).generateUserEmailHashes('player@example.com');
    expect(JSON.stringify(log.entries())).not.toContain('player@example.com');
  });

  it('logs the machine ids masked and returns them in full', async () => {
    const log = createLogStore();
    const outcome = await settingsActions(log).getMachineIds();
    expect(outcome).toEqual({ ok: true, value: { muid: 'machine-one', muidV2: 'machine-two' } });
    const text = JSON.stringify(log.entries());
    expect(text).not.toContain('machine-one');
    expect(text).toContain('mach••••••••');
  });

  it('reports a missing permission as forbidden', async () => {
    overwolf.setCommand('get_machine_ids', () => {
      throw new Error('overwolf.get_machine_ids not allowed by ACL');
    });
    const outcome = await settingsActions(createLogStore()).getMachineIds();
    expect(outcome.ok).toBe(false);
    expect(!outcome.ok && outcome.code).toBe('forbidden');
  });

  it('names the permission set of every call', () => {
    expect(PERMISSIONS.getMachineIds).toBe('overwolf:machine-id');
    expect(PERMISSIONS.generateUserEmailHashes).toBe('overwolf:email-hashes');
    expect(PERMISSIONS.setAnalyticsUserEnabled).toBe('overwolf:analytics');
    expect(PERMISSIONS.disableAdsFPD).toBe('overwolf:default');
    expect(Object.keys(PERMISSIONS).sort()).toEqual(
      Object.keys(settingsActions(createLogStore())).sort(),
    );
  });
});

describe('helpers', () => {
  it('masks ids', () => {
    expect(mask('abcdefgh')).toBe('abcd••••••••');
    expect(mask('abc')).toBe('•••');
    expect(mask('abcdef', 2)).toBe('ab••••••••');
  });

  it('writes UTM parameters one per line', () => {
    expect(utmText({ utmParams: null })).toBe('none recorded at install');
    expect(utmText({ utmParams: {} })).toBe('none recorded at install');
    expect(utmText({ utmParams: { utm_source: 'web', utm_medium: 'ad' } })).toBe(
      'utm_source=web\nutm_medium=ad',
    );
  });

  it('checks the e-mail shape', () => {
    expect(looksLikeEmail(' a@b ')).toBe(true);
    expect(looksLikeEmail('a@')).toBe(false);
    expect(looksLikeEmail('a b@c')).toBe(false);
    expect(looksLikeEmail('')).toBe(false);
  });
});
