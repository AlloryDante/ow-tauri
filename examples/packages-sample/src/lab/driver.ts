/**
 * The invisible lab's driver (e2e/README.md). Bundled only into the lab
 * build (`vite build --mode lab`, see `main.tsx`) and inert unless the app
 * was built with the `lab` Cargo feature and launched by `e2e/run.mjs`,
 * which passes `OW_SAMPLE_E2E_CONFIG`.
 *
 * The smoke run opens the ads tester, presses "Start ad" on both slots of
 * the default layout (buttons of the sample's own page, never an ad), waits
 * for `display_ad_loaded`, records what happened through the lab commands
 * of `src-tauri/src/lab.rs` and quits the app.
 *
 * @packageDocumentation
 */
import { invoke } from '@tauri-apps/api/core';

import type { LogStore } from '../log/store';

/** The runner's configuration (`OW_SAMPLE_E2E_CONFIG` plus the app's pid). */
export interface DriverConfig {
  /** How long to wait for `display_ad_loaded`, ms (default 60000). */
  adWaitMs?: number;
  /** The app process id (added by `e2e_config`). */
  pid?: number;
}

/** One record of `e2e.jsonl`. */
export type DriverRecord = Record<string, unknown> & { kind: string };

/** What the driver needs from its host; tests pass fakes. */
export interface DriverHost {
  /** The run configuration, or `null` outside a lab run. */
  config: () => Promise<DriverConfig | null>;
  /** Appends a record to `e2e.jsonl`. */
  record: (entry: DriverRecord) => Promise<void>;
  /** The window state the OS reports (visible, minimized, focused). */
  window: () => Promise<unknown>;
  /** Quits the app. */
  quit: () => Promise<void>;
  /** Waits `ms` milliseconds. */
  sleep: (ms: number) => Promise<void>;
}

/** The lab commands of the app. */
export const tauriHost: DriverHost = {
  config: async () => {
    try {
      return await invoke<DriverConfig | null>('e2e_config');
    } catch {
      // Not a lab build: the command does not exist.
      return null;
    }
  },
  record: async (entry) => {
    await invoke('e2e_record', { entry }).catch(() => invoke('e2e_record', { entry }));
  },
  window: () => invoke('e2e_window'),
  quit: async () => {
    await invoke('e2e_quit');
  },
  sleep: (ms) =>
    new Promise((resolve) => {
      setTimeout(resolve, ms);
    }),
};

/** How often the driver polls the page, ms. */
const POLL_MS = 100;

/**
 * Waits until `found()` returns a value, or `ms` milliseconds have passed.
 *
 * @returns the value, or `null` on timeout
 */
async function waitFor<T>(host: DriverHost, ms: number, found: () => T | null): Promise<T | null> {
  for (let waited = 0; ; waited += POLL_MS) {
    const value = found();
    if (value !== null) return value;
    if (waited >= ms) return null;
    await host.sleep(POLL_MS);
  }
}

/** The "Start ad" button of slot `name` on the ads tester, if rendered. */
function startButton(doc: Document, name: string): HTMLButtonElement | null {
  const buttons = doc.querySelectorAll<HTMLButtonElement>(`[data-slot="${name}"] button`);
  return [...buttons].find((b) => b.textContent === 'Start ad') ?? null;
}

/** The page's view of the ad slots, for the run record. */
function viewOf(doc: Document): Record<string, unknown> {
  const win = doc.defaultView;
  return {
    visibility: doc.visibilityState,
    viewport: win ? [win.innerWidth, win.innerHeight] : null,
    slots: [...doc.querySelectorAll('owadview')].map((el) => {
      const box = el.getBoundingClientRect();
      return {
        box: [box.x, box.y, box.width, box.height].map(Math.round),
        shown: el.checkVisibility({ opacityProperty: true, visibilityProperty: true }),
      };
    }),
  };
}

/**
 * Runs the smoke steps.
 *
 * @param log - the app's log store (the ad events are read from it)
 * @param host - the lab commands (default: the app's)
 * @param doc - the page (default: `document`)
 * @returns resolves once the run is recorded and the quit requested
 */
export async function startDriver(
  log: LogStore,
  host: DriverHost = tauriHost,
  doc: Document = document,
): Promise<void> {
  const config = await host.config();
  if (!config) return;
  try {
    await host.record({ kind: 'driver', pid: config.pid ?? null });
    await host.record({ kind: 'step', name: 'started' });
    // A user presses "Start ad" in a shown window: wait for the page to be.
    const shown = await waitFor(host, 10_000, () =>
      doc.visibilityState === 'visible' ? true : null,
    );
    await host.record({ kind: 'step', name: 'shown', shown: shown ?? false });
    doc.defaultView?.location.assign('#ads');
    const started: string[] = [];
    for (const slot of ['ad1', 'ad2']) {
      const button = await waitFor(host, 10_000, () => startButton(doc, slot));
      if (button) {
        button.click();
        started.push(slot);
      }
    }
    await host.record({ kind: 'step', name: 'ads-started', slots: started });
    const loaded = await waitFor(
      host,
      config.adWaitMs ?? 60_000,
      () =>
        log.entries().find((e) => e.source === 'ad' && e.message.endsWith(': display_ad_loaded')) ??
        null,
    );
    const events = log
      .entries()
      .filter((e) => e.source === 'ad')
      .map((e) => e.message);
    await host.record({
      kind: 'step',
      name: 'ads',
      owadviews: doc.querySelectorAll('owadview').length,
      view: viewOf(doc),
      window: await host.window(),
      events,
      errors: log
        .entries()
        .filter((e) => e.level === 'error')
        .map((e) => e.message),
    });
    await host.record({ kind: 'done', displayAdLoaded: loaded !== null });
  } catch (error) {
    await host.record({
      kind: 'fatal',
      text: String(error instanceof Error ? error.stack : error),
    });
  }
  await host.quit();
}
