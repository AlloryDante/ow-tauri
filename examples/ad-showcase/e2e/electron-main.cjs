// The ow-electron side of the lab: the main entry of a throwaway app folder
// that `run.mjs --host electron` prepares around the staged showcase
// (.stage/electron). It keeps every window invisible (opacity 0, ignoring
// the mouse, not focusable, as tools/parity-harness does), keeps dialogs and
// the file manager closed, loads the showcase's main process unchanged and
// runs the same steps (steps.cjs) as the ow-tauri driver.

'use strict';

const { app, BrowserWindow, dialog, shell } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { runSteps } = require('./steps.cjs');

const config = JSON.parse(process.env.OW_SHOWCASE_E2E_CONFIG || '{}');
const out = fs.openSync(path.join(config.runDir, 'e2e.jsonl'), 'a');
const blocked = (kind, detail) => {
  fs.appendFileSync(
    path.join(config.runDir, 'blocked.jsonl'),
    JSON.stringify({ wall: Date.now(), kind, detail }) + '\n',
  );
};

// --- No Dock icon, no visible window -------------------------------------
if (process.platform === 'darwin' && app.dock) app.dock.hide();

const proto = BrowserWindow.prototype;
const original = {
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
for (const method of ['show', 'showInactive']) {
  proto[method] = function show() {
    makeInvisible(this);
    return original.showInactive.call(this);
  };
}
for (const method of [
  'focus',
  'moveTop',
  'setFullScreen',
  'setSimpleFullScreen',
  'setKiosk',
  'flashFrame',
]) {
  if (typeof proto[method] !== 'function') continue;
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
// Runs before the constructor applies its options, so a window built with
// show: true appears at opacity 0.
app.on('browser-window-created', (_e, win) => {
  makeInvisible(win);
  win.on('show', () => makeInvisible(win));
});

// --- Dialogs and the file manager stay closed -----------------------------
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

// --- The showcase, unchanged ------------------------------------------------
require('./main/main.js');

// --- The steps ----------------------------------------------------------------
const isShowcase = (w) => /renderer\/index\.html/.test(w.webContents.getURL() || '');
const host = {
  name: 'electron',
  config,
  record: (entry) => fs.writeSync(out, JSON.stringify({ wall: Date.now(), ...entry }) + '\n'),
  mainWindow: () =>
    BrowserWindow.getAllWindows().find((w) => !w.isDestroyed() && isShowcase(w)) || null,
  quit: () => app.quit(),
};
host.record({ kind: 'driver', host: 'electron', versions: process.versions });
app
  .whenReady()
  .then(() =>
    runSteps(host).catch((e) => host.record({ kind: 'fatal', text: String(e && e.stack) })),
  );
