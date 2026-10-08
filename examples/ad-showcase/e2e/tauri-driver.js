// The lab driver on ow-tauri: bundled in front of the main process by
// `scripts/stage.mjs --lab` only (the normal build never includes it). It
// does nothing unless the app was built with the `lab` Cargo feature and
// launched by the runner, which passes OW_SHOWCASE_E2E_CONFIG.
//
// It runs the shared steps (steps.cjs) through the `ow-tauri/electron`
// facade: the same BrowserWindow / executeJavaScript calls the ow-electron
// baseline makes.

import { app, BrowserWindow } from 'electron';

import { runSteps } from './steps.cjs';

const invoke = (cmd, args) => globalThis.__TAURI_INTERNALS__.invoke(cmd, args);

// The showcase's show() runs unchanged: in the invisible lab the plugin
// itself keeps the app in the background (no activation, the window
// ordered front without becoming key; tauri-plugin-overwolf `lab`).

const isShowcase = (w) => /renderer\/index\.html/.test(w.webContents.getURL() || '');

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
  record({
    kind: 'driver',
    host: 'tauri',
    versions: globalThis.process && globalThis.process.versions,
  });
  const host = {
    name: 'tauri',
    config,
    record,
    mainWindow: () => BrowserWindow.getAllWindows().find(isShowcase) || null,
    quit: () => {
      void chain.then(() => app.quit());
    },
    // The window's native hit test and the test-mode click (src-tauri/src/lab.rs).
    nativeProbe: (points, click) => invoke('e2e_native_probe', { points, click }),
    still: (name) => invoke('e2e_still', { name }),
    probeGuests: (phase) => invoke('e2e_probe_guests', { phase }),
    windows: () =>
      BrowserWindow.getAllWindows().map((w) => ({
        id: w.id,
        visible: w.isVisible(),
        url: (w.webContents.getURL() || '').replace(/^.*\/renderer\//, '<app>/renderer/'),
      })),
  };
  app
    .whenReady()
    .then(() => runSteps(host).catch((e) => record({ kind: 'fatal', text: String(e && e.stack) })));
}

void start();
