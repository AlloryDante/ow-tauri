// The ow-electron baseline of the end-to-end run: the main entry of a
// throwaway copy of the UPSTREAM sample (built with its own webpack configs),
// prepared and launched by `run.mjs --host electron`. It makes every window
// invisible (as the parity harness's ow-electron app does), keeps dialogs
// and the file manager closed, loads the upstream main bundle unchanged and
// runs the same steps (`steps.js`) as the ow-tauri driver.

'use strict';

const { app, BrowserWindow, dialog, shell } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { runSteps } = require('./steps.js');

const config = JSON.parse(process.env.OW_SAMPLE_E2E_CONFIG || '{}');
const out = fs.openSync(path.join(config.runDir, 'e2e.jsonl'), 'a');
const t0 = Date.now();
const record = (entry) => {
  fs.writeSync(out, JSON.stringify({ t: Date.now() - t0, wall: Date.now(), ...entry }) + '\n');
};
const blocked = (kind, detail) => {
  fs.appendFileSync(
    path.join(config.runDir, 'blocked.jsonl'),
    JSON.stringify({ t: Date.now() - t0, wall: Date.now(), kind, detail }) + '\n',
  );
};

// --- No Dock icon, no visible window ----------------------------------------
if (process.platform === 'darwin' && app.dock) app.dock.hide();
// An accessory app has no Dock icon and is not activated by launching it.
if (process.platform === 'darwin' && typeof app.setActivationPolicy === 'function') {
  app.setActivationPolicy('accessory');
  app.whenReady().then(() => app.setActivationPolicy('accessory'));
}
for (const method of ['focus', 'show']) {
  if (typeof app[method] !== 'function') continue;
  app[method] = (...args) => blocked(`app.${method}`, args);
}

const proto = BrowserWindow.prototype;
const original = {
  BrowserWindow,
  showInactive: proto.showInactive,
  setOpacity: proto.setOpacity,
  setIgnoreMouseEvents: proto.setIgnoreMouseEvents,
  setFocusable: proto.setFocusable,
};
function makeInvisible(win) {
  try {
    original.setOpacity.call(win, 0);
    original.setIgnoreMouseEvents.call(win, true);
    original.setFocusable.call(win, false);
    win.setSkipTaskbar(true);
  } catch (error) {
    blocked('make-invisible-failed', String(error));
  }
}
// A shown window is shown inactive at opacity 0; calls that could raise,
// focus or full-screen a window are dropped.
for (const method of ['show', 'showInactive']) {
  proto[method] = function show() {
    makeInvisible(this);
    return original.showInactive.call(this);
  };
}
for (const method of ['focus', 'moveTop', 'setFullScreen', 'setSimpleFullScreen', 'setKiosk', 'flashFrame']) {
  const fn = proto[method];
  if (typeof fn !== 'function') continue;
  proto[method] = function dropped(...args) {
    blocked(`BrowserWindow.${method}`, { id: this.id, args });
    return undefined;
  };
}
proto.setOpacity = function pinned() {
  return original.setOpacity.call(this, 0);
};
proto.setIgnoreMouseEvents = function pinned() {
  return original.setIgnoreMouseEvents.call(this, true);
};
proto.setFocusable = function pinned() {
  return original.setFocusable.call(this, false);
};
// Runs before the constructor applies its options (the parity harness
// calibrates this), so a window built with show:true appears at opacity 0.
app.on('browser-window-created', (_e, win) => {
  makeInvisible(win);
  win.on('show', () => makeInvisible(win));
});

// A window built with show:true is shown by the constructor itself, and on
// macOS Electron then activates the app: the invisible app became the
// frontmost app and took the keyboard. The upstream bundle (and anything it
// loads) gets an `electron` module whose BrowserWindow is built hidden and
// not full screen, then shown inactive through the patched showInactive.
// The subclass keeps the name BrowserWindow: Electron's
// BrowserWindow.getAllWindows() keeps windows by their constructor's name.
const InvisibleBrowserWindow = class BrowserWindow extends original.BrowserWindow {
  constructor(options = {}) {
    const show = options.show !== false;
    if (options.fullscreen) blocked('BrowserWindow fullscreen option', { fullscreen: true });
    super({ ...options, show: false, fullscreen: false });
    if (show) this.showInactive();
  }
};
{
  const electron = require('electron');
  const patched = new Proxy(electron, {
    get: (target, key) => (key === 'BrowserWindow' ? InvisibleBrowserWindow : Reflect.get(target, key)),
  });
  const Module = require('node:module');
  const load = Module._load;
  Module._load = function loadElectron(request, ...rest) {
    return request === 'electron' ? patched : load.call(this, request, ...rest);
  };
}

// --- Dialogs and the file manager stay closed ---------------------------------
// As in ow-tauri's lab mode: answered as if dismissed at once.
dialog.showOpenDialog = async (...args) => {
  blocked('dialog.showOpenDialog', args.length > 1 ? args[1] : args[0]);
  return { canceled: true, filePaths: [] };
};
dialog.showSaveDialog = async (...args) => {
  blocked('dialog.showSaveDialog', args.length > 1 ? args[1] : args[0]);
  return { canceled: true, filePath: '' };
};
dialog.showMessageBox = async (...args) => {
  const o = args.length > 1 ? args[1] : args[0];
  blocked('dialog.showMessageBox', o);
  return { response: o && o.cancelId !== undefined ? o.cancelId : 0, checkboxChecked: false };
};
dialog.showErrorBox = (...args) => blocked('dialog.showErrorBox', args);
shell.openPath = async (p) => {
  blocked('shell.openPath', p);
  return '';
};
shell.openExternal = async (u) => blocked('shell.openExternal', u);
shell.showItemInFolder = (p) => blocked('shell.showItemInFolder', p);

// --- Main-process output -----------------------------------------------------
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
  const originalLog = console[level].bind(console);
  console[level] = (...args) => {
    mainBuffer.push({ t: Date.now(), kind: 'console', level, text: args.map(fmt).join(' ') });
    originalLog(...args);
  };
}
process.on('unhandledRejection', (reason) =>
  mainBuffer.push({ t: Date.now(), kind: 'unhandledrejection', text: fmt(reason) }),
);
process.on('uncaughtException', (error) =>
  mainBuffer.push({ t: Date.now(), kind: 'error', text: fmt(error) }),
);

// --- The upstream sample, unchanged ---------------------------------------------
require('./dist/browser/index.js');

// --- The steps ---------------------------------------------------------------
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

record({ kind: 'driver', host: 'electron', versions: process.versions });
const host = {
  name: 'electron',
  config,
  record,
  overwolf: app.overwolf,
  mainWindow: () => BrowserWindow.getAllWindows().find(isMainWindow) || null,
  windows: () =>
    BrowserWindow.getAllWindows()
      .filter((w) => !w.isDestroyed())
      .map((w) => ({ id: w.id, url: w.webContents.getURL(), visible: w.isVisible() })),
  takeMain: () => mainBuffer.splice(0),
  async checkUpdates(url) {
    const { autoUpdater } = require('electron-updater');
    const events = [];
    const listeners = updateEvents.map((name) => {
      const fn = (...args) => events.push({ event: name, args: args.map(summarise) });
      autoUpdater.on(name, fn);
      return [name, fn];
    });
    autoUpdater.forceDevUpdateConfig = true;
    autoUpdater.autoDownload = false;
    autoUpdater.channel = 'testingChannelz';
    autoUpdater.allowDowngrade = false;
    autoUpdater.setFeedURL({ provider: 'generic', url });
    let result = null;
    let error = null;
    try {
      const r = await autoUpdater.checkForUpdates();
      result = r ? { isUpdateAvailable: r.isUpdateAvailable, updateInfo: summarise(r.updateInfo) } : null;
    } catch (e) {
      error = String(e && e.message).split('\n')[0];
    }
    await new Promise((r) => setTimeout(r, 300));
    for (const [name, fn] of listeners) autoUpdater.removeListener(name, fn);
    return { result, error, events };
  },
  quit: () => app.quit(),
};
app.whenReady().then(() =>
  runSteps(host).catch((e) => record({ kind: 'fatal', text: String(e && e.stack) })),
);
