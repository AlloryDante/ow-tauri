/**
 * `tauri-plugin-overwolf-api`: the JavaScript API of `tauri-plugin-overwolf`.
 *
 * Identity, consent (CMP), e-mail hashes, analytics switches and window
 * naming. Ads are a separate side-effect import
 * (`import 'tauri-plugin-overwolf-api/adview'`), the Windows updater is
 * `tauri-plugin-overwolf-api/updater`.
 *
 * Every function calls one plugin command and rejects with an
 * {@link OverwolfError}. The permission each command needs is in the plugin's
 * `permissions/` sets: `overwolf:default` covers everything here except
 * {@link getMachineIds} (`overwolf:machine-id`), the e-mail hash functions
 * (`overwolf:email-hashes`) and {@link setExternalPaymentUserId},
 * {@link setAnalyticsUserEnabled} and {@link setAnonymousAnalyticsPreference}
 * (`overwolf:analytics`).
 *
 * @example
 * ```ts
 * import { getInfo, isCMPRequired, openAdPrivacySettingsWindow } from 'tauri-plugin-overwolf-api';
 *
 * const info = await getInfo();
 * console.log(`uid ${info.uid}, test ads ${String(info.testAd)}`);
 * if (await isCMPRequired()) await openAdPrivacySettingsWindow();
 * ```
 *
 * @packageDocumentation
 */
import { call } from './internal.js';
import type {
  CMPWindowOptions,
  EmailHashes,
  ExternalPaymentUserIdOptions,
  MachineIds,
  OverwolfInfo,
} from './types.js';

export {
  ERROR_CODES,
  OverwolfError,
  isOverwolfErrorWire,
  type OverwolfErrorCode,
  type OverwolfErrorOptions,
  type OverwolfErrorWire,
} from './errors.js';
export type {
  CMPTab,
  CMPWindowOptions,
  EmailHashes,
  ExternalPaymentUserIdOptions,
  HostInfo,
  MachineIds,
  OverwolfInfo,
} from './types.js';

/**
 * The app identity and plugin state. Needs `overwolf:default`.
 *
 * @returns uid, computed uid, rollout bucket, UTM parameters, test-ad and ads
 *   support flags, app name and version, host label and versions
 *
 * @example
 * ```ts
 * const { uid, adsSupported } = await getInfo();
 * ```
 */
export async function getInfo(): Promise<OverwolfInfo> {
  return await call<OverwolfInfo>('get_info');
}

/**
 * The machine ids every Overwolf app on this machine shares (ow-electron's
 * `muid` / `muidV2`). Needs the opt-in `overwolf:machine-id` permission;
 * rejects with `forbidden` without it.
 *
 * @returns the machine ids
 */
export async function getMachineIds(): Promise<MachineIds> {
  return await call<MachineIds>('get_machine_ids');
}

/**
 * Whether the user must be asked for ad consent (ow-electron's
 * `isCMPRequired()`). Never rejects: on any failure it resolves `true`, as
 * ow-electron does when the consent service cannot be reached.
 *
 * @returns whether consent is required
 */
export async function isCMPRequired(): Promise<boolean> {
  try {
    return (await call<boolean | null>('is_cmp_required')) !== false;
  } catch {
    return true;
  }
}

/**
 * Opens Overwolf's ad privacy settings (consent) window. Resolves when the
 * window exists.
 *
 * @param options - tab, modality, parent window and placement
 * @returns resolves when the window is open
 */
export async function openAdPrivacySettingsWindow(options?: CMPWindowOptions): Promise<void> {
  await call<null>('open_ad_privacy_settings_window', { options: options ?? null });
}

/**
 * The deprecated ow-electron name of {@link openAdPrivacySettingsWindow}; opens
 * the same window.
 *
 * @param options - tab, modality, parent window and placement
 * @returns resolves when the window is open
 * @deprecated Use {@link openAdPrivacySettingsWindow}.
 */
export async function openCMPWindow(options?: CMPWindowOptions): Promise<void> {
  await call<null>('open_cmp_window', { options: options ?? null });
}

/**
 * Hashes an e-mail address in the plugin (byte-identical to ow-electron),
 * sends the hashes to Overwolf's ad stack and stores them like ow-electron.
 * The address itself is never stored or logged. Needs `overwolf:email-hashes`.
 *
 * @param email - the user's e-mail address
 * @returns the hashes
 */
export async function generateUserEmailHashes(email: string): Promise<EmailHashes> {
  return await call<EmailHashes>('generate_user_email_hashes', { email });
}

/**
 * Sends hashes the app computed itself and stores them. Without an argument
 * the stored hashes are sent again. Needs `overwolf:email-hashes`.
 *
 * @param hashes - the hashes
 * @returns resolves when they were applied
 */
export async function setUserEmailHashes(hashes?: EmailHashes): Promise<void> {
  await call<null>('set_user_email_hashes', { hashes: hashes ?? null });
}

/**
 * Removes the stored e-mail hashes. Needs `overwolf:email-hashes`.
 *
 * @returns resolves when they are removed
 */
export async function clearUserEmailHashes(): Promise<void> {
  await call<null>('clear_user_email_hashes');
}

/**
 * Stops anonymous analytics for this launch (ow-electron's
 * `disableAnonymousAnalytics()`). Called after the launch burst it stops
 * what follows and logs one warning; to suppress the burst of the next
 * launches too, use {@link setAnonymousAnalyticsPreference}, the
 * `plugins.overwolf.analytics.disableAnonymous` setting or the Rust builder.
 *
 * @returns resolves when applied
 */
export async function disableAnonymousAnalytics(): Promise<void> {
  await call<null>('disable_anonymous_analytics');
}

/**
 * Turns ad optimisation off for this launch (ow-electron's
 * `disableAdsOptimization()`).
 *
 * @returns resolves when applied
 */
export async function disableAdsOptimization(): Promise<void> {
  await call<null>('disable_ads_optimization');
}

/**
 * Turns first-party ad data off for this launch (ow-electron's
 * `disableAdsFPD()`).
 *
 * @returns resolves when applied
 */
export async function disableAdsFPD(): Promise<void> {
  await call<null>('disable_ads_fpd');
}

/**
 * Stores the user's anonymous-analytics choice; it applies from the next
 * launch's burst on (and, when `false`, also stops analytics now). Needs
 * `overwolf:analytics`.
 *
 * @param enabled - whether anonymous analytics may be sent
 * @returns resolves when stored
 */
export async function setAnonymousAnalyticsPreference(enabled: boolean): Promise<void> {
  await call<null>('set_anonymous_analytics_preference', { enabled });
}

/**
 * Reports the user's id at an external payment provider (ow-electron's
 * `setExternalPaymentUserId()`), with ow-electron's validation messages.
 * Needs `overwolf:analytics`.
 *
 * @param options - provider, user id and payment id (key order is kept)
 * @returns resolves when sent
 */
export async function setExternalPaymentUserId(
  options: ExternalPaymentUserIdOptions,
): Promise<void> {
  await call<null>('set_external_payment_user_id', { options });
}

/**
 * Turns the user-level analytics switch on or off. Rejects with
 * `unsupported` unless `plugins.overwolf.analytics.userSwitch` is `true`.
 * Needs `overwolf:analytics`.
 *
 * @param enabled - whether analytics are on for this user
 * @returns resolves when applied
 */
export async function setAnalyticsUserEnabled(enabled: boolean): Promise<void> {
  await call<null>('set_analytics_user_enabled', { enabled });
}

/**
 * Names the caller's own window for Overwolf (analytics `window_closed`, the
 * ad pages' `windowName`). The default is derived from the page URL the way
 * ow-electron derives it. `name` is 1 to 128 printable ASCII characters.
 *
 * @param name - the window name
 * @returns resolves when applied
 */
export async function setWindowName(name: string): Promise<void> {
  await call<null>('set_window_name', { name });
}
