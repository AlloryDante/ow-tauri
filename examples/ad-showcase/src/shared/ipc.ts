/**
 * The contract between the showcase page and its host. On ow-electron the
 * preload exposes it over Electron IPC (the `showcase:*` channels); on Tauri
 * `src/tauri/showcase-api.ts` builds it from tauri-plugin-overwolf-api and
 * the app's commands. The page only sees {@link ShowcaseApi}, so nothing in
 * it knows which host runs it.
 *
 * @packageDocumentation
 */

/** Channel names, `showcase:` prefixed. */
export const Channel = {
  /** invoke: `() => HostInfo` */
  info: 'showcase:info',
  /** invoke: `() => boolean` (`app.overwolf.isCMPRequired()`) */
  cmpRequired: 'showcase:cmp-required',
  /** invoke: `() => void` (`app.overwolf.openAdPrivacySettingsWindow()`) */
  openPrivacy: 'showcase:open-privacy',
  /** invoke: `(email: string) => Record<string, string>` */
  emailHashes: 'showcase:email-hashes',
  /** invoke: `(request: ExportRequest) => ExportResult` */
  exportTimeline: 'showcase:export-timeline',
  /** invoke: `() => ParityLookup` */
  parity: 'showcase:parity',
  /** invoke: `(mode: AdMode, route?: string) => void`; relaunches the app on `route` */
  restart: 'showcase:restart',
  /** invoke: `(action: WindowAction) => void` */
  windowAction: 'showcase:window-action',
  /** main to window: `(event: WindowEvent)` */
  windowEvent: 'showcase:window-event',
} as const;

/** Test ads (`--test-ad`) or live ads. */
export type AdMode = 'test' | 'live';

/** Which host runs the app. */
export type HostName = 'ow-electron' | 'ow-tauri';

/** What the window shows in its top bar and on the identity page. */
export interface HostInfo {
  /** `ow-electron` or `ow-tauri`. */
  host: HostName;
  /** The host's version (ow-electron's Electron version, or the ow-tauri plugin version). */
  hostVersion: string;
  /** The engine line: `Electron 42.11.4` or `Tauri 2.12.1`. */
  engine: string;
  /** `process.platform` style: `darwin`, `win32`, `linux`. */
  platform: string;
  /** Ad mode, from the `--test-ad` switch. */
  mode: AdMode;
  /** `app.overwolf.uid` (the window masks it). */
  uid: string;
  /** The formula uid from author and product name (`app_cuid`). */
  cuid: string;
  /** `app.overwolf.muid` (the window masks it). */
  muid: string;
  /** `app.overwolf.phasePercent`. */
  phasePercent: number;
  /** `app.getName()`. */
  productName: string;
  /** `app.getVersion()`. */
  appVersion: string;
  /** The folder exports go to: `<userData>/exports`, home folder as `~`. */
  exportsDir: string;
}

/** A timeline export the window asks the main process to write. */
export interface ExportRequest {
  /** The JSON text to write. */
  json: string;
}

/** Where an export was written. */
export interface ExportResult {
  /** The written file, with the home folder as `~` (`~/.../exports/<file>.json`). */
  path: string;
}

/** One difference in a parity report (`tools/parity-harness` `parity-diff.json`). */
export interface ParityDiff {
  /** Verdict: `BUG`, `variance`, `intended:*`, `not-mirrored`. */
  class: string;
  /** Report section, for example `host-request`. */
  section: string;
  /** What differs. */
  key?: unknown;
  /** Which field differs. */
  field?: unknown;
  /** The ow-electron value. */
  electron?: unknown;
  /** The ow-tauri value. */
  tauri?: unknown;
  /** Why the verdict was given. */
  why?: string;
}

/** The parity report, if one was placed where the app looks for it. */
export interface ParityLookup {
  /** The path the main process read (or would read), with the home folder as `~`. */
  path: string;
  /** The parsed report, or `null` when the file does not exist. */
  report: ParityReport | null;
  /** A read or parse error, if any. */
  error?: string;
}

/** The fields of `parity-diff.json` the parity page renders. */
export interface ParityReport {
  /** Run ids of the two captures. */
  electron?: { runId?: string };
  /** Run id of the ow-tauri capture. */
  tauri?: { runId?: string; everVisible?: boolean | null };
  /** Differences per verdict. */
  counts?: Record<string, number>;
  /** Number of BUG differences. */
  bugs?: number;
  /** Every difference. */
  diffs?: ParityDiff[];
}

/** Window actions for the controls and interstitial pages. */
export type WindowAction = 'hide-3s' | 'minimize-3s' | 'shrink-900x500' | 'restore-size';

/** A window state change the main process reports to the window. */
export interface WindowEvent {
  /** What happened: `hide`, `show`, `minimize`, `restore`, `resize`. */
  name: string;
  /** Extra detail (bounds after a resize). */
  detail?: Record<string, unknown>;
}

/** The API the page uses as `window.showcase` (ow-electron preload, or `src/tauri`). */
export interface ShowcaseApi {
  /** Host, mode and identity. */
  info(): Promise<HostInfo>;
  /** `app.overwolf.isCMPRequired()`. */
  cmpRequired(): Promise<boolean>;
  /** `app.overwolf.openAdPrivacySettingsWindow()`. */
  openPrivacySettings(): Promise<void>;
  /** `app.overwolf.generateUserEmailHashes(email)`. */
  emailHashes(email: string): Promise<Record<string, string>>;
  /** Writes a timeline export into `<userData>/exports`. */
  exportTimeline(request: ExportRequest): Promise<ExportResult>;
  /** Reads `<userData>/parity-report.json`. */
  parity(): Promise<ParityLookup>;
  /**
   * Relaunches the app in the given ad mode, on `route` (`page` or
   * `page/arg`, see `route.ts`) when given.
   */
  restart(mode: AdMode, route?: string): Promise<void>;
  /** Runs a window action. */
  windowAction(action: WindowAction): Promise<void>;
  /** Subscribes to window state changes; returns an unsubscribe function. */
  onWindowEvent(listener: (event: WindowEvent) => void): () => void;
}
