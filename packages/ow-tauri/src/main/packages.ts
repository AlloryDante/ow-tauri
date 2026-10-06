/**
 * `app.overwolf.packages` (`docs/CONTRACT.md` B.1.2, B.1.3 and H): the
 * package manager as ow-electron 42.11.4 behaves on a host where packages are
 * not available [OBS].
 *
 * No event is emitted, `hasPendingUpdates()` answers synchronously,
 * `getChannel()` resolves `{}`, `setChannel()` and `getAvailableChannels()`
 * reject asynchronously with ow-electron's exact messages, and the package
 * objects (`gep`, `overlay`, ...) are `undefined`. The emitter still accepts
 * listeners, so code written for ow-electron runs unchanged. If a future
 * package runtime sends `packages` host messages (Appendix P.7), they are
 * emitted with a synthetic Electron `Event` as the first argument (B.1.2).
 *
 * @packageDocumentation
 */
import type { FacadeKernel } from '../bootstrap/facade-kernel.js';
import { EventEmitter, emitFromHost } from '../shared/emitter.js';
import { OwTauriError } from '../shared/errors.js';
import type {
  AvailableChannelsResult,
  ChannelPackageInfo,
  CurrentChannelsResult,
  OverwolfPackageManager,
  PackageName,
  PendingUpdatesResult,
  SetChannelResult,
} from './overwolf-types.js';

/** The synthetic Electron `Event` passed as the first listener argument (CONTRACT B.1.2). */
export interface SyntheticEvent {
  /** Marks the event as handled (for `crashed`: do not relaunch the package). */
  preventDefault(): void;
  /** Whether {@link SyntheticEvent.preventDefault} was called. */
  readonly defaultPrevented: boolean;
}

/**
 * Creates a {@link SyntheticEvent}.
 *
 * @param onPrevent - called once, on the first `preventDefault()`
 * @returns the event
 */
export function syntheticEvent(onPrevent?: () => void): SyntheticEvent {
  let prevented = false;
  return {
    preventDefault: () => {
      if (prevented) return;
      prevented = true;
      onPrevent?.();
    },
    get defaultPrevented() {
      return prevented;
    },
  };
}

/** ow-electron's rejection text for a package it cannot load [OBS]. */
function notRegistered(method: string, name: unknown): Error {
  return new Error(`${method} - package '${String(name)}' is not registered in this app`);
}

/**
 * The plain `Error` an A.2.4 command rejection stands for: the plugin's
 * `data.message` when present (ow-electron's exact text), else `fallback`.
 */
function packageError(error: unknown, fallback: Error): Error {
  if (error instanceof OwTauriError) {
    const data = error.data as { message?: unknown } | undefined;
    if (typeof data?.message === 'string') return new Error(data.message, { cause: error });
  }
  return fallback;
}

/** Fields of a `packages` host message (Appendix P.7). */
interface PackagesMessage {
  event?: unknown;
  name?: unknown;
  version?: unknown;
  info?: unknown;
  canRecover?: unknown;
  eventId?: unknown;
}

/**
 * The package manager (CONTRACT B.1.3). Created once per main webview with
 * the `app.overwolf` object; app code reaches it as `app.overwolf.packages`.
 */
export class PackageManager extends EventEmitter implements OverwolfPackageManager {
  readonly #kernel: FacadeKernel;

  /**
   * @param kernel - the runtime kernel
   * @internal
   */
  constructor(kernel: FacadeKernel) {
    super();
    this.#kernel = kernel;
    kernel.on('packages', (message) => {
      this.#onHostMessage(message as PackagesMessage);
    });
    kernel.onReset(() => this.removeAllListeners());
  }

  /** Relaunches the package manager: no effect while no package is loaded [OBS]. */
  relaunch(): void {
    this.#kernel.require('main', 'app.overwolf.packages.relaunch');
    this.#kernel.command('packages_relaunch').catch((error: unknown) => {
      this.#kernel.log('debug', `packages_relaunch failed: ${(error as Error).message}`);
    });
  }

  /**
   * Pending package updates, answered synchronously from the state cache:
   * `{ hasPendingUpdate: false, details: [] }` while no package runtime exists [OBS].
   *
   * @returns a fresh result object
   */
  hasPendingUpdates(): PendingUpdatesResult {
    this.#kernel.require('main', 'app.overwolf.packages.hasPendingUpdates');
    const cached = this.#kernel.state.get('packages.pendingUpdates') as
      { hasPendingUpdate?: unknown; details?: unknown } | undefined;
    const details = Array.isArray(cached?.details)
      ? (cached.details as unknown[]).filter(
          (d): d is { name: string; version: string } =>
            typeof d === 'object' &&
            d !== null &&
            typeof (d as { name?: unknown }).name === 'string' &&
            typeof (d as { version?: unknown }).version === 'string',
        )
      : [];
    return {
      hasPendingUpdate: cached?.hasPendingUpdate === true,
      details: details.map((d) => ({ name: d.name, version: d.version })),
    };
  }

  /**
   * Switches a package to a release channel. While no package runtime
   * exists it rejects asynchronously with
   * `Error("setChannel - package '<name>' is not registered in this app")` [OBS].
   *
   * @param packageName - the package
   * @param channel - the channel
   * @param _ready - called when a download completes (never, without a runtime)
   * @returns the result
   */
  async setChannel(
    packageName: PackageName,
    channel?: string,
    _ready?: (packageInfo: ChannelPackageInfo) => void,
  ): Promise<SetChannelResult> {
    this.#kernel.require('main', 'app.overwolf.packages.setChannel');
    try {
      const result = await this.#kernel.command('packages_set_channel', {
        name: String(packageName),
        channel: channel ?? null,
      });
      if (typeof result === 'object' && result !== null) return result as SetChannelResult;
    } catch (error) {
      throw packageError(error, notRegistered('setChannel', packageName));
    }
    throw notRegistered('setChannel', packageName);
  }

  /**
   * The release channels per package. With names it rejects asynchronously
   * with ow-electron's error for the first name,
   * `getAvailableChannels - package 'gep' is not registered in this app`
   * [OBS]; with no names it resolves an empty object [DEC].
   *
   * @param packageNames - the packages
   * @returns the channels
   */
  async getAvailableChannels(...packageNames: PackageName[]): Promise<AvailableChannelsResult> {
    this.#kernel.require('main', 'app.overwolf.packages.getAvailableChannels');
    const names = packageNames.map(String);
    let result: unknown;
    try {
      result = await this.#kernel.command('packages_get_available_channels', { names });
    } catch (error) {
      if (names.length === 0) return {};
      throw packageError(error, notRegistered('getAvailableChannels', names[0]));
    }
    if (names.length > 0 && (typeof result !== 'object' || result === null))
      throw notRegistered('getAvailableChannels', names[0]);
    return typeof result === 'object' && result !== null ? (result as AvailableChannelsResult) : {};
  }

  /**
   * The active release channel per package: `{}` while no package runtime
   * exists, for any arguments [OBS].
   *
   * @param packageNames - the packages
   * @returns the channels
   */
  async getChannel(...packageNames: PackageName[]): Promise<CurrentChannelsResult> {
    this.#kernel.require('main', 'app.overwolf.packages.getChannel');
    try {
      const result = await this.#kernel.command('packages_get_channel', {
        names: packageNames.map(String),
      });
      return typeof result === 'object' && result !== null ? (result as CurrentChannelsResult) : {};
    } catch {
      return {};
    }
  }

  /**
   * The literal string ow-electron reports:
   * `<userData>/..\ow-electron/<uid>/logs`, backslash included, on every OS
   * (F.4) [OBS].
   */
  get logsFolderPath(): string {
    this.#kernel.require('main', 'app.overwolf.packages.logsFolderPath');
    const cached = this.#kernel.state.get('packages.logsFolderPath');
    if (typeof cached === 'string') return cached;
    const userData = this.#kernel.state.get('paths.userData');
    const uid = this.#kernel.state.get('identity.uid');
    return `${typeof userData === 'string' ? userData : ''}/..\\ow-electron/${typeof uid === 'string' ? uid : ''}/logs`;
  }

  /** Rollout bucket, 0 to 99 (E.4). */
  get phasePercent(): number {
    this.#kernel.require('main', 'app.overwolf.packages.phasePercent');
    const cached = this.#kernel.state.get('packages.phasePercent');
    if (typeof cached === 'number') return cached;
    const identity = this.#kernel.state.get('identity.phasePercent');
    return typeof identity === 'number' ? identity : 0;
  }

  /**
   * Emits a `packages` host message of a package runtime (Appendix P.7) with
   * the synthetic `Event` first, in the upstream listener signatures.
   */
  #onHostMessage(message: PackagesMessage): void {
    const name = message.event;
    if (typeof name !== 'string') return;
    const eventId = typeof message.eventId === 'number' ? message.eventId : undefined;
    const event = syntheticEvent(
      eventId === undefined
        ? undefined
        : () => {
            this.#kernel
              .command('package_event_action', { eventId, action: 'prevent-default' })
              .catch((error: unknown) => {
                this.#kernel.log(
                  'warn',
                  `package_event_action failed: ${(error as Error).message}`,
                );
              });
          },
    );
    switch (name) {
      case 'loading':
      case 'failed-to-initialize':
        emitFromHost(this, name, event, message.name);
        return;
      case 'ready':
      case 'updated':
        emitFromHost(this, name, event, message.name, message.version);
        return;
      case 'crashed':
        emitFromHost(this, name, event, message.canRecover === true);
        return;
      case 'package-update-pending':
        emitFromHost(this, name, event, Array.isArray(message.info) ? message.info : []);
        return;
      default:
        this.#kernel.log('debug', `unknown packages event '${name}'`);
    }
  }
}
