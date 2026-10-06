// The global `overwolf` namespace and the `<owadview>` overload (CONTRACT
// B.4), re-declared from the ow-electron 42.11.4 typings (MIT, Overwolf) with
// `BrowserWindow` and `Event` pointing at the ow-tauri facade. Also serves as
// the empty `@overwolf/ow-electron` module: point
// `paths['@overwolf/ow-electron']` at this file.

/// <reference types="node" />
import type { App, BrowserWindow, Event } from '../electron/index.js';
import type { errorMonitor } from 'events';

export {};

declare global {
  namespace overwolf {
    /**
     * Electron's `app` with `overwolf`, as ow-electron declares it
     * (`interface OverwolfApp extends App`), so `app as overwolf.OverwolfApp`
     * keeps `getPath`, `name` and the other `app` members.
     *
     * @deprecated Prefer `app` from `electron`: `app.overwolf` is typed
     * directly. Kept for backwards compatibility, as in ow-electron.
     */
    interface OverwolfApp extends Omit<App, 'overwolf'> {
      /**
       * Overwolf additional api's
       */
      overwolf: OverwolfApi;
    }

    /**
     * The Overwolf APIs, exposed on Electron's `app` object as `app.overwolf`.
     */
    interface OverwolfApi {
      /**
       * Disable sending any anonymous analytics,
       * this should be called before app.ready
       */
      disableAnonymousAnalytics(): void;

      /**
       * Disable Ads optimization
       */
      disableAdsOptimization(): void;

      /**
       * Opt out from using first party data (email address) for ad targeting
       */
      disableAdsFPD(): void;

      /**
       * Returns true if the current user should be able to update their cmp
       * configurations (i.e. openCMPWindow).
       *
       * Note that this is an async function and should be called with await.
       * The function will never throw an exception - the default value is true.
       */
      isCMPRequired(): Promise<boolean>;

      /**
       * Opens the CMP configuration window - should only be called when
       * isCMPRequired returns true.
       */
      openCMPWindow(options?: CMPWindowOptions): Promise<void>;

      /**
       * Opens the Ads settings configuration window.
       */
      openAdPrivacySettingsWindow(options?: CMPWindowOptions): Promise<void>;

      /**
       * The Overwolf Package Manager instance
       */
      packages: overwolf.packages.OverwolfPackageManager;

      /**
       * Generate a hashed email, to allow for better ad performance,
       * this should be called after app.ready
       * NOTE: the email is not stored! only the hashed email.
       */
      generateUserEmailHashes(email: string): EmailHashes;

      /**
       * Set the user email hashes (see generateUserEmailHashes),
       * this should be called after app.ready
       * See https://unifiedid.com/docs/getting-started/gs-normalization-encoding#email-address-normalization for more
       * details how to normalize email before creating hash
       */
      setUserEmailHashes(emailHashes?: EmailHashes): void;

      /**
       * Associates the current user's id at an external payment provider
       * (e.g. Tebex) with this machine.
       * Call this once on every app launch, after app.ready.
       * Rejects if userId is missing. providerName defaults to 'tebex'.
       * Rejects with 'ow-electron is not ready yet!' if called before app.ready.
       * A failed analytics report does not reject.
       */
      setExternalPaymentUserId(options: ExternalPaymentUserIdOptions): Promise<void>;

      /**
       * Client persistence phasing percent
       */
      readonly phasePercent: number;

      /**
       * Overwolf installer provided UTM params
       */
      readonly utmParams: any;

      /**
       * A unique identifier for the user machine.
       */
      readonly muid: string;

      /**
       * The ow-electron uid (Overwolf App Id).
       */
      readonly uid: string;
    }

    interface CMPWindowOptions {
      /**
       * Select open tab. The default is 'purposes'
       */
      tab?: 'purposes' | 'features' | 'vendors';

      /**
       * Whether this is a modal window. This only works when the window is a child
       * window. Default is `false`.
       */
      modal?: boolean;

      /**
       * Specify parent window. Default is `null`.
       */
      parent?: BrowserWindow;

      /**
       * Show window in the center of the screen. Default is `true`.
       */
      center?: boolean;

      /**
       * Control the CMP preloader background window
       */
      backgroundColor?: string;

      /**
       * Control the CMP preloader color (spinner)
       */
      preLoaderSpinnerColor?: string;

      /**
       * Control the CMP Window width
       */
      width?: number;

      /**
       * Control the CMP Window height
       */
      height?: number;

      /**
       * Control the CMP Window left pos
       */
      x?: number;

      /**
       * Control the CMP Window top pos
       */
      y?: number;

      /**
       * Overrides the path of the cmp html
       */
      cmpURL?: string;

      /**
       * Cmp html language
       */
      language?: string;
    }

    interface EmailHashes {
      readonly sha1?: string;
      readonly sha256?: string;
      readonly md5?: string;
    }

    /**
     * A enum of the external payment providers we know about.
     */
    type ExternalPaymentProvider = 'tebex' | string;

    interface ExternalPaymentUserIdOptions {
      /**
       * The name of the external payment provider.
       */
      providerName: ExternalPaymentProvider;

      /**
       * The user id your app passes to the external payment provider.
       */
      userId: string;

      /**
       * Optional. The provider's recurring payment (subscription) agreement id.
       */
      paymentId?: string;
    }

    /**
     * A utility type for registering to `error` events, that includes the `errorMonitor` type
     */
    type error = 'error' | typeof errorMonitor;

    /**
     * Namespace containing everything related to Overwolf Packages
     */
    namespace packages {
      /**
       * A fake enum for all built-in package names
       */
      type PackageName = 'gep' | 'overlay' | 'recorder' | 'utility' | 'crn' | string;

      /**
       * Package info
       */
      interface PackageInfo {
        name: string;
        version: string;
      }

      type PendingUpdatesResult = {
        hasPendingUpdate: boolean;
        details: PackageInfo[];
      };

      /**
       * Result returned by `setChannel()`.
       * `error` is `'invalid-package'` when the package is not found on the server,
       * or `'invalid-channel'` when the channel does not exist for that package.
       */
      type SetChannelResult = {
        success: boolean;
        error?: 'invalid-package' | 'invalid-channel';
      };

      /**
       * Passed to the `ready` callback of `setChannel()`.
       * Identifies the downloaded package that is pending a restart.
       */
      interface ChannelPackageInfo {
        name: string;
        version: string;
      }

      /**
       * Returned by `getAvailableChannels()`.
       * Maps each package name to the list of channel names available on the server.
       * Packages with no channels defined return an empty array.
       */
      type AvailableChannelsResult = Record<string, string[]>;

      /**
       * Returned by `getChannel()`.
       * Maps each package name to its currently active channel.
       * `'public'` means the package is on the default public release.
       */
      type CurrentChannelsResult = Record<string, string>;

      /**
       * Overwolf Package Manager interface.
       *
       * For package-specific API types, see `@overwolf/ow-electron-packages-types`.
       */
      interface OverwolfPackageManager extends NodeJS.EventEmitter {
        /**
         * Register listener for Overwolf Package crashes.
         * Calling `event.preventDefault()` will prevent the package from automatically attempting to re-launch itself.
         *
         * @param {string | symbol} eventName Name of the node event ('crashed')
         * @param {(Event, any[]) => void} listener The listener that will be invoked when this event is fired
         * @returns {this} The current instance of the Overwolf Package Manager
         */
        on(eventName: 'crashed', listener: (event: Event, canRecover: boolean) => void): this;

        /**
         * Register listener for when an Overwolf Package is ready
         *
         * @param {string | symbol} eventName Name of the node event ('ready')
         * @param {(Event, any[]) => void} listener The listener that will be invoked when this event is fired
         * @returns {this} The current instance of the Overwolf Package Manager
         */
        on(
          eventName: 'ready',
          listener: (event: Event, packageName: PackageName, version: string) => void,
        ): this;

        /**
         * Register listener for when an Overwolf Package is ready to update
         *
         * @param {string | symbol} eventName Name of the node event ('package-update-pending')
         * @param {(Event, PackageInfo[]) => void} listener The listener that will be invoked when this event is fired
         * @returns {this} The current instance of the Overwolf Package Manager
         */
        on(
          eventName: 'package-update-pending',
          listener: (event: Event, info: PackageInfo[]) => void,
        ): this;

        /**
         * Register listener for when an Overwolf Package updated
         *
         * @param {string | symbol} eventName Name of the node event ('updated')
         * @returns {this} The current instance of the Overwolf Package Manager
         */
        on(
          eventName: 'updated',
          listener: (event: Event, packageName: string, version: string) => void,
        ): this;

        /**
         * Register listener for Overwolf Package initialization failures
         *
         * @param {string | symbol} eventName Name of the node event ('failed-to-initialize')
         * @param {(Event, any[]) => void} listener **The listener that will be invoked when this event is fired**
         * @returns {this} The current instance of the Overwolf Package Manager
         */
        on(
          eventName: 'failed-to-initialize',
          listener: (event: Event, packageName: PackageName) => void,
        ): this;

        /**
         * Register listener for when an Overwolf Package begins its load sequence.
         * Fires before 'ready'.
         *
         * @param {string} eventName Name of the node event ('loading')
         * @param {(Event, PackageName) => void} listener The listener that will be invoked when this event is fired
         * @returns {this} The current instance of the Overwolf Package Manager
         */
        on(eventName: 'loading', listener: (event: Event, packageName: PackageName) => void): this;

        /**
         * Relaunch the Overwolf Package Manager. Call it to force all pending Overwolf Package updates.
         *
         * The Overwolf Package Manager will automatically relaunch itself if an update is available and no package is currently running.*
         */
        relaunch(): void;

        /**
         * Checks if there are any pending package updates that require a client restart.
         *
         * @returns {PendingUpdatesResult} - Result indicating the status of pending updates.
         */
        hasPendingUpdates(): PendingUpdatesResult;

        /**
         * Switches a package to a named release channel and immediately triggers a
         * download of that channel's version.
         *
         * - Channel preferences are persisted in storage and applied on every subsequent
         *   update check, including the next app launch.
         * - Pass `undefined`, `null`, an empty string, or `'public'` to restore the
         *   default public release.
         * - The optional `ready` callback is invoked with `{ name, version }` once the
         *   download completes and the app must restart to apply the new version.
         * - If the package is already at the requested channel version, `ready` never fires.
         * - Throws if `packageName` is not listed in the app's `package.json`
         *   `overwolf.packages` array.
         *
         * @param {PackageName} packageName  The package to switch.
         * @param {string}      [channel]    Target channel name. Omit or pass `'public'` /
         *                                   empty string to restore the public release.
         * @param {Function}    [ready]      Invoked when the download completes and a
         *                                   restart is required.
         * @returns {Promise<SetChannelResult>}
         *
         * @example
         * ```typescript
         * const result = await api.setChannel('overlay', 'pre-release', (pkg) => {
         *   console.log(`overlay v${pkg.version} ready - restart required`);
         *   api.relaunch();
         * });
         * if (!result.success) console.error('setChannel failed:', result.error);
         * ```
         *
         * @example
         * ```typescript
         * // Restore the public release - all four are equivalent
         * await api.setChannel('overlay');
         * await api.setChannel('overlay', undefined);
         * await api.setChannel('overlay', '');
         * await api.setChannel('overlay', 'public');
         * ```
         */
        setChannel(
          packageName: PackageName,
          channel?: string | 'public',
          ready?: (packageInfo: ChannelPackageInfo) => void,
        ): Promise<SetChannelResult>;

        /**
         * Returns the list of release channels available on the server for one or more packages.
         *
         * - Pass no arguments to query all packages registered in the app's `overwolf.packages` list.
         * - Pass one or more package names to query a specific subset.
         * - Packages with no channels defined return an empty array `[]`.
         * - Throws if any supplied name is not in the registered packages list.
         *
         * @param {...PackageName} packageNames  Optional package names to query.
         * @returns {Promise<AvailableChannelsResult>}
         *
         * @example
         * ```typescript
         * const channels = await api.getAvailableChannels();
         * // { overlay: ['pre-release', 'beta'], gep: ['pre-release'], utility: [] }
         *
         * const { overlay } = await api.getAvailableChannels('overlay');
         * // overlay: ['pre-release', 'beta']
         * ```
         */
        getAvailableChannels(...packageNames: PackageName[]): Promise<AvailableChannelsResult>;

        /**
         * Returns the currently active release channel for one or more packages.
         *
         * - `'public'` means the package is on the default public release.
         * - Pass no arguments to query all registered packages (plus any package with a
         *   non-public channel stored, even if not in `package.json`).
         * - Unknown package names are silently omitted from the result (no error thrown).
         *
         * @param {...PackageName} packageNames  Optional package names to query.
         * @returns {Promise<CurrentChannelsResult>}
         *
         * @example
         * ```typescript
         * const current = await api.getChannel();
         * // { overlay: 'pre-release', gep: 'public', utility: 'public' }
         *
         * const { overlay } = await api.getChannel('overlay');
         * // 'pre-release'
         * ```
         */
        getChannel(...packageNames: PackageName[]): Promise<CurrentChannelsResult>;

        /**
         * The path to the application's logs folder.
         */
        readonly logsFolderPath: string;

        /**
         * The ow-electron phase percentage (used by the package manager).
         */
        readonly phasePercent: number;
      }
    }

    /**
     * The `<owadview>` HTML element, used to display Overwolf ads.
     */
    interface AdviewTag extends HTMLElement {
      /**
       * A JSON string of custom tracking key-value pairs to report with the ad.
       *
       * Can be set declaratively:
       * ```html
       * <owadview customTracking='{"page":"main_menu"}'></owadview>
       * ```
       * or changed at any time afterwards, in which case the new value is
       * pushed to the running ad view.
       */
      customTracking: string;
    }

    namespace Renderer {
      type AdviewTag = overwolf.AdviewTag;
    }
  }

  interface Document {
    createElement(tagName: 'owadview'): overwolf.AdviewTag;
  }
}
