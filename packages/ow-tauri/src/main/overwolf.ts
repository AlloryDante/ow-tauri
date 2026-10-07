/**
 * `app.overwolf` (`docs/CONTRACT.md` B.1.1): the ow-electron 42.11.4
 * Overwolf API for the main webview.
 *
 * Synchronous members (`uid`, `muid`, `phasePercent`, `utmParams`) read the
 * state cache Rust keeps current (B.1.6); `generateUserEmailHashes()` hashes
 * in JavaScript, because ow-electron returns the hashes synchronously; every
 * other member is one plugin command (A.2.2).
 *
 * @packageDocumentation
 */
import type { FacadeKernel } from '../bootstrap/facade-kernel.js';
import { OwTauriError } from '../shared/errors.js';
import { isPreCommandRejection } from '../shared/wire-error.js';
import { emailHashes } from './hashes.js';
import type {
  AdsOptimizationSettings,
  CMPWindowOptions,
  EmailHashes,
  ExternalPaymentUserIdOptions,
  OverwolfApi,
  OverwolfSettings,
} from './overwolf-types.js';
import { PackageManager } from './packages.js';

/** ow-electron's message for `setExternalPaymentUserId()` without a user id [OBS]. */
export const PAYMENT_ID_MANDATORY = 'providerName and userId are mandatory';
/** ow-electron's message for `setExternalPaymentUserId()` before `app.ready` [TYPES]. */
export const NOT_READY_MESSAGE = 'ow-electron is not ready yet!';

/** `CMPWindowOptions` keys copied onto the wire (`parent` becomes `parentId`). */
const CMP_KEYS = [
  'tab',
  'modal',
  'center',
  'backgroundColor',
  'preLoaderSpinnerColor',
  'width',
  'height',
  'x',
  'y',
  'cmpURL',
  'language',
] as const satisfies readonly (keyof CMPWindowOptions)[];

function deepFreeze<T>(value: T): T {
  if (typeof value === 'object' && value !== null && !Object.isFrozen(value)) {
    Object.freeze(value);
    for (const key of Object.keys(value)) deepFreeze((value as Record<string, unknown>)[key]);
  }
  return value;
}

/**
 * Builds `app.overwolf.__settings__` with ow-electron's keys and key order
 * [OBS]. The object is frozen; only `adsOptimization` changes: `{}` at
 * start, `anonymous: true` after `disableAdsFPD()`, `disable: true` after
 * `disableAdsOptimization()` [OBS].
 *
 * @param adsOptimization - reads the current opt-outs
 * @param firstLaunch - whether this is the app's first launch
 * @returns the settings object
 */
export function createSettings(
  adsOptimization: () => AdsOptimizationSettings,
  firstLaunch: boolean,
): OverwolfSettings {
  const settings = {} as Record<string, unknown>;
  const define = (key: string, value: unknown): void => {
    Object.defineProperty(settings, key, { value: deepFreeze(value), enumerable: true });
  };
  define('src', 'https://www.overwolf.com/monsdk/electron/latest/adview.html');
  define('forceSandboxMode', false);
  Object.defineProperty(settings, 'adsOptimization', { get: adsOptimization, enumerable: true });
  define('adsSetting', {
    gvlUrlV1: 'https://content.overwolf.com/cmp',
    gvlUrl: 'https://content.overwolf.com/cmp/v3',
    cmpFeatureUrl: 'https://features.overwolf.com/experiments/cmp-eu-only',
    cmpUrl: 'https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/ow-cmp-v2.html',
    cmpSettingUrl: 'https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/cmp.html',
    cmpWindowUrl: '',
  });
  define('logger', { enabled: false });
  define('analytics', {
    analyticsUrl: 'https://analyticsnew.overwolf.com/analytics/Counter',
    trackingUrl: 'https://tracking.overwolf.com',
  });
  define('firstLaunch', firstLaunch);
  return Object.freeze(settings) as unknown as OverwolfSettings;
}

/**
 * The `setExternalPaymentUserId()` options as the wire carries them:
 * ow-electron appends the options as given to the report, so every own
 * enumerable key is copied in the app's order with its original value [OBS
 * R2-3]. Values JSON cannot carry (functions, symbols, bigints, `undefined`)
 * are left out, and so is an empty `providerName`, which the plugin then
 * defaults to `"tebex"` after the other fields [DEC].
 *
 * @param raw - the app's options
 * @returns the wire options
 */
function paymentOptions(raw: object): Record<string, unknown> {
  const wire: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(raw as Record<string, unknown>)) {
    if (
      value === undefined ||
      typeof value === 'function' ||
      typeof value === 'symbol' ||
      typeof value === 'bigint'
    )
      continue;
    if (key === 'providerName' && value === '') continue;
    // defineProperty, so a `__proto__` key stays a plain field.
    Object.defineProperty(wire, key, {
      value,
      enumerable: true,
      writable: true,
      configurable: true,
    });
  }
  return wire;
}

/**
 * The `app.overwolf` implementation. One instance per main webview, shared by
 * `ow-tauri/main` (`overwolf`) and `ow-tauri/electron` (`app.overwolf`).
 */
export class Overwolf implements OverwolfApi {
  /** The package manager (B.1.3). */
  readonly packages: PackageManager;
  /** ow-electron's internal settings object [OBS]. */
  readonly __settings__: OverwolfSettings;
  /**
   * Present on ow-electron's object and always `false`, also after
   * `disableAdsOptimization()` [OBS]; not in its typings.
   *
   * @internal
   */
  readonly enableAdsOptimization: boolean = false;
  readonly #kernel: FacadeKernel;
  #adsOptimization: AdsOptimizationSettings = Object.freeze({});

  /**
   * @param kernel - the runtime kernel
   * @internal
   */
  constructor(kernel: FacadeKernel) {
    this.#kernel = kernel;
    this.packages = new PackageManager(kernel);
    this.__settings__ = createSettings(
      () => this.#adsOptimization,
      kernel.state.get('firstLaunch') === true,
    );
    kernel.onReset(() => {
      this.#adsOptimization = Object.freeze({});
    });
  }

  /** App uid (G.2), from the state cache. */
  get uid(): string {
    return this.#string('identity.uid', 'app.overwolf.uid');
  }

  /**
   * Machine id (E.4), from the state cache: ow-electron's getter answers
   * `muidV2` (on Windows the per-install id, while analytics and the ad
   * guests carry `muid`) [OBS: Windows lab]; on macOS the two are equal.
   */
  get muid(): string {
    const v2 = this.#string('identity.muidV2', 'app.overwolf.muid');
    return v2 === '' ? this.#string('identity.muid', 'app.overwolf.muid') : v2;
  }

  /** Rollout bucket, 0 to 99 (E.4), from the state cache. */
  get phasePercent(): number {
    this.#kernel.require('main', 'app.overwolf.phasePercent');
    const value = this.#kernel.state.get('identity.phasePercent');
    return typeof value === 'number' ? value : 0;
  }

  /** `ow-electron.json` `utmParams`; `undefined` (not `null`) when there are none [OBS]. */
  get utmParams(): unknown {
    this.#kernel.require('main', 'app.overwolf.utmParams');
    const value = this.#kernel.state.get('utmParams');
    return value === null ? undefined : value;
  }

  /**
   * Disables anonymous analytics for this session. Recorded in the cache at
   * once; the command is sent before `main_ready` when called at startup.
   */
  disableAnonymousAnalytics(): void {
    this.#kernel.require('main', 'app.overwolf.disableAnonymousAnalytics');
    this.#kernel.state.set?.('flags.anonymousAnalyticsDisabled', true);
    this.#beforeReady(this.#fire('disable_anonymous_analytics'));
  }

  /** Disables ad optimisation for this session; `__settings__.adsOptimization.disable` becomes `true` [OBS]. */
  disableAdsOptimization(): void {
    this.#kernel.require('main', 'app.overwolf.disableAdsOptimization');
    this.#adsOptimization = Object.freeze({ ...this.#adsOptimization, disable: true });
    this.#kernel.state.set?.('flags.adsOptimizationDisabled', true);
    this.#beforeReady(this.#fire('disable_ads_optimization'));
  }

  /** Opts out of first-party data for this session; `__settings__.adsOptimization.anonymous` becomes `true` [OBS]. */
  disableAdsFPD(): void {
    this.#kernel.require('main', 'app.overwolf.disableAdsFPD');
    this.#adsOptimization = Object.freeze({ ...this.#adsOptimization, anonymous: true });
    this.#kernel.state.set?.('flags.adsFpdDisabled', true);
    this.#beforeReady(this.#fire('disable_ads_fpd'));
  }

  /**
   * Whether the user should be offered the consent settings window (D.6.2).
   * Never rejects; resolves `true` on any failure.
   *
   * @returns the answer
   */
  async isCMPRequired(): Promise<boolean> {
    try {
      this.#kernel.require('main', 'app.overwolf.isCMPRequired');
      return (await this.#kernel.command('is_cmp_required')) !== false;
    } catch (error) {
      this.#kernel.log('warn', `is_cmp_required failed: ${(error as Error).message}`);
      return true;
    }
  }

  /**
   * Opens the consent settings window (deprecated upstream; identical to
   * {@link Overwolf.openAdPrivacySettingsWindow}).
   *
   * @param options - window options
   * @returns resolves once the window exists
   */
  async openCMPWindow(options?: CMPWindowOptions): Promise<void> {
    this.#kernel.require('main', 'app.overwolf.openCMPWindow');
    await this.#kernel.command('open_cmp_window', { options: this.#cmpOptions(options) });
  }

  /**
   * Opens the ad privacy settings window (D.6.4). Resolves once the window
   * exists, not when it closes; a second call focuses the open window [OBS].
   *
   * @param options - window options
   * @returns resolves once the window exists
   */
  async openAdPrivacySettingsWindow(options?: CMPWindowOptions): Promise<void> {
    this.#kernel.require('main', 'app.overwolf.openAdPrivacySettingsWindow');
    await this.#kernel.command('open_ad_privacy_settings_window', {
      options: this.#cmpOptions(options),
    });
  }

  /**
   * Hashes an email address synchronously and, as ow-electron does, sends
   * the hashes to every existing ad view (`eHashes`) [OBS]. Empty or
   * whitespace input returns `{}` and sends nothing [DEC].
   *
   * @param email - the address; it is not stored
   * @returns the hashes, keys in the order `sha1`, `md5`, `sha256`
   */
  generateUserEmailHashes(email: string): EmailHashes {
    this.#kernel.require('main', 'app.overwolf.generateUserEmailHashes');
    const hashes = emailHashes(typeof email === 'string' ? email : String(email));
    if (hashes.sha1 !== undefined) this.#sendHashes(hashes);
    return hashes;
  }

  /**
   * Sends email hashes to every existing ad view (`eHashes`, D.5).
   *
   * @param emailHashes - the hashes; absent or empty sends nothing (decided by the plugin)
   */
  setUserEmailHashes(emailHashes?: EmailHashes): void {
    this.#kernel.require('main', 'app.overwolf.setUserEmailHashes');
    this.#sendHashes(emailHashes);
  }

  /**
   * Reports the user's id at an external payment provider (E.2 `sub_info`).
   * The options reach the report as given: same keys, order and values
   * [OBS]. Rejects (never throws) with {@link PAYMENT_ID_MANDATORY} without a
   * `userId`, and with {@link NOT_READY_MESSAGE} before `app.ready`; a failed
   * report still resolves [OBS].
   *
   * @param options - provider, user id and payment id
   * @returns resolves after the report
   */
  async setExternalPaymentUserId(options: ExternalPaymentUserIdOptions): Promise<void> {
    this.#kernel.require('main', 'app.overwolf.setExternalPaymentUserId');
    const raw: Partial<Record<keyof ExternalPaymentUserIdOptions, unknown>> =
      typeof options === 'object' && (options as unknown) !== null ? options : {};
    const userId = raw.userId;
    if (
      userId === undefined ||
      userId === null ||
      userId === '' ||
      (typeof userId !== 'string' && typeof userId !== 'number')
    )
      throw new Error(PAYMENT_ID_MANDATORY);
    if (!this.#kernel.isReady) throw new Error(NOT_READY_MESSAGE);
    const wire = paymentOptions(raw);
    try {
      await this.#kernel.command('set_external_payment_user_id', { options: wire });
    } catch (error) {
      if (
        error instanceof OwTauriError &&
        (error.code === 'invalid-argument' || error.code === 'not-ready') &&
        !isPreCommandRejection(error)
      ) {
        const data = error.data as { message?: unknown } | undefined;
        throw new Error(
          typeof data?.message === 'string'
            ? data.message
            : error.code === 'not-ready'
              ? NOT_READY_MESSAGE
              : PAYMENT_ID_MANDATORY,
          { cause: error },
        );
      }
      this.#kernel.log('warn', `set_external_payment_user_id failed: ${(error as Error).message}`);
    }
  }

  /**
   * Present on ow-electron's object, not in its typings [OBS]: resolves once
   * the app is ready (`main_ready` acknowledged) [DEC].
   *
   * @returns resolves when ready
   * @internal
   */
  async assureOWElectronIsReady(): Promise<void> {
    this.#kernel.require('main', 'app.overwolf.assureOWElectronIsReady');
    await this.#kernel.whenHostReady();
  }

  /**
   * Present on ow-electron's object, not in its typings, semantics not
   * observed [OBS]: has no effect in ow-tauri and logs a warning once.
   *
   * @param _url - ignored
   * @internal
   */
  overrideAdViewUrl(_url: unknown): void {
    this.#kernel.require('main', 'app.overwolf.overrideAdViewUrl');
    this.#kernel.warnOnce(
      'app.overwolf.overrideAdViewUrl',
      'app.overwolf.overrideAdViewUrl() is internal to ow-electron and has no effect in ow-tauri',
    );
  }

  /**
   * Present on ow-electron's object, not in its typings, semantics not
   * observed [OBS]: has no effect in ow-tauri and logs a warning once; use
   * {@link Overwolf.setUserEmailHashes}.
   *
   * @param _hashes - ignored
   * @internal
   */
  storeEmailHashes(_hashes: unknown): void {
    this.#kernel.require('main', 'app.overwolf.storeEmailHashes');
    this.#kernel.warnOnce(
      'app.overwolf.storeEmailHashes',
      'app.overwolf.storeEmailHashes() is internal to ow-electron and has no effect in ow-tauri; use setUserEmailHashes()',
    );
  }

  #sendHashes(hashes: EmailHashes | undefined): void {
    let wire: Record<string, string> | null = null;
    if (typeof hashes === 'object' && (hashes as EmailHashes | null) !== null) {
      wire = {};
      for (const key of ['sha1', 'md5', 'sha256'] as const) {
        const value = hashes[key];
        if (typeof value === 'string') wire[key] = value;
      }
    }
    void this.#fire('set_user_email_hashes', { hashes: wire });
  }

  #cmpOptions(options: CMPWindowOptions | undefined): Record<string, unknown> | null {
    if (typeof options !== 'object' || (options as CMPWindowOptions | null) === null) return null;
    const wire: Record<string, unknown> = {};
    for (const key of CMP_KEYS) if (options[key] !== undefined) wire[key] = options[key];
    const parent = options.parent;
    if (parent !== undefined) {
      const parentId =
        typeof parent === 'object' && typeof parent.id === 'number'
          ? this.#kernel.windowIds.toHost(parent.id)
          : undefined;
      if (parentId === undefined)
        this.#kernel.log(
          'warn',
          'CMPWindowOptions.parent is not an open BrowserWindow; the consent window opens without a parent',
        );
      else wire['parentId'] = parentId;
    }
    return wire;
  }

  #string(path: string, api: string): string {
    this.#kernel.require('main', api);
    const value = this.#kernel.state.get(path);
    return typeof value === 'string' ? value : '';
  }

  #fire(command: string, args?: Record<string, unknown>): Promise<void> {
    return this.#kernel.command(command, args).then(
      () => undefined,
      (error: unknown) => {
        this.#kernel.log('warn', `${command} failed: ${(error as Error).message}`);
      },
    );
  }

  /** Holds `main_ready` until a startup call was acknowledged (see `FacadeKernel.deferMainReady`). */
  #beforeReady(task: Promise<void>): void {
    if (!this.#kernel.isReady) this.#kernel.deferMainReady?.(task);
  }
}

/**
 * The document's `app.overwolf` singleton.
 *
 * @param kernel - the runtime kernel
 * @returns the shared instance
 * @internal
 */
export function overwolfOf(kernel: FacadeKernel): Overwolf {
  return kernel.singleton('main.overwolf', () => new Overwolf(kernel));
}
