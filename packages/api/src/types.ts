/**
 * Types of the main entry (`tauri-plugin-overwolf-api`).
 *
 * @packageDocumentation
 */

/** The host the app runs on, as Overwolf's analytics and ad pages see it. */
export interface HostInfo {
  /** The host label sent to Overwolf where ow-electron sends `electron` (default `"tauri"`). */
  label: string;
  /** The host version sent with it (default: the Tauri version). */
  version: string;
  /** The Overwolf runtime version the ad pages receive (`owVersion`). */
  owVersion: string;
}

/**
 * What `getInfo()` returns: the app identity and the plugin state. The
 * machine ids are not part of it (see `getMachineIds`).
 */
export interface OverwolfInfo {
  /** The app uid (the console-assigned uid, or the ow-electron formula). */
  uid: string;
  /** The computed uid (ow-electron's `app_cuid`), even when `uid` is pinned. */
  appCuid: string;
  /** The rollout bucket of this install, 0 to 99. */
  phasePercent: number;
  /** The UTM parameters recorded at install, or `null`. */
  utmParams: Record<string, string> | null;
  /** Whether Overwolf test ads are on. */
  testAd: boolean;
  /** Whether `<owadview>` can show ads on this platform (false on Linux and mobile). */
  adsSupported: boolean;
  /** The app name Overwolf sees (`plugins.overwolf.name`, else `productName`). */
  name: string;
  /** The app version (`tauri.conf.json` `version`). */
  version: string;
  /** The host label and versions. */
  host: HostInfo;
}

/**
 * The machine identifiers every Overwolf app on this machine shares. Needs
 * the `overwolf:machine-id` permission.
 */
export interface MachineIds {
  /** ow-electron's `app.overwolf.muid`: `muidV2` when present, else the first-generation id. */
  muid: string;
  /** The second-generation machine id. */
  muidV2: string;
}

/** A tab of the consent window. */
export type CMPTab = 'purposes' | 'features' | 'vendors';

/** Options of the consent (ad privacy settings) window. */
export interface CMPWindowOptions {
  /** The tab to open. */
  tab?: CMPTab;
  /** Whether the window is modal to its parent. */
  modal?: boolean;
  /** The label of the parent window; with `modal`, the caller's window by default. */
  parent?: string;
  /** Whether to centre the window. */
  center?: boolean;
  /** Window background colour (CSS colour). */
  backgroundColor?: string;
  /** Colour of the loading spinner. */
  preLoaderSpinnerColor?: string;
  /** Width in logical pixels. */
  width?: number;
  /** Height in logical pixels. */
  height?: number;
  /** Left edge in logical pixels. */
  x?: number;
  /** Top edge in logical pixels. */
  y?: number;
  /**
   * Another consent page; its origin must be listed in
   * `plugins.overwolf.consent.allowedCmpOrigins`, else the call rejects with
   * `invalid-argument`.
   */
  cmpURL?: string;
  /** The page language (e.g. `"de"`). */
  language?: string;
}

/**
 * Hashes of the user's e-mail address, encoded as
 * `plugins.overwolf.emailHashes.encoding` says (lower-case hex by default,
 * as ow-electron).
 */
export interface EmailHashes {
  /** SHA-1. */
  sha1?: string;
  /** MD5. */
  md5?: string;
  /** SHA-256. */
  sha256?: string;
}

/** Options of `setExternalPaymentUserId()`. */
export interface ExternalPaymentUserIdOptions {
  /** The payment provider. */
  providerName?: 'tebex' | (string & {});
  /** The user's id at the provider (required). */
  userId: string | number;
  /** A payment id. */
  paymentId?: string;
}
