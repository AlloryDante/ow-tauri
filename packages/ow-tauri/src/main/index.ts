/**
 * `ow-tauri/main`: the host runtime for the hidden main webview
 * (`docs/CONTRACT.md` section B.1). Using it anywhere else throws
 * `OwTauriError('forbidden')`.
 *
 * @packageDocumentation
 */
import type { FacadeKernel } from '../bootstrap/facade-kernel.js';
import { attachRuntime } from '../bootstrap/install.js';
import { createFiles, type Files } from './files.js';
import { overwolfOf, type Overwolf } from './overwolf.js';

const kernel: FacadeKernel = attachRuntime();

/**
 * The `app.overwolf` object (B.1.1); `ow-tauri/electron`'s `app.overwolf` is
 * the same instance. Members check that they run in the main webview when
 * used, not at import.
 *
 * @example
 * ```ts
 * import { overwolf } from 'ow-tauri/main';
 *
 * overwolf.disableAnonymousAnalytics(); // before app.whenReady() resolves
 * console.log(overwolf.uid, overwolf.muid, overwolf.phasePercent);
 * const hashes = overwolf.generateUserEmailHashes('user@example.com'); // synchronous
 * if (await overwolf.isCMPRequired()) await overwolf.openAdPrivacySettingsWindow({ tab: 'purposes' });
 * overwolf.packages.on('ready', (_event, name, version) => console.log(name, version));
 * ```
 */
export const overwolf: Overwolf = overwolfOf(kernel);

/**
 * Scoped file access replacing Node `fs` in main-process code (B.1.7).
 *
 * @example
 * ```ts
 * import { files } from 'ow-tauri/main';
 * import { app } from 'electron';
 *
 * const path = `${app.getPath('userData')}/settings.json`;
 * const text = (await files.readText(path)) ?? '{}';
 * await files.writeText(path, JSON.stringify({ ...JSON.parse(text), seen: true }));
 * ```
 */
export const files: Files = kernel.singleton('main.files', () => createFiles(kernel));

/**
 * Resolves once `ipc_main_ready` and `main_ready` were sent to the host.
 *
 * @returns the readiness promise
 *
 * @example
 * ```ts
 * import { whenHostReady } from 'ow-tauri/main';
 *
 * await whenHostReady(); // the same moment as app.whenReady()
 * ```
 */
export function whenHostReady(): Promise<void> {
  kernel.require('main', 'whenHostReady');
  return kernel.whenHostReady();
}

export { RecorderError } from './recorder-error.js';
export { NOT_READY_MESSAGE, Overwolf, PAYMENT_ID_MANDATORY, createSettings } from './overwolf.js';
export { PackageManager, syntheticEvent } from './packages.js';
export type { SyntheticEvent } from './packages.js';
export { emailHashes, normalizeEmail } from './hashes.js';
export type { UserEmailHashes } from './hashes.js';
export type {
  AdsOptimizationSettings,
  AvailableChannelsResult,
  ChannelPackageInfo,
  CMPParentWindow,
  CMPWindowOptions,
  CurrentChannelsResult,
  EmailHashes,
  ExternalPaymentProvider,
  ExternalPaymentUserIdOptions,
  OverwolfApi,
  OverwolfPackageManager,
  OverwolfSettings,
  PackageInfo,
  PackageName,
  PendingUpdatesResult,
  SetChannelResult,
} from './overwolf-types.js';
export type { Files, MkdirOptions } from './files.js';
export { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
export type { OwTauriErrorCode, OwTauriErrorOptions } from '../shared/errors.js';
