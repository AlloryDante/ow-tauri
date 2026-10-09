// The lab driver on ow-tauri: bundled in front of the showcase page by
// `scripts/stage.mjs --lab` only (the normal build never includes it). It
// does nothing unless the app was built with the `lab` Cargo feature and
// launched by the runner, which passes OW_SHOWCASE_E2E_CONFIG.
//
// It runs the shared steps (steps.cjs) inside the showcase page itself: the
// steps' `executeJavaScript` is an indirect eval in this page (the lab build
// allows it in its CSP), and the lab commands of src-tauri/src/lab.rs give
// it the records, the window list, the native probes and the quit.
//
// It also records a heartbeat every 5 s and every page visibility change,
// so the runner's stall watchdog can tell a stalled page (hidden: WebKit
// stops its timers) from a slow step.

import { invoke } from '@tauri-apps/api/core';

import { restartPhaseOf, runSteps } from './steps.cjs';

/** The route the page started on (read before the page can change it). */
const startHash = location.hash;

/** How often the driver records a heartbeat (the runner's stall watchdog). */
const BEAT_MS = 5000;

/** Evaluates `code` in the page, as Electron's `executeJavaScript` does. */
const evaluate = (code) => Promise.resolve((0, eval)(code));

async function start() {
  let config;
  try {
    config = await invoke('e2e_config');
  } catch {
    return; // not a lab build: stay inert
  }
  if (!config) return;
  // Records leave in order; a failed write is retried once.
  let chain = Promise.resolve();
  const record = (entry) => {
    chain = chain.then(() =>
      invoke('e2e_record', { entry })
        .catch(() => invoke('e2e_record', { entry }))
        .catch(() => undefined),
    );
  };
  const restartPhase = restartPhaseOf(startHash);
  record({ kind: 'driver', host: 'tauri', pid: config.pid, restartPhase, startHash });
  document.addEventListener('visibilitychange', () => {
    record({ kind: 'visibility', state: document.visibilityState });
  });
  const beat = setInterval(() => {
    record({ kind: 'beat', visibility: document.visibilityState });
  }, BEAT_MS);
  const page = { webContents: { executeJavaScript: evaluate } };
  const host = {
    name: 'tauri',
    config,
    record,
    mainWindow: () => page,
    quit: () => {
      clearInterval(beat);
      void chain.then(() => invoke('e2e_quit'));
    },
    // The window's native hit test and the test-mode click (src-tauri/src/lab.rs).
    nativeProbe: (points, click) => invoke('e2e_native_probe', { points, click }),
    still: (name) => invoke('e2e_still', { name }),
    windows: () => invoke('e2e_windows'),
    restartPhase,
    flush: () => chain,
  };
  runSteps(host).catch((e) => {
    record({ kind: 'fatal', text: String(e && e.stack) });
    host.quit();
  });
}

void start();
