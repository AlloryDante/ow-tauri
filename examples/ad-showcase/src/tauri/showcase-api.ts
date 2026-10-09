/**
 * `window.showcase` on ow-tauri: the same {@link ShowcaseApi} the ow-electron
 * preload exposes, built from the plugin's JavaScript API
 * (`tauri-plugin-overwolf-api`) and the app's own commands
 * (`src-tauri/src/showcase.rs`). The pieces come in as {@link ShowcaseDeps},
 * so the unit tests run it without Tauri.
 *
 * @packageDocumentation
 */
import { exportFileName } from '../shared/identity.js';
import type {
  AdMode,
  ExportRequest,
  ExportResult,
  HostInfo,
  ParityLookup,
  ParityReport,
  ShowcaseApi,
  WindowAction,
  WindowEvent,
} from '../shared/ipc.js';
import { homeRelative, redactHome } from '../shared/paths.js';

/** The app event that carries window state changes (`WINDOW_EVENT` in `showcase.rs`). */
export const WINDOW_EVENT = 'showcase://window-event';

/** What {@link createShowcaseApi} is built from. */
export interface ShowcaseDeps {
  /** `invoke` of `@tauri-apps/api/core`, for the app's commands. */
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  /**
   * `listen` of `@tauri-apps/api/event` for the window's state changes.
   *
   * @returns a promise of the unsubscribe function
   */
  listen(event: string, handler: (event: { payload: WindowEvent }) => void): Promise<() => void>;
  /** The plugin's `isCMPRequired`. */
  isCMPRequired(): Promise<boolean>;
  /** The plugin's `openAdPrivacySettingsWindow`. */
  openAdPrivacySettingsWindow(): Promise<void>;
  /** The plugin's `generateUserEmailHashes`. */
  generateUserEmailHashes(email: string): Promise<Record<string, string | undefined>>;
  /** The clock (exports are named after the time). */
  now?(): Date;
}

/** `showcase_paths`. */
interface Paths {
  home: string;
}

/** `showcase_read_parity`. */
interface ParityText {
  path: string;
  text: string | null;
}

/**
 * Builds the API.
 *
 * @param deps - the plugin API and Tauri's `invoke` and `listen`
 * @returns the API the showcase page uses as `window.showcase`
 */
export function createShowcaseApi(deps: ShowcaseDeps): ShowcaseApi {
  let home: Promise<string> | null = null;
  /** The home folder, asked once (`''` when the app cannot tell). */
  const homeDir = (): Promise<string> => {
    home ??= deps.invoke<Paths>('showcase_paths').then(
      (p) => p.home,
      () => '',
    );
    return home;
  };
  let info: Promise<HostInfo> | null = null;
  const hostInfo = (): Promise<HostInfo> => {
    info ??= deps.invoke<HostInfo>('showcase_info');
    return info;
  };

  return {
    info: hostInfo,
    cmpRequired: () => deps.isCMPRequired(),
    openPrivacySettings: () => deps.openAdPrivacySettingsWindow(),
    async emailHashes(email: string) {
      if (typeof email !== 'string') throw new TypeError('email must be a string');
      const hashes = await deps.generateUserEmailHashes(email);
      const out: Record<string, string> = {};
      for (const [name, value] of Object.entries(hashes)) {
        if (typeof value === 'string') out[name] = value;
      }
      return out;
    },
    async exportTimeline(request: ExportRequest): Promise<ExportResult> {
      if (typeof request.json !== 'string') throw new TypeError('json must be a string');
      const { mode } = await hostInfo();
      const name = exportFileName('ow-tauri', mode, deps.now ? deps.now() : new Date());
      // Event payloads can carry local URLs: the file never names the home folder.
      const json = redactHome(request.json, await homeDir());
      const path = await deps.invoke<string>('showcase_write_export', { name, json });
      return { path };
    },
    async parity(): Promise<ParityLookup> {
      const h = await homeDir();
      let read: ParityText;
      try {
        read = await deps.invoke<ParityText>('showcase_read_parity');
      } catch (error) {
        return { path: '', report: null, error: redactHome(String(error), h) };
      }
      const path = homeRelative(read.path, h);
      if (read.text === null) return { path, report: null };
      try {
        // The report quotes capture paths and page URLs: shown without the home folder.
        return { path, report: JSON.parse(redactHome(read.text, h)) as ParityReport };
      } catch (error) {
        return { path, report: null, error: redactHome(String(error), h) };
      }
    },
    restart: (mode: AdMode, route?: string) =>
      deps.invoke<null>('showcase_restart', { mode, route: route ?? null }).then(() => undefined),
    windowAction: (action: WindowAction) =>
      deps.invoke<null>('showcase_window_action', { action }).then(() => undefined),
    onWindowEvent(listener: (event: WindowEvent) => void) {
      let stopped = false;
      let stop: (() => void) | null = null;
      void deps
        .listen(WINDOW_EVENT, (event) => {
          if (!stopped) listener(event.payload);
        })
        .then(
          (unlisten) => {
            if (stopped) unlisten();
            else stop = unlisten;
          },
          () => undefined,
        );
      return () => {
        stopped = true;
        stop?.();
      };
    },
  };
}
