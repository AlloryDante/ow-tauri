/**
 * The `app.overwolf` types of the ow-electron 42.11.4 typings
 * (`docs/CONTRACT.md` B.1.1, B.1.3), declared as module types for
 * `ow-tauri/main`. The global `overwolf` namespace of `ow-tauri/types`
 * (B.4) declares the same shapes for code that uses the namespace.
 *
 * @packageDocumentation
 */
import type { EventEmitter } from '../shared/emitter.js';

/** Window reference accepted as `CMPWindowOptions.parent`: any facade `BrowserWindow`. */
export interface CMPParentWindow {
  /** The window's app-visible id (`BrowserWindow.id`). */
  readonly id: number;
}

/** Options of `openCMPWindow()` / `openAdPrivacySettingsWindow()` (CONTRACT A.2.2). */
export interface CMPWindowOptions {
  /** Tab to open; default `'purposes'`. */
  tab?: 'purposes' | 'features' | 'vendors';
  /** Owned by `parent` and kept above it (input modality is Windows-only); default `false`. */
  modal?: boolean;
  /** Parent window; default none. */
  parent?: CMPParentWindow;
  /** Centre the window on the screen; default `true`. */
  center?: boolean;
  /** Preloader background colour; default `#0D0D0D`. */
  backgroundColor?: string;
  /** Preloader spinner colour. */
  preLoaderSpinnerColor?: string;
  /** Window width; default 800. */
  width?: number;
  /** Window height; default 800. */
  height?: number;
  /** Window left edge. */
  x?: number;
  /** Window top edge. */
  y?: number;
  /** Overrides the consent page URL (`https:` only). */
  cmpURL?: string;
  /** Consent page language; default `en`. */
  language?: string;
}

/** Email hashes, as `generateUserEmailHashes()` returns them and `setUserEmailHashes()` takes them. */
export interface EmailHashes {
  /** SHA-1, lower-case hex. */
  readonly sha1?: string;
  /** SHA-256, lower-case hex. */
  readonly sha256?: string;
  /** MD5, lower-case hex. */
  readonly md5?: string;
}

/** External payment providers Overwolf knows about. */
export type ExternalPaymentProvider = 'tebex' | (string & {});

/** Options of `setExternalPaymentUserId()`. */
export interface ExternalPaymentUserIdOptions {
  /** The payment provider; `'tebex'` when absent. */
  providerName: ExternalPaymentProvider;
  /** The user id the app passes to the provider (mandatory). */
  userId: string;
  /** The provider's recurring payment (subscription) id. */
  paymentId?: string;
}

/** Built-in Overwolf package names. */
export type PackageName = 'gep' | 'overlay' | 'recorder' | 'utility' | 'crn' | (string & {});

/** A package name and version. */
export interface PackageInfo {
  /** Package name. */
  name: string;
  /** Package version. */
  version: string;
}

/** `hasPendingUpdates()` result. */
export interface PendingUpdatesResult {
  /** Whether an update waits for a restart. */
  hasPendingUpdate: boolean;
  /** The packages that wait. */
  details: PackageInfo[];
}

/** `setChannel()` result. */
export interface SetChannelResult {
  /** Whether the switch succeeded. */
  success: boolean;
  /** Why it failed. */
  error?: 'invalid-package' | 'invalid-channel';
}

/** Passed to the `ready` callback of `setChannel()`. */
export interface ChannelPackageInfo {
  /** Package name. */
  name: string;
  /** Downloaded version. */
  version: string;
}

/** `getAvailableChannels()` result: channel names per package. */
export type AvailableChannelsResult = Record<string, string[]>;

/** `getChannel()` result: the active channel per package. */
export type CurrentChannelsResult = Record<string, string>;

/**
 * The Overwolf package manager (`app.overwolf.packages`, CONTRACT B.1.3): a
 * Node-style event emitter whose events are `loading`, `ready`,
 * `failed-to-initialize`, `crashed`, `package-update-pending` and `updated`,
 * each with a synthetic Electron `Event` as the first argument.
 */
export interface OverwolfPackageManager extends EventEmitter {
  /** Relaunches the package manager; no effect while no package is loaded. */
  relaunch(): void;
  /**
   * Whether package updates wait for a restart (synchronous).
   *
   * @returns the pending updates
   */
  hasPendingUpdates(): PendingUpdatesResult;
  /**
   * Switches a package to a release channel.
   *
   * @param packageName - the package
   * @param channel - the channel; absent, `''` or `'public'` restores the public release
   * @param ready - called when the download completes
   * @returns the result
   */
  setChannel(
    packageName: PackageName,
    channel?: string,
    ready?: (packageInfo: ChannelPackageInfo) => void,
  ): Promise<SetChannelResult>;
  /**
   * The release channels available per package.
   *
   * @param packageNames - packages to query; none queries all listed packages
   * @returns the channels
   */
  getAvailableChannels(...packageNames: PackageName[]): Promise<AvailableChannelsResult>;
  /**
   * The active release channel per package.
   *
   * @param packageNames - packages to query
   * @returns the channels
   */
  getChannel(...packageNames: PackageName[]): Promise<CurrentChannelsResult>;
  /** The application's package logs folder (ow-electron's literal string, F.4). */
  readonly logsFolderPath: string;
  /** Rollout bucket used by the package manager (E.4). */
  readonly phasePercent: number;
}

/** `app.overwolf.__settings__.adsOptimization` [OBS]. */
export interface AdsOptimizationSettings {
  /** Set by `disableAdsFPD()`. */
  readonly anonymous?: true;
  /** Set by `disableAdsOptimization()`. */
  readonly disable?: true;
}

/** `app.overwolf.__settings__` (not in the typings), as ow-electron exposes it [OBS]. */
export interface OverwolfSettings {
  /** The ad page URL. */
  readonly src: string;
  /** Always `false`. */
  readonly forceSandboxMode: boolean;
  /** Ad optimisation opt-outs of this session. */
  readonly adsOptimization: AdsOptimizationSettings;
  /** Consent endpoints. */
  readonly adsSetting: {
    /** Global vendor list, v1. */
    readonly gvlUrlV1: string;
    /** Global vendor list. */
    readonly gvlUrl: string;
    /** The EU-only consent experiment endpoint. */
    readonly cmpFeatureUrl: string;
    /** The startup consent page. */
    readonly cmpUrl: string;
    /** The consent settings page. */
    readonly cmpSettingUrl: string;
    /** Always `''`. */
    readonly cmpWindowUrl: string;
  };
  /** ow-electron's logger switch; always disabled. */
  readonly logger: {
    /** Always `false`. */
    readonly enabled: boolean;
  };
  /** Overwolf's analytics endpoints. */
  readonly analytics: {
    /** The Counter endpoint. */
    readonly analyticsUrl: string;
    /** The tracking host. */
    readonly trackingUrl: string;
  };
  /** Whether this is the app's first launch on this machine. */
  readonly firstLaunch: boolean;
}

/**
 * The Overwolf APIs on Electron's `app` object (`app.overwolf`), every member
 * of the ow-electron 42.11.4 typings (CONTRACT B.1.1).
 */
export interface OverwolfApi {
  /** Disables anonymous analytics for this session; call before `app.ready`. */
  disableAnonymousAnalytics(): void;
  /** Disables ad optimisation for this session. */
  disableAdsOptimization(): void;
  /** Opts out of first-party data (email hashes) for ad targeting in this session. */
  disableAdsFPD(): void;
  /**
   * Whether the user should be offered the consent settings window. Never rejects.
   *
   * @returns `true` unless the host says otherwise
   */
  isCMPRequired(): Promise<boolean>;
  /**
   * Opens the consent settings window (deprecated upstream; same as
   * {@link OverwolfApi.openAdPrivacySettingsWindow}).
   *
   * @param options - window options
   * @returns resolves once the window exists
   */
  openCMPWindow(options?: CMPWindowOptions): Promise<void>;
  /**
   * Opens the ad privacy settings window.
   *
   * @param options - window options
   * @returns resolves once the window exists
   */
  openAdPrivacySettingsWindow(options?: CMPWindowOptions): Promise<void>;
  /** The package manager. */
  readonly packages: OverwolfPackageManager;
  /**
   * Hashes an email address (synchronous) and sends the hashes to the ad views.
   *
   * @param email - the address; it is not stored
   * @returns the hashes
   */
  generateUserEmailHashes(email: string): EmailHashes;
  /**
   * Sends email hashes to the ad views.
   *
   * @param emailHashes - the hashes
   */
  setUserEmailHashes(emailHashes?: EmailHashes): void;
  /**
   * Associates the user's id at an external payment provider with this machine.
   *
   * @param options - provider, user id and payment id
   * @returns resolves after the report; rejects only for a missing `userId` or before `app.ready`
   */
  setExternalPaymentUserId(options: ExternalPaymentUserIdOptions): Promise<void>;
  /** Rollout bucket, 0 to 99 (E.4). */
  readonly phasePercent: number;
  /** UTM parameters from the Overwolf installer, or `undefined`. */
  readonly utmParams: unknown;
  /** Machine id (E.4). */
  readonly muid: string;
  /** App uid (G.2). */
  readonly uid: string;
  /** ow-electron's internal settings object (not in the typings) [OBS]. */
  readonly __settings__: OverwolfSettings;
}
