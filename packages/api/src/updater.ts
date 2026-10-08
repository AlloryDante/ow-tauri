/**
 * `tauri-plugin-overwolf-api/updater`: the Overwolf-hosted update feed, in
 * the shape of `@tauri-apps/plugin-updater` (`check()` returns an
 * {@link Update} or `null`).
 *
 * Windows only in 1.0: on other platforms, and without the crate's `updater`
 * feature, {@link check} rejects with `unsupported` (use
 * `@tauri-apps/plugin-updater` there). Needs the `overwolf:updater`
 * permission. The feed URL, publisher check and installer arguments come from
 * `plugins.overwolf.updater`; a page can never change them.
 *
 * @example
 * ```ts
 * import { check } from 'tauri-plugin-overwolf-api/updater';
 *
 * const update = await check();
 * if (update) {
 *   let downloaded = 0;
 *   await update.downloadAndInstall((event) => {
 *     if (event.event === 'Started') console.log(`size ${String(event.data.contentLength ?? '?')}`);
 *     if (event.event === 'Progress') downloaded += event.data.chunkLength;
 *     if (event.event === 'Finished') console.log(`downloaded ${String(downloaded)}`);
 *   });
 * }
 * ```
 *
 * @packageDocumentation
 */
import { Channel, Resource } from '@tauri-apps/api/core';

import { call } from './internal.js';

/** Options of {@link check}. */
export interface CheckOptions {
  /**
   * A feed channel other than `latest`. It never downgrades unless
   * `plugins.overwolf.updater.allowJsDowngrade` is `true`.
   */
  channel?: string;
  /** Accept an older version (only with `updater.allowJsDowngrade`). */
  allowDowngrade?: boolean;
  /** Accept pre-release versions. */
  allowPrerelease?: boolean;
  /** Request timeout in milliseconds. */
  timeout?: number;
}

/** Progress of {@link Update.download} and {@link Update.downloadAndInstall}. */
export type DownloadEvent =
  | {
      /** The download started. */
      event: 'Started';
      /** The download size. */
      data: {
        /** The installer size in bytes, when the server sends it. */
        contentLength?: number;
      };
    }
  | {
      /** A chunk arrived. */
      event: 'Progress';
      /** The chunk. */
      data: {
        /** The chunk size in bytes. */
        chunkLength: number;
      };
    }
  | {
      /** The installer is downloaded and verified. */
      event: 'Finished';
    };

/** What `updater_check` returns for an available update. */
interface UpdateMetadata {
  rid: number;
  version: string;
  currentVersion: string;
  date?: string | null;
  body?: string | null;
  raw: Record<string, unknown>;
}

/**
 * An available update. It holds a plugin resource; {@link Update.close}
 * releases it (also done by a finished install).
 */
export class Update extends Resource {
  /** The version on the feed. */
  readonly version: string;
  /** The running version. */
  readonly currentVersion: string;
  /** The release date from the feed, if any. */
  readonly date?: string;
  /** The release notes from the feed, if any. */
  readonly body?: string;
  /** The feed entry as published (electron-updater `latest.yml` fields). */
  readonly raw: Record<string, unknown>;

  /**
   * @param metadata - what `updater_check` returned
   * @internal
   */
  constructor(metadata: UpdateMetadata) {
    super(metadata.rid);
    this.version = metadata.version;
    this.currentVersion = metadata.currentVersion;
    if (typeof metadata.date === 'string') this.date = metadata.date;
    if (typeof metadata.body === 'string') this.body = metadata.body;
    this.raw = metadata.raw;
  }

  /**
   * Downloads the installer and verifies its hash and publisher.
   *
   * @param onEvent - progress callback
   * @returns resolves when the installer is downloaded and verified
   */
  async download(onEvent?: (event: DownloadEvent) => void): Promise<void> {
    await call<null>('updater_download', { rid: this.rid, onEvent: channel(onEvent) });
  }

  /**
   * Installs the downloaded update: re-verifies the installer, starts it and
   * exits the app (an explicit request of the app).
   *
   * @returns resolves just before the app exits
   */
  async install(): Promise<void> {
    await call<null>('updater_install', { rid: this.rid });
  }

  /**
   * {@link Update.download} then {@link Update.install}.
   *
   * @param onEvent - progress callback
   * @returns resolves just before the app exits
   */
  async downloadAndInstall(onEvent?: (event: DownloadEvent) => void): Promise<void> {
    await call<null>('updater_download_and_install', {
      rid: this.rid,
      onEvent: channel(onEvent),
    });
  }
}

function channel(onEvent?: (event: DownloadEvent) => void): Channel<DownloadEvent> {
  const events = new Channel<DownloadEvent>();
  if (onEvent) events.onmessage = onEvent;
  return events;
}

/**
 * Checks Overwolf's update feed.
 *
 * @param options - channel, downgrade and pre-release choices, timeout
 * @returns the update, or `null` when the app is up to date
 */
export async function check(options?: CheckOptions): Promise<Update | null> {
  const metadata = await call<UpdateMetadata | null>('updater_check', {
    options: options ?? null,
  });
  return metadata ? new Update(metadata) : null;
}
