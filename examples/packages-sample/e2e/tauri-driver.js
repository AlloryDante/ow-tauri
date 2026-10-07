// The end-to-end driver on ow-tauri: bundled into the sample's main webview
// bundle, before `src/browser/index.ts`, by `e2e/run.mjs` only (the normal
// `npm run build` never includes it). It does nothing unless the app was
// built with the `lab` Cargo feature and launched by the runner, which
// passes the run configuration (`OW_SAMPLE_E2E_CONFIG`).
//
// It hooks the main webview's console and errors first, then runs the
// shared steps (`steps.js`) through the `ow-tauri/electron` facade, the same
// `BrowserWindow` / `webContents.executeJavaScript` calls the baseline makes
// on ow-electron.

'use strict';

const { app, BrowserWindow } = require('electron');
const { autoUpdater } = require('ow-tauri/main');
const { runSteps } = require('./steps.js');

const internals = globalThis.__TAURI_INTERNALS__;
const invoke = (cmd, args) => internals.invoke(cmd, args);

const mainBuffer = [];
const fmt = (v) => {
  try {
    if (v instanceof Error) return `${v.name}: ${v.message}`;
    if (typeof v === 'string') return v;
    if (v === undefined) return 'undefined';
    return JSON.stringify(v);
  } catch {
    return '[unserialisable]';
  }
};
for (const level of ['log', 'info', 'warn', 'error', 'debug']) {
  const original = console[level].bind(console);
  console[level] = (...args) => {
    mainBuffer.push({ t: Date.now(), kind: 'console', level, text: args.map(fmt).join(' ') });
    original(...args);
  };
}
globalThis.addEventListener('error', (e) =>
  mainBuffer.push({ t: Date.now(), kind: 'error', text: String(e.message) }),
);
globalThis.addEventListener('unhandledrejection', (e) =>
  mainBuffer.push({ t: Date.now(), kind: 'unhandledrejection', text: fmt(e.reason) }),
);

const isMainWindow = (w) => /renderer\/index\.html/.test(w.webContents.getURL() || '');

const updateEvents = [
  'checking-for-update',
  'update-available',
  'update-not-available',
  'error',
  'download-progress',
  'update-downloaded',
];
const summarise = (value) => {
  if (value instanceof Error) return { error: value.message };
  if (value && typeof value === 'object' && 'version' in value) {
    return {
      version: value.version,
      files: (value.files || []).map((f) => f.url),
      path: value.path,
      releaseDate: value.releaseDate,
    };
  }
  return value === undefined ? null : fmt(value);
};

async function start() {
  let config;
  try {
    config = await invoke('e2e_config');
  } catch {
    return; // not a lab build: stay inert
  }
  if (!config) return;
  // Records leave in order; a failed write is retried once and counted in
  // the next record (`recordFailures`), so no step disappears silently.
  let failures = [];
  let chain = Promise.resolve();
  const record = (entry) => {
    chain = chain.then(async () => {
      const withFailures = failures.length ? { ...entry, recordFailures: failures.splice(0) } : entry;
      try {
        await invoke('e2e_record', { entry: withFailures });
      } catch (first) {
        try {
          await invoke('e2e_record', { entry: withFailures });
        } catch (second) {
          failures.push({ seq: entry.seq, kind: entry.kind, error: String(second && (second.message || second)) });
        }
      }
    });
  };
  record({ kind: 'driver', host: 'tauri', versions: globalThis.process && globalThis.process.versions });
  const host = {
    name: 'tauri',
    config,
    record,
    overwolf: app.overwolf,
    mainWindow: () => BrowserWindow.getAllWindows().find(isMainWindow) || null,
    windows: () =>
      BrowserWindow.getAllWindows().map((w) => ({
        id: w.id,
        url: w.webContents.getURL(),
        visible: w.isVisible(),
      })),
    takeMain: () => mainBuffer.splice(0),
    async checkUpdates(url) {
      const events = [];
      const listeners = updateEvents.map((name) => {
        const fn = (...args) => events.push({ event: name, args: args.map(summarise) });
        autoUpdater.on(name, fn);
        return [name, fn];
      });
      // The sample's settings (updater.service.ts), with the local feed; the
      // app is not packaged, so the dev config is forced (as upstream does).
      autoUpdater.forceDevUpdateConfig = true;
      autoUpdater.autoDownload = false;
      autoUpdater.channel = 'testingChannelz';
      autoUpdater.allowDowngrade = false;
      autoUpdater.setFeedURL({ provider: 'generic', url });
      let result = null;
      let error = null;
      try {
        const r = await autoUpdater.checkForUpdates();
        result = r
          ? { isUpdateAvailable: r.isUpdateAvailable, updateInfo: summarise(r.updateInfo) }
          : null;
      } catch (e) {
        error = String(e && e.message).split('\n')[0];
      }
      await new Promise((r) => setTimeout(r, 300));
      for (const [name, fn] of listeners) autoUpdater.removeListener(name, fn);
      return { result, error, events };
    },
    quit: () => {
      void chain.then(() => app.quit());
    },
  };
  app.whenReady().then(() =>
    runSteps(host).catch((e) => record({ kind: 'fatal', text: String(e && e.stack) })),
  );
}

void start();
