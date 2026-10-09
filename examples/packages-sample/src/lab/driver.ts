/**
 * The invisible lab's driver (e2e/README.md). Bundled only into the lab
 * build (`vite build --mode lab`, see `main.tsx`) and inert unless the app
 * was built with the `lab` Cargo feature and launched by `e2e/run.mjs`,
 * which passes `OW_SAMPLE_E2E_CONFIG`. It presses the sample's own buttons
 * only, never an ad.
 *
 * - `smoke` (default): opens the ads tester, presses "Start ad" on both
 *   slots of the default layout, waits for `display_ad_loaded`, records
 *   what happened and quits.
 * - `restart`: the restart check. The first process opens the CMP &
 *   settings page (no ad guest) and presses "Restart with live ads"; the
 *   second (LIVE, on the same page) presses "Restart with test ads"; the
 *   third (TEST, same page) records `done` and quits. Each process records
 *   its phase and pid.
 * - `tour`: a still of every page (`e2e_still`; in-process, no screen
 *   capture), the ads tester with both test ads loaded, and the ad privacy
 *   settings window the plugin opens (put on screen at alpha 0 for the
 *   still), then quits.
 *
 * Every run also records page visibility changes and a heartbeat every
 * {@link BEAT_MS}, so the runner can tell a stalled page (WebKit stops the
 * timers of a page it considers hidden) from a slow step.
 *
 * @packageDocumentation
 */
import { invoke } from '@tauri-apps/api/core';

import type { LogStore } from '../log/store';
import { restartApp, type AdMode } from '../restart';

/** The runner's configuration (`OW_SAMPLE_E2E_CONFIG` plus the app's pid). */
export interface DriverConfig {
  /** What to run (default `smoke`). */
  steps?: 'smoke' | 'restart' | 'tour';
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
  /** Writes a still of a window (default the sample's) named `name`. */
  still?: (name: string, window?: string) => Promise<unknown>;
  /** Puts a consent window the plugin keeps hidden in the lab on screen at alpha 0. */
  reveal?: (label: string) => Promise<unknown>;
  /** Restarts the app; answers why it refused, or `null`. */
  restart?: (mode: AdMode) => Promise<string | null>;
  /** Calls `tick` every `ms` until the returned function is called. */
  every?: (ms: number, tick: () => void) => () => void;
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
  still: (name, window) => invoke('e2e_still', { name, window: window ?? null }),
  reveal: (label) => invoke('e2e_reveal', { label }),
  restart: (mode) => restartApp(mode),
  every: (ms, tick) => {
    const id = setInterval(tick, ms);
    return () => {
      clearInterval(id);
    };
  },
};

/** How often the driver polls the page, ms. */
const POLL_MS = 100;
/** How often the driver records a heartbeat, ms. */
export const BEAT_MS = 5000;
/** The tour's pages, in navigation order, and the still of each. */
export const TOUR = [
  { page: 'logger', still: 'logger' },
  { page: 'ads', still: 'ads-tester' },
  { page: 'settings', still: 'settings' },
  { page: 'updater', still: 'updater' },
  { page: 'packages', still: 'packages' },
] as const;
/** The page the restart check restarts on (no ad guest). */
export const RESTART_PAGE = 'settings';
/** The label of the ad privacy settings window the plugin opens. */
const PRIVACY_WINDOW = 'ow-cmp';

/**
 * The restart check's phase of this process: 1 when it started without a
 * page (the runner's launch), else 2 in LIVE mode and 3 in TEST mode (the
 * two restarts keep the page).
 *
 * @param startHash - `location.hash` when the driver started
 * @param mode - the ad mode the page shows
 * @returns the phase
 */
export function restartPhase(startHash: string, mode: AdMode): 1 | 2 | 3 {
  if (startHash === '' || startHash === '#') return 1;
  return mode === 'live' ? 2 : 3;
}

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

/** The first enabled button under `root` whose text is `text`, if rendered. */
function buttonNamed(root: ParentNode, text: string): HTMLButtonElement | null {
  const buttons = root.querySelectorAll<HTMLButtonElement>('button');
  return [...buttons].find((b) => b.textContent === text && !b.disabled) ?? null;
}

/** The "Start ad" button of slot `name` on the ads tester, if rendered. */
function startButton(doc: Document, name: string): HTMLButtonElement | null {
  const slot = doc.querySelector(`[data-slot="${name}"]`);
  return slot ? buttonNamed(slot, 'Start ad') : null;
}

/** The ad mode the settings page shows, once it has loaded. */
function shownMode(doc: Document): AdMode | null {
  const text = doc.querySelector('[data-testid="ad-mode"]')?.textContent;
  if (text === 'test ads') return 'test';
  if (text === 'live ads') return 'live';
  return null;
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

/** The `display_ad_loaded` log lines of the ad slots. */
function loadedAds(log: LogStore): number {
  return log.entries().filter((e) => e.source === 'ad' && e.message.endsWith(': display_ad_loaded'))
    .length;
}

/**
 * Runs the configured steps.
 *
 * @param log - the app's log store (the ad events are read from it)
 * @param host - the lab commands (default: the app's)
 * @param doc - the page (default: `document`)
 * @returns resolves once the run is recorded and the quit (or restart)
 *   requested
 */
export async function startDriver(
  log: LogStore,
  host: DriverHost = tauriHost,
  doc: Document = document,
): Promise<void> {
  const config = await host.config();
  if (!config) return;
  const steps = config.steps ?? 'smoke';
  const startHash = doc.defaultView?.location.hash ?? '';
  const visibility = (): void => {
    void host.record({ kind: 'visibility', state: doc.visibilityState }).catch(() => undefined);
  };
  doc.addEventListener('visibilitychange', visibility);
  const stopBeat = host.every?.(BEAT_MS, () => {
    void host.record({ kind: 'beat', visibility: doc.visibilityState }).catch(() => undefined);
  });
  const go = (page: string): void => {
    doc.defaultView?.location.assign(`#${page}`);
  };

  const shownStep = async (): Promise<void> => {
    // A user presses buttons in a shown window: wait for the page to be.
    const shown = await waitFor(host, 10_000, () =>
      doc.visibilityState === 'visible' ? true : null,
    );
    await host.record({ kind: 'step', name: 'shown', shown: shown ?? false });
  };

  const smoke = async (): Promise<void> => {
    go('ads');
    const started: string[] = [];
    for (const slot of ['ad1', 'ad2']) {
      const button = await waitFor(host, 10_000, () => startButton(doc, slot));
      if (button) {
        button.click();
        started.push(slot);
      }
    }
    await host.record({ kind: 'step', name: 'ads-started', slots: started });
    const loaded = await waitFor(host, config.adWaitMs ?? 60_000, () =>
      loadedAds(log) > 0 ? true : null,
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
  };

  const restart = async (): Promise<void> => {
    if (!host.restart) throw new Error('the host cannot restart the app');
    go(RESTART_PAGE);
    const mode = await waitFor(host, 15_000, () => shownMode(doc));
    if (!mode) throw new Error('the settings page never showed the ad mode');
    const phase = restartPhase(startHash, mode);
    await host.record({
      kind: 'step',
      name: `restart-${String(phase)}`,
      phase,
      pid: config.pid ?? null,
      mode,
      startHash,
      page: doc.defaultView?.location.hash ?? null,
    });
    if (phase === 3) {
      await host.record({ kind: 'done', restarted: true, mode, page: RESTART_PAGE });
      return;
    }
    const next: AdMode = phase === 1 ? 'live' : 'test';
    const button = buttonNamed(doc, `Restart with ${next} ads`);
    if (!button) throw new Error(`no "Restart with ${next} ads" button`);
    await host.record({ kind: 'restart-requested', phase, mode: next });
    // The sample's own button (it calls `sample_restart`, which exits this
    // process once the plugins are done; the new one starts at exit).
    button.click();
    const notice = await waitFor(
      host,
      15_000,
      () => doc.querySelector('.notice')?.textContent ?? null,
    );
    throw new Error(`the restart did not exit the app (${notice ?? 'no answer'})`);
  };

  const tour = async (): Promise<void> => {
    if (!host.still) throw new Error('the host takes no stills');
    for (const { page, still } of TOUR) {
      go(page);
      await host.sleep(1500);
      if (page === 'ads') {
        for (const slot of ['ad1', 'ad2']) {
          const button = await waitFor(host, 10_000, () => startButton(doc, slot));
          button?.click();
        }
        const loaded = await waitFor(host, config.adWaitMs ?? 60_000, () =>
          loadedAds(log) >= 2 ? true : null,
        );
        await host.record({ kind: 'step', name: 'ads-loaded', loaded: loaded !== null });
        // `display_ad_loaded` fires before the creative has painted.
        await host.sleep(3000);
      }
      if (page === 'settings') await waitFor(host, 15_000, () => shownMode(doc));
      const out = await host.still(still);
      await host.record({ kind: 'still', name: still, page, out });
      if (page === 'settings') await privacyWindow();
    }
    await host.record({ kind: 'done', stills: TOUR.length + 1 });
  };

  /** Opens the ad privacy settings window and records a still of it. */
  const privacyWindow = async (): Promise<void> => {
    const open = buttonNamed(doc, 'openAdPrivacySettingsWindow()');
    if (!open || !host.reveal || !host.still) throw new Error('cannot open the privacy window');
    open.click();
    let revealed: unknown = null;
    for (let i = 0; i < 50 && revealed === null; i += 1) {
      revealed = await host.reveal(PRIVACY_WINDOW).catch(() => null);
      if (revealed === null) await host.sleep(200);
    }
    if (revealed === null) throw new Error('the privacy settings window never opened');
    // The consent page loads from Overwolf's CDN and lays itself out.
    await host.sleep(8000);
    const out = await host.still('privacy-settings', PRIVACY_WINDOW);
    await host.record({ kind: 'still', name: 'privacy-settings', window: revealed, out });
  };

  try {
    await host.record({ kind: 'driver', pid: config.pid ?? null });
    await host.record({ kind: 'step', name: 'started' });
    await shownStep();
    if (steps === 'restart') await restart();
    else if (steps === 'tour') await tour();
    else await smoke();
  } catch (error) {
    await host.record({
      kind: 'fatal',
      text: String(error instanceof Error ? error.stack : error),
    });
  }
  stopBeat?.();
  doc.removeEventListener('visibilitychange', visibility);
  await host.quit();
}
