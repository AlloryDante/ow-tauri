// The ow-electron side of the lab: the main entry of a throwaway app folder
// that `run.mjs --host electron` prepares around the staged showcase
// (.stage/electron). It keeps every window invisible (opacity 0, ignoring
// the mouse, not focusable, as tools/parity-harness does), keeps dialogs and
// the file manager closed, loads the showcase's main process unchanged and
// runs the same steps (steps.cjs) as the ow-tauri driver.

'use strict';

const { app, BrowserWindow, dialog, shell, webContents } = require('electron');
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

// --- Ad guest console (observation only) ----------------------------------
// What the ad pages log (`<owadview> is not visible. waiting...` and the like)
// goes to guest-console.jsonl, as the ow-tauri lab trace records its guests.
// The <owadview> guests are not always announced through
// 'web-contents-created', so a poll picks them up too.
const watchedContents = new Set();
function watchConsole(wc) {
  if (watchedContents.has(wc.id)) return;
  watchedContents.add(wc.id);
  wc.on('console-message', (...args) => {
    const detail = args[0] && typeof args[0] === 'object' && 'message' in args[0] ? args[0] : null;
    const message = detail ? detail.message : args[2];
    const type = wc.getType();
    if (type === 'window') return;
    fs.appendFileSync(
      path.join(config.runDir, 'guest-console.jsonl'),
      JSON.stringify({
        wall: Date.now(),
        id: wc.id,
        type,
        message: String(message).slice(0, 2000),
      }) + '\n',
    );
  });
}
app.on('web-contents-created', (_event, wc) => watchConsole(wc));
app.whenReady().then(() => {
  setInterval(() => {
    for (const wc of webContents.getAllWebContents()) watchConsole(wc);
  }, 250).unref();
});

// --- The showcase, unchanged ------------------------------------------------
require('./main/main.js');

// --- The steps ----------------------------------------------------------------
const STILL_TIMEOUT_MS = 3000;
const isShowcase = (w) => /renderer\/index\.html/.test(w.webContents.getURL() || '');
const host = {
  name: 'electron',
  config,
  record: (entry) => fs.writeSync(out, JSON.stringify({ wall: Date.now(), ...entry }) + '\n'),
  mainWindow: () =>
    BrowserWindow.getAllWindows().find((w) => !w.isDestroyed() && isShowcase(w)) || null,
  quit: () => app.quit(),
  // A click into the app's own page (test mode only; the steps send it only
  // when the page's hit test names the app's button, never at an ad).
  pageClick: (x, y) => {
    if (config.mode !== 'test') return { sent: false, refused: 'not in test mode' };
    const win = host.mainWindow();
    if (!win) return { sent: false, refused: 'no window' };
    for (const type of ['mouseDown', 'mouseUp']) {
      win.webContents.sendInputEvent({ type, x, y, button: 'left', clickCount: 1 });
    }
    return { sent: true };
  },
  windows: () =>
    BrowserWindow.getAllWindows().map((w) => ({
      id: w.id,
      visible: w.isVisible(),
      url: (w.webContents.getURL() || '').replace(/^.*\/renderer\//, '<app>/renderer/'),
    })),
  // Each ad guest renders itself (`webContents.capturePage()`, in process;
  // nothing captures the screen): `<name>.electron-guest-<id>-<w>x<h>.png`
  // in the stills folder, plus how much of it is painted, so a blank
  // creative can be told from a host that shows nothing. A hidden guest
  // (display: none) never answers capturePage(); it is skipped after
  // STILL_TIMEOUT_MS.
  still: async (name) => {
    const guests = webContents
      .getAllWebContents()
      .filter((wc) => !wc.isDestroyed() && wc.getType() === 'owadview');
    const shots = [];
    for (const wc of guests) {
      const image = await Promise.race([
        wc.capturePage(),
        new Promise((done) => setTimeout(() => done(null), STILL_TIMEOUT_MS)),
      ]);
      if (!image) {
        shots.push({ id: wc.id, timedOut: true });
        continue;
      }
      const { width, height } = image.getSize();
      shots.push({ id: wc.id, width, height, ...paintStats(image.toBitmap()) });
      if (config.stillsDir && width > 0 && height > 0) {
        fs.mkdirSync(config.stillsDir, { recursive: true });
        fs.writeFileSync(
          path.join(config.stillsDir, `${name}.electron-guest-${wc.id}-${width}x${height}.png`),
          image.toPNG(),
        );
      }
    }
    return { guests: shots };
  },
};

/**
 * How much of a BGRA bitmap is painted: the share of pixels that are not
 * transparent, and how many distinct colours (4 bits per channel) they use.
 * A blank creative is transparent or one flat colour.
 */
function paintStats(bitmap) {
  let opaque = 0;
  const colours = new Set();
  const pixels = bitmap.length / 4;
  for (let i = 0; i < bitmap.length; i += 4) {
    if (bitmap[i + 3] < 16) continue;
    opaque += 1;
    if (colours.size < 64) {
      colours.add(((bitmap[i] >> 4) << 8) | ((bitmap[i + 1] >> 4) << 4) | (bitmap[i + 2] >> 4));
    }
  }
  return {
    opaqueShare: pixels ? Math.round((opaque / pixels) * 1000) / 1000 : 0,
    colours: colours.size,
  };
}
host.record({ kind: 'driver', host: 'electron', versions: process.versions });
app
  .whenReady()
  .then(() =>
    runSteps(host).catch((e) => host.record({ kind: 'fatal', text: String(e && e.stack) })),
  );
