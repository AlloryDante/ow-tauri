/**
 * Electron's `app` (`docs/CONTRACT.md` section B.2.1), main webview only.
 *
 * `app.overwolf` is added by `ow-tauri/main` (section B.1.1) and is not part
 * of this module.
 *
 * @packageDocumentation
 */
import type { Kernel } from '../bootstrap/kernel.js';
import { EventEmitter, emitFromHost } from '../shared/emitter.js';
import { OwTauriUnsupportedError } from '../shared/errors.js';
import type { ElectronPathName, LifecycleMessage } from '../shared/protocol.js';
import { defineUnsupported } from '../shared/unsupported.js';
import { windowHooks } from './browser-window.js';
import { createEvent, kernel } from './runtime.js';

/** Names `app.getPath()` accepts (CONTRACT B.2.1). */
const PATH_NAMES: ReadonlySet<string> = new Set<ElectronPathName>([
  'appData',
  'userData',
  'sessionData',
  'temp',
  'home',
  'desktop',
  'documents',
  'downloads',
  'music',
  'pictures',
  'videos',
  'logs',
  'exe',
  'crashDumps',
]);

/** Switches that take effect from the next launch (CONTRACT A.1.1). */
const NEXT_LAUNCH_SWITCHES: ReadonlySet<string> = new Set(['disable-gpu', 'remote-debugging-port']);

/** Electron's `app.commandLine` (CONTRACT B.2.1). */
export interface CommandLine {
  /**
   * Whether the process arguments (or an `appendSwitch` call) contain `--<name>`.
   *
   * @param name - the switch name without dashes
   * @returns `true` when present
   */
  hasSwitch(name: string): boolean;
  /**
   * The value of `--<name>=<value>` (or `--<name> <value>`), else `''`.
   *
   * @param name - the switch name without dashes
   * @returns the value
   */
  getSwitchValue(name: string): string;
  /**
   * Partial: recorded so `hasSwitch` sees it; browser switches cannot change
   * webviews that already exist (CONTRACT A.1.1).
   *
   * @param name - the switch name
   * @param value - its value
   */
  appendSwitch(name: string, value?: string): void;
  /**
   * Partial: recorded only.
   *
   * @param value - the argument
   */
  appendArgument(value: string): void;
  /**
   * Removes a switch recorded with `appendSwitch`.
   *
   * @param name - the switch name
   */
  removeSwitch(name: string): void;
}

/** Electron's `app.relaunch` options. */
export interface RelaunchOptions {
  /** Arguments for the new process. */
  args?: string[];
  /** Unsupported: must be absent. */
  execPath?: string;
}

/**
 * Electron's `App` (CONTRACT B.2.1). Events: `ready`, `window-all-closed`,
 * `before-quit`, `will-quit`, `quit`, `activate`, `second-instance`,
 * `browser-window-created`, `browser-window-focus`, `browser-window-blur`.
 */
export class App extends EventEmitter {
  readonly #kernel: Kernel;
  readonly #appended: string[] = [];
  readonly #paths = new Map<string, string>();
  #name: string | undefined;
  #generation = 0;

  /**
   * @param k - the kernel
   * @internal
   */
  constructor(k: Kernel) {
    super();
    this.#kernel = k;
    k.on('lifecycle', (message) => {
      this.#onLifecycle(message as LifecycleMessage);
    });
    const hooks = windowHooks();
    hooks.created = (win) => emitFromHost(this, 'browser-window-created', createEvent(), win);
    hooks.focus = (win) => emitFromHost(this, 'browser-window-focus', createEvent(), win);
    hooks.blur = (win) => emitFromHost(this, 'browser-window-blur', createEvent(), win);
    hooks.allClosed = () => {
      if (this.listenerCount('window-all-closed') > 0) emitFromHost(this, 'window-all-closed');
      else this.quit();
    };
    k.onReset(() => {
      this.#appended.length = 0;
      this.#paths.clear();
      this.#name = undefined;
      this.removeAllListeners();
      this.#armReady();
    });
    this.#armReady();
  }

  /**
   * Resolves once the app is ready (`main_ready` acknowledged).
   *
   * @returns the readiness promise
   */
  whenReady(): Promise<void> {
    this.#require('app.whenReady');
    return this.#kernel.whenHostReady();
  }

  /**
   * Whether the app is ready.
   *
   * @returns `true` after `ready`
   */
  isReady(): boolean {
    return this.#kernel.isReady;
  }

  /** Quits gracefully: `before-quit`, window `close` events, `will-quit`, `quit` (A.6). */
  quit(): void {
    this.#require('app.quit');
    this.#fire('app_quit');
  }

  /**
   * Exits immediately (skips `before-quit` and `will-quit`).
   *
   * @param exitCode - the exit code (default 0)
   */
  exit(exitCode = 0): void {
    this.#require('app.exit');
    this.#fire('app_exit', { code: exitCode });
  }

  /**
   * Relaunches the app on the next `quit()` / `exit()`.
   *
   * @param options - `args`; `execPath` is unsupported
   */
  relaunch(options: RelaunchOptions = {}): void {
    this.#require('app.relaunch');
    if (options.execPath !== undefined) {
      throw new OwTauriUnsupportedError(
        'app.relaunch({ execPath })',
        'a Tauri app can only relaunch itself',
      );
    }
    this.#fire('app_relaunch', options.args === undefined ? {} : { args: options.args });
  }

  /**
   * Focuses the most recent visible window.
   *
   * @param options - `{ steal }`
   */
  focus(options: { steal?: boolean } = {}): void {
    this.#require('app.focus');
    this.#fire('app_focus', options.steal === undefined ? {} : { steal: options.steal });
  }

  /**
   * The virtual app root; `getAppPath() + '/package.json'` is readable
   * through `files` of `ow-tauri/main`.
   *
   * @returns the app root
   */
  getAppPath(): string {
    this.#require('app.getAppPath');
    return this.#string('paths.appPath') ?? '/';
  }

  /**
   * A well-known path (CONTRACT B.2.1). `module` and `recent` are unsupported.
   *
   * @param name - the path name
   * @returns the path
   * @throws Error for unknown names, as Electron
   */
  getPath(name: string): string {
    this.#require('app.getPath');
    if (name === 'module' || name === 'recent') {
      throw new OwTauriUnsupportedError(
        `app.getPath('${name}')`,
        'there is no such path in a Tauri app',
      );
    }
    const override = this.#paths.get(name);
    if (override !== undefined) return override;
    const value = PATH_NAMES.has(name) ? this.#string(`paths.${name}`) : undefined;
    if (value === undefined) throw new Error(`Failed to get '${name}' path`);
    return value;
  }

  /**
   * Partial: overrides a path for ow-tauri lookups in this session.
   *
   * @param name - the path name
   * @param path - the new path
   */
  setPath(name: string, path: string): void {
    this.#require('app.setPath');
    if (!PATH_NAMES.has(name)) throw new Error(`Failed to set path '${name}'`);
    this.#paths.set(name, path);
  }

  /**
   * The app name: `productName`, else `name` (CONTRACT G.1).
   *
   * @returns the name
   */
  getName(): string {
    this.#require('app.getName');
    return (
      this.#name ?? this.#string('manifest.productName') ?? this.#string('manifest.name') ?? ''
    );
  }

  /**
   * Partial: changes the name for this session only; never changes the uid.
   *
   * @param name - the name
   */
  setName(name: string): void {
    this.#require('app.setName');
    this.#name = name;
  }

  /** The app name (see {@link App.getName} / {@link App.setName}). */
  get name(): string {
    return this.getName();
  }

  /**
   * Partial: renames the app for this session (see {@link App.setName}).
   *
   * @param name - the new name
   */
  set name(name: string) {
    this.setName(name);
  }

  /**
   * The manifest version.
   *
   * @returns the version
   */
  getVersion(): string {
    this.#require('app.getVersion');
    return this.#string('manifest.version') ?? '0.0.0';
  }

  /** Whether this is a release build. */
  get isPackaged(): boolean {
    this.#require('app.isPackaged');
    return this.#kernel.state.get('isPackaged') === true;
  }

  /**
   * The app locale.
   *
   * @returns e.g. `en-US`
   */
  getLocale(): string {
    this.#require('app.getLocale');
    return this.#string('locale') ?? 'en-US';
  }

  /**
   * The OS locale (same source as {@link App.getLocale}).
   *
   * @returns e.g. `en-US`
   */
  getSystemLocale(): string {
    return this.getLocale();
  }

  /** The process command line (CONTRACT B.2.1). */
  readonly commandLine: CommandLine = {
    hasSwitch: (name) => this.#switch(name) !== undefined,
    getSwitchValue: (name) => this.#switch(name) ?? '',
    appendSwitch: (name, value) => {
      this.#require('app.commandLine.appendSwitch');
      this.#appended.push(value === undefined ? `--${name}` : `--${name}=${value}`);
      const why = NEXT_LAUNCH_SWITCHES.has(name)
        ? 'set plugins.overwolf.webview in tauri.conf.json instead; webviews that already exist keep their browser arguments'
        : 'it has no effect on Tauri webviews';
      this.#kernel.warnOnce(
        `appendSwitch:${name}`,
        `app.commandLine.appendSwitch('${name}') is recorded only: ${why}`,
      );
    },
    appendArgument: (value) => {
      this.#require('app.commandLine.appendArgument');
      this.#appended.push(value);
    },
    removeSwitch: (name) => {
      for (let i = this.#appended.length - 1; i >= 0; i--) {
        const entry = this.#appended[i] ?? '';
        if (entry === `--${name}` || entry.startsWith(`--${name}=`)) this.#appended.splice(i, 1);
      }
    },
  };

  /**
   * Partial: on Windows use `plugins.overwolf.webview.disableGpu`; webviews
   * that already exist cannot change (CONTRACT A.1.1).
   */
  disableHardwareAcceleration(): void {
    this.#require('app.disableHardwareAcceleration');
    this.#kernel.warnOnce(
      'disableHardwareAcceleration',
      'app.disableHardwareAcceleration() cannot change webviews that already exist; set plugins.overwolf.webview.disableGpu',
    );
  }

  /**
   * Partial: no-op; Tauri sets the AUMID from the bundle identifier.
   *
   * @param _id - ignored
   */
  setAppUserModelId(_id: string): void {
    this.#require('app.setAppUserModelId');
  }

  /**
   * Partial: always `true`; single-instance behaviour comes from
   * `tauri-plugin-single-instance` (CONTRACT A.5).
   *
   * @param _additionalData - ignored
   * @returns `true`
   */
  requestSingleInstanceLock(_additionalData?: unknown): boolean {
    this.#require('app.requestSingleInstanceLock');
    return true;
  }

  /**
   * Partial: always `true`.
   *
   * @returns `true`
   */
  hasSingleInstanceLock(): boolean {
    return true;
  }

  /** Partial: no-op. */
  releaseSingleInstanceLock(): void {
    // the lock belongs to tauri-plugin-single-instance
  }

  #armReady(): void {
    const generation = ++this.#generation;
    void this.#kernel.whenHostReady().then(() => {
      if (generation === this.#generation) emitFromHost(this, 'ready', createEvent(), {});
    });
  }

  #onLifecycle(message: LifecycleMessage): void {
    switch (message.event) {
      case 'before-quit':
      case 'will-quit': {
        const event = createEvent();
        emitFromHost(this, message.event, event);
        if (typeof message.requestId === 'number') {
          this.#fire('app_quit_reply', {
            requestId: message.requestId,
            prevent: event.defaultPrevented,
          });
        }
        return;
      }
      case 'quit':
        emitFromHost(
          this,
          'quit',
          createEvent(),
          typeof message.exitCode === 'number' ? message.exitCode : 0,
        );
        return;
      case 'second-instance':
        emitFromHost(
          this,
          'second-instance',
          createEvent(),
          Array.isArray(message['argv']) ? message['argv'] : [],
          typeof message['cwd'] === 'string' ? message['cwd'] : '',
          message['additionalData'],
        );
        return;
      case 'activate':
        emitFromHost(this, 'activate', createEvent(), message['hasVisibleWindows'] !== false);
        return;
      default:
        this.#kernel.log('debug', `unknown lifecycle event '${message.event}'`);
    }
  }

  #switch(name: string): string | undefined {
    const argv = this.#kernel.state.get('switches.argv');
    const args = [
      ...(Array.isArray(argv)
        ? (argv as unknown[]).filter((a): a is string => typeof a === 'string')
        : []),
      ...this.#appended,
    ];
    const flag = `--${name}`;
    for (let i = args.length - 1; i >= 0; i--) {
      const arg = args[i] ?? '';
      if (arg.startsWith(`${flag}=`)) return arg.slice(flag.length + 1);
      if (arg === flag) {
        const next = args[i + 1];
        return next !== undefined && !next.startsWith('-') ? next : '';
      }
    }
    return undefined;
  }

  #string(path: string): string | undefined {
    const value = this.#kernel.state.get(path);
    return typeof value === 'string' ? value : undefined;
  }

  #require(api: string): void {
    this.#kernel.require('main', api);
  }

  #fire(command: string, args?: Record<string, unknown>): void {
    this.#kernel.command(command, args).catch((error: unknown) => {
      this.#kernel.log('warn', `${command} failed: ${(error as Error).message}`);
    });
  }
}

defineUnsupported(
  App.prototype,
  'app.',
  [
    'getGPUInfo',
    'getAppMetrics',
    'setLoginItemSettings',
    'getLoginItemSettings',
    'setBadgeCount',
    'setJumpList',
    'setUserTasks',
    'showAboutPanel',
    'setAsDefaultProtocolClient',
    'importCertificate',
    'moveToApplicationsFolder',
  ],
  ['dock'],
  (key, message) => {
    kernel.warnOnce(key, message);
  },
);

/** Electron's `app` (main webview only). */
export const app: App = kernel.singleton('electron.app', () => new App(kernel));
