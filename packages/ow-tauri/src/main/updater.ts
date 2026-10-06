/**
 * `autoUpdater` (`docs/CONTRACT.md` section I.5): the subset of
 * electron-updater's `AppUpdater` that ow-electron apps use, over the
 * plugin's `updater_*` commands (A.2.8) and `updater` host messages (I.3).
 *
 * The update client itself (feed, download, verification, install) runs in
 * Rust ([ADR 0008](../../../../docs/adr/0008-updater-client.md)); this object
 * keeps the electron-updater properties, sends them as `updater_configure`
 * and re-emits the host's events.
 *
 * @packageDocumentation
 */
import type { FacadeKernel } from '../bootstrap/facade-kernel.js';
import { EventEmitter, emitFromHost } from '../shared/emitter.js';
import { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
import type { HostMessage } from '../shared/protocol.js';
import { fromWireError, isPreCommandRejection } from '../shared/wire-error.js';

/** The configuration `updater_configure` receives (CONTRACT I.1). */
export interface UpdaterConfig {
  /** The only supported provider. */
  provider: 'generic';
  /**
   * The feed base URL: https, http only for localhost in debug builds.
   * Absent only when `forceDevUpdateConfig` makes the client read the
   * embedded `dev-app-update.yml`.
   */
  url?: string;
  /** Feed channel; the client reads `<channel>.yml`. Default `latest`. */
  channel?: string;
  /** Whether an older feed version counts as an update. Default `false`. */
  allowDowngrade?: boolean;
  /** Whether prerelease versions count as updates. Default `false`. */
  allowPrerelease?: boolean;
  /** Download an available update automatically. Default `true`. */
  autoDownload?: boolean;
  /** Install a downloaded update on normal exit. Default `true`. */
  autoInstallOnAppQuit?: boolean;
  /** Check in debug builds, reading the embedded `dev-app-update.yml`. */
  forceDevUpdateConfig?: boolean;
  /** Extra request headers for feed and download requests. */
  requestHeaders?: Record<string, string>;
}

/** Options of {@link AppUpdater.setFeedURL}: electron-updater's generic provider options. */
export interface GenericFeedOptions {
  /** Must be `'generic'`: other providers throw `OwTauriUnsupportedError`. */
  provider: 'generic';
  /** The feed base URL. */
  url: string;
  /** Feed channel, used while {@link AppUpdater.channel} is not set. */
  channel?: string | null;
  /** Extra request headers. */
  requestHeaders?: Record<string, string>;
}

/** One installer file of a feed entry. */
export interface UpdateFileInfo {
  /** File URL, absolute or relative to the feed URL. */
  url: string;
  /** Base64 SHA-512 of the file. */
  sha512: string;
  /** Size in bytes. */
  size?: number;
  /** Block map size; ignored, updates are full downloads (I.2). */
  blockMapSize?: number;
  /** Whether the installer must run elevated (`IsAdminRightsRequired` in Overwolf's feed). */
  isAdminRightsRequired?: boolean;
}

/** A release note of one version. */
export interface ReleaseNoteInfo {
  /** The version. */
  readonly version: string;
  /** The note. */
  readonly note: string | null;
}

/** A parsed feed entry (electron-updater's `UpdateInfo`). */
export interface UpdateInfo {
  /** The feed version. */
  readonly version: string;
  /** Installer files. */
  readonly files: UpdateFileInfo[];
  /** Legacy top-level file path. */
  readonly path?: string;
  /** Legacy top-level SHA-512. */
  readonly sha512?: string;
  /** Release name. */
  releaseName?: string | null;
  /** Release notes. */
  releaseNotes?: string | ReleaseNoteInfo[] | null;
  /** Release date (ISO 8601). */
  releaseDate: string;
  /** Staged rollout percentage. */
  readonly stagingPercentage?: number;
  /** Path of the downloaded file, on `update-downloaded`. */
  downloadedFile?: string;
}

/** What `checkForUpdates()` resolves. */
export interface UpdateCheckResult {
  /** Whether the feed version is an update for this build. */
  readonly isUpdateAvailable: boolean;
  /** The feed entry. */
  readonly updateInfo: UpdateInfo;
  /** Same as `updateInfo` (electron-updater keeps both). */
  readonly versionInfo: UpdateInfo;
}

/** `download-progress` payload. */
export interface ProgressInfo {
  /** Total bytes. */
  total: number;
  /** Bytes since the previous event. */
  delta?: number;
  /** Bytes received. */
  transferred: number;
  /** Percent done, 0 to 100. */
  percent: number;
  /** Current rate. */
  bytesPerSecond: number;
}

/** A logger: any object with these methods (`console` works). */
export interface UpdaterLogger {
  /**
   * Informational message.
   *
   * @param message - one string per message
   */
  info(message?: unknown): void;
  /**
   * Warning.
   *
   * @param message - one string per message
   */
  warn(message?: unknown): void;
  /**
   * Error.
   *
   * @param message - one string per message
   */
  error(message?: unknown): void;
  /**
   * Debug message; optional.
   *
   * @param message - one string per message
   */
  debug?(message: string): void;
}

/** electron-updater's `currentVersion`: the running app version. */
export interface CurrentVersion {
  /** The full version string, e.g. `1.2.3-beta.1`. */
  readonly version: string;
  /** Major number. */
  readonly major: number;
  /** Minor number. */
  readonly minor: number;
  /** Patch number. */
  readonly patch: number;
  /** Prerelease identifiers. */
  readonly prerelease: readonly (string | number)[];
  /**
   * The version string.
   *
   * @returns `version`
   */
  toString(): string;
}

type UpdaterEvent =
  | 'checking-for-update'
  | 'update-available'
  | 'update-not-available'
  | 'download-progress'
  | 'update-downloaded'
  | 'error';

interface UpdaterMessage {
  type: 'updater';
  event?: unknown;
  info?: unknown;
  progress?: unknown;
  error?: unknown;
}

const SKIP_MESSAGE =
  'Skip checkForUpdates because application is not packed and dev update config is not forced';

const silent: UpdaterLogger = Object.freeze({
  info: () => undefined,
  warn: () => undefined,
  error: () => undefined,
});

function versionOf(info: unknown): string {
  const version = (info as { version?: unknown } | null | undefined)?.version;
  return typeof version === 'string' ? version : String(version);
}

function parseVersion(version: string): CurrentVersion {
  const match = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?/.exec(version);
  const prerelease = (match?.[4]?.split('.') ?? []).map((part) =>
    /^\d+$/.test(part) ? Number(part) : part,
  );
  return Object.freeze({
    version,
    major: Number(match?.[1] ?? 0),
    minor: Number(match?.[2] ?? 0),
    patch: Number(match?.[3] ?? 0),
    prerelease: Object.freeze(prerelease),
    toString: () => version,
  });
}

/**
 * electron-updater's `AppUpdater`, as far as CONTRACT I.5 provides it.
 * Events: `checking-for-update`, `update-available(info)`,
 * `update-not-available(info)`, `download-progress(progress)`,
 * `update-downloaded(info)`, `error(error, message)`.
 */
export class AppUpdater extends EventEmitter {
  readonly #kernel: FacadeKernel;
  #autoDownload = true;
  #autoInstallOnAppQuit = true;
  #allowDowngrade = false;
  #allowPrerelease = false;
  #forceDevUpdateConfig = false;
  #channel: string | null = null;
  #logger: UpdaterLogger = console;
  #feed: GenericFeedOptions | null = null;
  #dirty = true;
  #sync: Promise<void> = Promise.resolve();
  #flushQueued = false;
  #checkPromise: Promise<UpdateCheckResult | null> | null = null;
  /** A check runs whose failure the plugin reports as an `error` message. */
  #checkErrorPending = false;

  /**
   * @param kernel - the runtime kernel
   * @internal
   */
  constructor(kernel: FacadeKernel) {
    super();
    this.#kernel = kernel;
    kernel.on('updater', (message: HostMessage) => {
      this.#onHostMessage(message as UpdaterMessage);
    });
    kernel.onReset(() => {
      this.removeAllListeners();
      this.#autoDownload = true;
      this.#autoInstallOnAppQuit = true;
      this.#allowDowngrade = false;
      this.#allowPrerelease = false;
      this.#forceDevUpdateConfig = false;
      this.#channel = null;
      this.#logger = console;
      this.#feed = null;
      this.#dirty = true;
      this.#sync = Promise.resolve();
      this.#flushQueued = false;
      this.#checkPromise = null;
      this.#checkErrorPending = false;
    });
  }

  /** Download an available update automatically (default `true`). */
  get autoDownload(): boolean {
    return this.#autoDownload;
  }

  /**
   * Sets {@link AppUpdater.autoDownload}.
   *
   * @param value - the new value
   */
  set autoDownload(value: boolean) {
    this.#autoDownload = value;
    this.#changed();
  }

  /** Install a downloaded update when the app quits normally (default `true`). */
  get autoInstallOnAppQuit(): boolean {
    return this.#autoInstallOnAppQuit;
  }

  /**
   * Sets {@link AppUpdater.autoInstallOnAppQuit}.
   *
   * @param value - the new value
   */
  set autoInstallOnAppQuit(value: boolean) {
    this.#autoInstallOnAppQuit = value;
    this.#changed();
  }

  /** Whether an older feed version counts as an update (default `false`). */
  get allowDowngrade(): boolean {
    return this.#allowDowngrade;
  }

  /**
   * Sets {@link AppUpdater.allowDowngrade}.
   *
   * @param value - the new value
   */
  set allowDowngrade(value: boolean) {
    this.#allowDowngrade = value;
    this.#changed();
  }

  /** Whether prerelease versions count as updates (default `false`). */
  get allowPrerelease(): boolean {
    return this.#allowPrerelease;
  }

  /**
   * Sets {@link AppUpdater.allowPrerelease}.
   *
   * @param value - the new value
   */
  set allowPrerelease(value: boolean) {
    this.#allowPrerelease = value;
    this.#changed();
  }

  /**
   * Check in debug builds too (default `false`). In electron-updater it
   * makes an unpackaged app check for updates; the plugin reads the embedded
   * `dev-app-update.yml` when no feed URL was set (I.1).
   */
  get forceDevUpdateConfig(): boolean {
    return this.#forceDevUpdateConfig;
  }

  /**
   * Sets {@link AppUpdater.forceDevUpdateConfig}.
   *
   * @param value - the new value
   */
  set forceDevUpdateConfig(value: boolean) {
    this.#forceDevUpdateConfig = value;
    this.#changed();
  }

  /**
   * The feed channel, or `null` for the default (`latest`). As in
   * electron-updater, assigning it also sets `allowDowngrade` to `true`.
   */
  get channel(): string | null {
    return this.#channel;
  }

  /**
   * Sets the channel; also sets `allowDowngrade` to `true`.
   *
   * @param value - the new value
   */
  set channel(value: string | null) {
    if (this.#channel !== null) {
      if (typeof value !== 'string')
        throw new OwTauriError(
          'invalid-argument',
          `Channel must be a string, but got: ${String(value)}`,
        );
      if (value.length === 0)
        throw new OwTauriError('invalid-argument', 'Channel must be not an empty string');
    }
    this.#channel = value;
    this.#allowDowngrade = true;
    this.#changed();
  }

  /** The logger (default `console`); `null` silences it. */
  get logger(): UpdaterLogger | null {
    return this.#logger === silent ? null : this.#logger;
  }

  /**
   * Sets the logger; `null` silences it.
   *
   * @param value - the new value
   */
  set logger(value: UpdaterLogger | null) {
    this.#logger = value ?? silent;
  }

  /** The running app version (`package.json` `version`). */
  get currentVersion(): CurrentVersion {
    const version = this.#kernel.state.get('manifest.version');
    return parseVersion(typeof version === 'string' ? version : '0.0.0');
  }

  /**
   * Sets the update feed. A string is a generic-provider URL.
   *
   * @param options - a URL, or generic provider options
   * @throws `OwTauriUnsupportedError` for a provider other than `generic`
   */
  setFeedURL(options: GenericFeedOptions | string): void {
    this.#kernel.require('main', 'autoUpdater.setFeedURL');
    const feed: GenericFeedOptions =
      typeof options === 'string' ? { provider: 'generic', url: options } : { ...options };
    if ((feed.provider as string) !== 'generic')
      throw new OwTauriUnsupportedError(
        'autoUpdater.setFeedURL',
        `provider '${feed.provider}' (ow-tauri reads generic feeds only, CONTRACT I.1)`,
      );
    if (typeof feed.url !== 'string' || feed.url.length === 0)
      throw new OwTauriError('invalid-argument', 'setFeedURL: url must be a non-empty string');
    this.#feed = feed;
    this.#changed();
  }

  /**
   * Checks the feed. Resolves `null` without a request when the build is
   * not packaged and `forceDevUpdateConfig` is off, as electron-updater does,
   * or when the plugin disables updates for this build. With `autoDownload`
   * the plugin starts the download itself (`update-downloaded` follows).
   *
   * While a check is in progress, another call returns the same promise and
   * sends no second request, as electron-updater does. A failed check emits
   * `error(error, 'Cannot check for updates: ' + stack)`.
   *
   * @returns the check result, or `null`
   */
  checkForUpdates(): Promise<UpdateCheckResult | null> {
    try {
      this.#kernel.require('main', 'autoUpdater.checkForUpdates');
    } catch (error) {
      return Promise.reject(error instanceof Error ? error : new Error(String(error)));
    }
    if (!this.#active()) return Promise.resolve(null);
    const pending = this.#checkPromise;
    if (pending) {
      this.#logger.info('Checking for update (already in progress)');
      return pending;
    }
    this.#logger.info('Checking for update');
    this.#checkErrorPending = true;
    const check: Promise<UpdateCheckResult | null> = this.#check().then(
      (result) => {
        if (this.#checkPromise === check) this.#checkPromise = null;
        // `null`: the plugin disabled updates and sent no event.
        if (result === null) this.#checkErrorPending = false;
        return result;
      },
      (error: unknown) => {
        if (this.#checkPromise === check) this.#checkPromise = null;
        throw error;
      },
    );
    this.#checkPromise = check;
    return check;
  }

  /**
   * Same as {@link AppUpdater.checkForUpdates}; no OS notification is shown
   * (partial, CONTRACT I.5).
   *
   * @returns the check result, or `null`
   */
  checkForUpdatesAndNotify(): Promise<UpdateCheckResult | null> {
    this.#kernel.require('main', 'autoUpdater.checkForUpdatesAndNotify');
    return this.checkForUpdates();
  }

  /**
   * Downloads the update the last check found and verifies it (I.3).
   *
   * @returns the downloaded file paths
   */
  async downloadUpdate(): Promise<string[]> {
    this.#kernel.require('main', 'autoUpdater.downloadUpdate');
    await this.#configured('updater_download');
    const files = await this.#call('updater_download');
    return Array.isArray(files) ? files.filter((f): f is string => typeof f === 'string') : [];
  }

  /**
   * Quits the app and runs the downloaded installer (I.4). Failures are
   * emitted as `error`.
   *
   * @param isSilent - Windows: run the installer silently (default `false`)
   * @param isForceRunAfter - start the app after the install (default `false`)
   */
  quitAndInstall(isSilent = false, isForceRunAfter = false): void {
    this.#kernel.require('main', 'autoUpdater.quitAndInstall');
    this.#logger.info('Install on explicit quitAndInstall');
    this.#call('updater_quit_and_install', { isSilent, isForceRunAfter }).catch(() => undefined);
  }

  async #check(): Promise<UpdateCheckResult | null> {
    await this.#configured('updater_check', true);
    const result = await this.#call('updater_check', undefined, true);
    return (result ?? null) as UpdateCheckResult | null;
  }

  #active(): boolean {
    if (this.#kernel.state.get('isPackaged') === true || this.#forceDevUpdateConfig) return true;
    this.#logger.info(SKIP_MESSAGE);
    return false;
  }

  #config(): UpdaterConfig {
    const config: UpdaterConfig = {
      provider: 'generic',
      allowDowngrade: this.#allowDowngrade,
      allowPrerelease: this.#allowPrerelease,
      autoDownload: this.#autoDownload,
      autoInstallOnAppQuit: this.#autoInstallOnAppQuit,
      forceDevUpdateConfig: this.#forceDevUpdateConfig,
    };
    const feed = this.#feed;
    if (feed) config.url = feed.url;
    const channel = this.#channel ?? feed?.channel ?? null;
    if (channel !== null) config.channel = channel;
    if (feed?.requestHeaders) config.requestHeaders = { ...feed.requestHeaders };
    return config;
  }

  #changed(): void {
    this.#dirty = true;
    // Keeps the plugin current for `autoInstallOnAppQuit` changed after a
    // download; check and download send the config themselves.
    if (this.#feed === null || this.#flushQueued) return;
    this.#flushQueued = true;
    queueMicrotask(() => {
      this.#flushQueued = false;
      this.#configure().catch((error: unknown) => {
        this.#kernel.log('warn', `updater_configure failed: ${(error as Error).message}`);
      });
    });
  }

  #configure(): Promise<void> {
    if (!this.#dirty) return this.#sync;
    this.#dirty = false;
    const config = this.#config();
    const run = this.#sync.then(async () => {
      await this.#kernel.command('updater_configure', { ...config });
    });
    this.#sync = run.catch(() => {
      this.#dirty = true;
    });
    return run;
  }

  /**
   * Makes sure the plugin has the current configuration.
   *
   * @param command - the command about to run, for the error message
   * @param check - whether a check runs (error message prefix)
   */
  async #configured(command: string, check = false): Promise<void> {
    if (this.#feed === null && !this.#forceDevUpdateConfig) {
      const error = new OwTauriError(
        'invalid-argument',
        `${command}: no update feed; call autoUpdater.setFeedURL() first (ow-tauri has no app-update.yml)`,
      );
      this.#dispatchError(error, check);
      throw error;
    }
    try {
      await this.#configure();
    } catch (raw) {
      const error = fromWireError(raw, 'updater_configure');
      this.#dispatchError(error, check);
      throw error;
    }
  }

  /**
   * Runs an updater command.
   *
   * @param command - the command
   * @param args - its arguments
   * @param check - whether it is `updater_check` (error message prefix)
   */
  async #call(command: string, args?: Record<string, unknown>, check = false): Promise<unknown> {
    try {
      return await this.#kernel.command(command, args);
    } catch (raw) {
      const error = fromWireError(raw, command);
      // The plugin emits `error` for failures it handled; a call Tauri
      // rejected before the command ran never reached it.
      if (isPreCommandRejection(error) || error.code === 'unsupported')
        this.#dispatchError(error, check);
      throw error;
    }
  }

  /**
   * Logs and emits `error(error, message)` as electron-updater does; a
   * failed check prefixes the message with `Cannot check for updates: `.
   *
   * @param error - the error
   * @param check - whether a check failed
   */
  #dispatchError(error: Error, check = false): void {
    if (check) this.#checkErrorPending = false;
    this.#logger.error(`Error: ${error.stack ?? error.message}`);
    const detail = error.stack ?? String(error);
    emitFromHost(this, 'error', error, check ? `Cannot check for updates: ${detail}` : detail);
  }

  #onHostMessage(message: UpdaterMessage): void {
    const event = message.event as UpdaterEvent;
    switch (event) {
      case 'checking-for-update':
        emitFromHost(this, event);
        return;
      case 'update-available': {
        this.#checkErrorPending = false;
        const info = message.info as Partial<UpdateInfo> | undefined;
        const urls = Array.isArray(info?.files) ? info.files.map((f) => f.url).join(', ') : '';
        this.#logger.info(`Found version ${versionOf(info)} (url: ${urls})`);
        emitFromHost(this, event, info);
        return;
      }
      case 'update-not-available': {
        this.#checkErrorPending = false;
        const info = message.info;
        this.#logger.info(
          `Update for version ${this.currentVersion.version} is not available (latest version: ${versionOf(info)}, downgrade is ${this.#allowDowngrade ? 'allowed' : 'disallowed'}).`,
        );
        emitFromHost(this, event, info);
        return;
      }
      case 'download-progress':
        emitFromHost(this, event, message.progress);
        return;
      case 'update-downloaded': {
        const info = message.info as Partial<UpdateInfo> | undefined;
        const file = typeof info?.downloadedFile === 'string' ? ` to ${info.downloadedFile}` : '';
        this.#logger.info(`New version ${versionOf(info)} has been downloaded${file}`);
        emitFromHost(this, event, info);
        return;
      }
      case 'error':
        // The check's own failure comes first: the plugin reports nothing
        // else for it before update-available or update-not-available.
        this.#dispatchError(fromWireError(message.error, 'autoUpdater'), this.#checkErrorPending);
        return;
      default:
        this.#kernel.log('debug', `unknown updater event: ${String(message.event)}`);
    }
  }
}

/**
 * The `autoUpdater` singleton of a kernel.
 *
 * @param kernel - the kernel
 * @returns the updater
 * @internal
 */
export function autoUpdaterOf(kernel: FacadeKernel): AppUpdater {
  return kernel.singleton('main.autoUpdater', () => new AppUpdater(kernel));
}
