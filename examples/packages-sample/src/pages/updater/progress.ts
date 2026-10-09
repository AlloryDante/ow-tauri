/**
 * The updater page's state: check, the result, and the download progress
 * from the `Started` / `Progress` / `Finished` events of
 * `Update.downloadAndInstall()`.
 *
 * @packageDocumentation
 */
import type { DownloadEvent } from 'tauri-plugin-overwolf-api/updater';

/** What `check()` found. */
export interface FoundUpdate {
  /** The version on the feed. */
  version: string;
  /** The running version. */
  currentVersion: string;
  /** The release date, if the feed has one. */
  date?: string;
  /** The release notes, if the feed has them. */
  body?: string;
}

/** The updater page's state. */
export type UpdaterState =
  | { phase: 'idle' }
  | { phase: 'checking' }
  | { phase: 'up-to-date' }
  | { phase: 'available'; update: FoundUpdate }
  | {
      phase: 'downloading';
      update: FoundUpdate;
      /** Bytes received so far. */
      downloaded: number;
      /** The installer size, when the server sent it. */
      total?: number;
    }
  | { phase: 'installing'; update: FoundUpdate; downloaded: number }
  | { phase: 'error'; code: string; message: string };

/** What changes the state. */
export type UpdaterAction =
  | { type: 'check' }
  | { type: 'checked'; update: FoundUpdate | null }
  | { type: 'download' }
  | { type: 'event'; event: DownloadEvent }
  | { type: 'failed'; code: string; message: string };

/** The state before the first check. */
export const INITIAL: UpdaterState = { phase: 'idle' };

/**
 * The next state.
 *
 * @param state - the current state
 * @param action - what happened
 * @returns the next state (the same object when the action does not apply)
 */
export function updaterReducer(state: UpdaterState, action: UpdaterAction): UpdaterState {
  switch (action.type) {
    case 'check':
      return state.phase === 'downloading' || state.phase === 'installing'
        ? state
        : { phase: 'checking' };
    case 'checked':
      return action.update
        ? { phase: 'available', update: action.update }
        : { phase: 'up-to-date' };
    case 'download':
      return state.phase === 'available'
        ? { phase: 'downloading', update: state.update, downloaded: 0 }
        : state;
    case 'event': {
      if (state.phase !== 'downloading') return state;
      const { event } = action;
      if (event.event === 'Started') {
        const total = event.data.contentLength;
        return total === undefined
          ? { phase: 'downloading', update: state.update, downloaded: 0 }
          : { phase: 'downloading', update: state.update, downloaded: 0, total };
      }
      if (event.event === 'Progress')
        return { ...state, downloaded: state.downloaded + event.data.chunkLength };
      return { phase: 'installing', update: state.update, downloaded: state.downloaded };
    }
    case 'failed':
      return { phase: 'error', code: action.code, message: action.message };
  }
}

/**
 * The download progress in whole percent, or `null` when the size is not
 * known (or nothing is downloading).
 *
 * @param state - the state
 * @returns 0 to 100, or `null`
 */
export function percent(state: UpdaterState): number | null {
  if (state.phase === 'installing') return 100;
  if (state.phase !== 'downloading' || !state.total) return null;
  return Math.min(100, Math.floor((state.downloaded / state.total) * 100));
}

/**
 * A plain explanation of an updater error code.
 *
 * @param code - the `OverwolfError` code
 * @returns one sentence, or `null` when the message says enough
 */
export function explain(code: string): string | null {
  switch (code) {
    case 'unsupported':
      return "The Overwolf updater runs on Windows only (the plugin's `updater` feature, which this sample turns on for Windows). On macOS and Linux use @tauri-apps/plugin-updater.";
    case 'forbidden':
      return 'The capability does not grant overwolf:updater.';
    case 'network':
      return "The update feed could not be reached (Overwolf's feed serves an app once the console has published a version of it).";
    case 'verification':
      return 'The installer failed its hash or publisher check, so it was not started.';
    default:
      return null;
  }
}
