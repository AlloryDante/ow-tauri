/**
 * The calls of the CMP & settings page, each written to the log with its
 * result. Every function here is one `tauri-plugin-overwolf-api` call; the
 * permission set it needs is in {@link PERMISSIONS} (granted in
 * `src-tauri/capabilities/default.json`).
 *
 * @packageDocumentation
 */
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
  setUserEmailHashes,
  type CMPTab,
  type EmailHashes,
  type MachineIds,
  type OverwolfInfo,
} from 'tauri-plugin-overwolf-api';

import { logged, type Outcome } from '../../log/logged';
import type { LogStore } from '../../log/store';

/** The permission set each call needs. */
export const PERMISSIONS = {
  getInfo: 'overwolf:default',
  isCMPRequired: 'overwolf:default',
  openAdPrivacySettingsWindow: 'overwolf:default',
  openCMPWindow: 'overwolf:default',
  disableAdsFPD: 'overwolf:default',
  disableAdsOptimization: 'overwolf:default',
  disableAnonymousAnalytics: 'overwolf:default',
  generateUserEmailHashes: 'overwolf:email-hashes',
  setUserEmailHashes: 'overwolf:email-hashes',
  clearUserEmailHashes: 'overwolf:email-hashes',
  getMachineIds: 'overwolf:machine-id',
  setAnonymousAnalyticsPreference: 'overwolf:analytics',
  setAnalyticsUserEnabled: 'overwolf:analytics',
} as const satisfies Record<SettingsCall, string>;

/** A call of this page. */
export type SettingsCall = keyof SettingsActions;

/**
 * An id with all but its first `keep` characters replaced by `•`, so a
 * machine id never shows in full unless the user asks.
 *
 * @param id - the id
 * @param keep - characters kept (default 4)
 * @returns the masked id
 */
export function mask(id: string, keep = 4): string {
  return id.length <= keep ? '•'.repeat(id.length) : `${id.slice(0, keep)}${'•'.repeat(8)}`;
}

/**
 * The UTM parameters of `getInfo()` as `key=value` lines, or a note when the
 * install recorded none.
 *
 * @param info - what `getInfo()` returned
 * @returns the text
 */
export function utmText(info: Pick<OverwolfInfo, 'utmParams'>): string {
  const params = info.utmParams;
  if (!params || Object.keys(params).length === 0) return 'none recorded at install';
  return Object.entries(params)
    .map(([k, v]) => `${k}=${v}`)
    .join('\n');
}

/**
 * Whether `text` looks like an e-mail address (a quick check before the
 * plugin's own validation).
 *
 * @param text - the input
 * @returns whether it has one `@` with text on both sides
 */
export function looksLikeEmail(text: string): boolean {
  return /^[^\s@]+@[^\s@]+$/.test(text.trim());
}

/** The page's calls; each resolves with its outcome and never rejects. */
export interface SettingsActions {
  /** `getInfo()`. */
  getInfo(): Promise<Outcome<OverwolfInfo>>;
  /** `isCMPRequired()`. */
  isCMPRequired(): Promise<Outcome<boolean>>;
  /** `openAdPrivacySettingsWindow()`, on `tab` when given. */
  openAdPrivacySettingsWindow(tab?: CMPTab): Promise<Outcome<void>>;
  /** `openCMPWindow()` (the deprecated ow-electron name of the same window). */
  openCMPWindow(): Promise<Outcome<void>>;
  /** `disableAdsFPD()`. */
  disableAdsFPD(): Promise<Outcome<void>>;
  /** `disableAdsOptimization()`. */
  disableAdsOptimization(): Promise<Outcome<void>>;
  /** `disableAnonymousAnalytics()`. */
  disableAnonymousAnalytics(): Promise<Outcome<void>>;
  /** `generateUserEmailHashes(email)`; the address is never logged. */
  generateUserEmailHashes(email: string): Promise<Outcome<EmailHashes>>;
  /** `setUserEmailHashes(hashes)`; without hashes the stored ones are sent again. */
  setUserEmailHashes(hashes?: EmailHashes): Promise<Outcome<void>>;
  /** `clearUserEmailHashes()`. */
  clearUserEmailHashes(): Promise<Outcome<void>>;
  /** `getMachineIds()`; the log shows them masked. */
  getMachineIds(): Promise<Outcome<MachineIds>>;
  /** `setAnonymousAnalyticsPreference(enabled)`. */
  setAnonymousAnalyticsPreference(enabled: boolean): Promise<Outcome<void>>;
  /** `setAnalyticsUserEnabled(enabled)`. */
  setAnalyticsUserEnabled(enabled: boolean): Promise<Outcome<void>>;
}

/**
 * The page's calls, bound to a log store.
 *
 * @param log - the log store
 * @returns one function per call
 */
export function settingsActions(log: LogStore): SettingsActions {
  return {
    getInfo: (): Promise<Outcome<OverwolfInfo>> => logged(log, 'getInfo()', getInfo),
    isCMPRequired: (): Promise<Outcome<boolean>> => logged(log, 'isCMPRequired()', isCMPRequired),
    openAdPrivacySettingsWindow: (tab?: CMPTab): Promise<Outcome<void>> =>
      logged(log, `openAdPrivacySettingsWindow(${tab ? `{ tab: '${tab}' }` : ''})`, () =>
        openAdPrivacySettingsWindow(tab ? { tab } : undefined),
      ),
    openCMPWindow: (): Promise<Outcome<void>> =>
      // eslint-disable-next-line @typescript-eslint/no-deprecated -- the page shows the deprecated ow-electron name on purpose
      logged(log, 'openCMPWindow()', () => openCMPWindow()),
    disableAdsFPD: (): Promise<Outcome<void>> => logged(log, 'disableAdsFPD()', disableAdsFPD),
    disableAdsOptimization: (): Promise<Outcome<void>> =>
      logged(log, 'disableAdsOptimization()', disableAdsOptimization),
    disableAnonymousAnalytics: (): Promise<Outcome<void>> =>
      logged(log, 'disableAnonymousAnalytics()', disableAnonymousAnalytics),
    // The address itself is never logged.
    generateUserEmailHashes: (email: string): Promise<Outcome<EmailHashes>> =>
      logged(log, 'generateUserEmailHashes(<e-mail>)', () => generateUserEmailHashes(email.trim())),
    setUserEmailHashes: (hashes?: EmailHashes): Promise<Outcome<void>> =>
      logged(log, hashes ? 'setUserEmailHashes(hashes)' : 'setUserEmailHashes()', () =>
        setUserEmailHashes(hashes),
      ),
    clearUserEmailHashes: (): Promise<Outcome<void>> =>
      logged(log, 'clearUserEmailHashes()', clearUserEmailHashes),
    getMachineIds: (): Promise<Outcome<MachineIds>> =>
      logged(log, 'getMachineIds()', getMachineIds, {
        shown: (ids) => ({ muid: mask(ids.muid), muidV2: mask(ids.muidV2) }),
      }),
    setAnonymousAnalyticsPreference: (enabled: boolean): Promise<Outcome<void>> =>
      logged(log, `setAnonymousAnalyticsPreference(${String(enabled)})`, () =>
        setAnonymousAnalyticsPreference(enabled),
      ),
    setAnalyticsUserEnabled: (enabled: boolean): Promise<Outcome<void>> =>
      logged(log, `setAnalyticsUserEnabled(${String(enabled)})`, () =>
        setAnalyticsUserEnabled(enabled),
      ),
  };
}
