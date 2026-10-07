// Parity harness, Tauri edition: the app's main process, run in the plugin's
// hidden main webview (ow-main) through ow-tauri's Electron facade.
//
// It mirrors ../../app/main.cjs and ../../app/scenario.cjs: the same window,
// the same harness page, the same timed actions, the same app.overwolf
// snapshots and calls, the same live-load cap and quit flow. What
// ow-electron's harness reads from Electron internals (net log, CDP, cookie
// events, guest probes, IPC), the plugin's lab trace records natively
// (OW_TAURI_LAB_DIR, see crates/tauri-plugin-overwolf/src/lab.rs); run.mjs
// converts it to the ow-electron capture schema.
//
// Safety: the plugin builds every window invisible (OW_TAURI_LAB_INVISIBLE=1:
// hidden, then alpha 0, click-through, not focusable, then shown). This
// script only ever calls showInactive(), never show() or focus(), and never
// sends input to a page.

import { app, BrowserWindow } from 'ow-tauri/electron';

const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args);

// --- 1. Configuration --------------------------------------------------------
const config = await invoke('harness_config');
const t0 = config.startedWall;

/** Append one JSON line to `<runDir>/<file>`. */
function record(file, entry) {
  const line = JSON.stringify({ t: Date.now() - t0, ...entry });
  invoke('harness_record', { file, line }).catch(() => {});
}

/** Write a pretty JSON file into the run directory. */
function writeJson(file, value) {
  invoke('harness_write', { file, text: JSON.stringify(value, null, 2) + '\n' }).catch(() => {});
}

/** JSON-safe copy of an arbitrary value (functions and cycles described). */
function safe(value, depth = 0, seen = new WeakSet()) {
  if (value === null || value === undefined) return value ?? null;
  const type = typeof value;
  if (type === 'string' || type === 'number' || type === 'boolean') return value;
  if (type === 'bigint') return { bigint: String(value) };
  if (type === 'function') return { function: value.name || '(anonymous)', length: value.length };
  if (type !== 'object') return { [type]: String(value) };
  if (seen.has(value)) return '[cycle]';
  if (depth > 6) return '[depth]';
  seen.add(value);
  if (Array.isArray(value)) return value.map((v) => safe(v, depth + 1, seen));
  if (value instanceof Uint8Array) return { buffer: btoa(String.fromCharCode(...value)) };
  const out = {};
  for (const key of Object.keys(value)) out[key] = safe(value[key], depth + 1, seen);
  return out;
}

function log(message, extra) {
  record('events.jsonl', { kind: 'log', message, ...(extra ? { extra: safe(extra) } : {}) });
}

window.addEventListener('error', (event) =>
  record('events.jsonl', {
    kind: 'uncaught',
    message: String(event.error?.stack ?? event.message),
  }),
);
window.addEventListener('unhandledrejection', (event) =>
  record('events.jsonl', {
    kind: 'uncaught',
    message: String(event.reason?.stack ?? event.reason),
  }),
);

// --- 2. app.overwolf and package manager ------------------------------------
const env = globalThis.process?.env ?? {};

function describeOverwolf(label) {
  const ow = app.overwolf;
  const snapshot = {
    label,
    present: Boolean(ow),
    env: { OVERWOLF_APP_UID: env.OVERWOLF_APP_UID ?? null },
  };
  if (!ow) return snapshot;
  const names = new Set();
  for (let o = ow; o && o !== Object.prototype; o = Object.getPrototypeOf(o)) {
    for (const name of Object.getOwnPropertyNames(o)) names.add(name);
  }
  snapshot.members = {};
  for (const name of [...names].sort()) {
    if (name === 'constructor') continue;
    let value;
    try {
      value = ow[name];
    } catch (error) {
      snapshot.members[name] = { threw: String(error) };
      continue;
    }
    snapshot.members[name] =
      typeof value === 'function'
        ? { type: 'function', length: value.length }
        : name === 'packages'
          ? { type: 'object' }
          : { type: typeof value, value: safe(value) };
  }
  const pkgs = ow.packages;
  if (pkgs) {
    snapshot.packages = {};
    const pkgNames = new Set();
    for (let o = pkgs; o && o !== Object.prototype; o = Object.getPrototypeOf(o)) {
      for (const name of Object.getOwnPropertyNames(o)) pkgNames.add(name);
    }
    for (const name of [...pkgNames].sort()) {
      if (name === 'constructor' || name.startsWith('_')) continue;
      let value;
      try {
        value = pkgs[name];
      } catch (error) {
        snapshot.packages[name] = { threw: String(error) };
        continue;
      }
      snapshot.packages[name] =
        typeof value === 'function'
          ? { type: 'function', length: value.length }
          : value !== null && typeof value === 'object'
            ? { type: 'object' }
            : { type: typeof value, value };
    }
  }
  return snapshot;
}

function hookPackageManager() {
  const pkgs = app.overwolf && app.overwolf.packages;
  if (!pkgs || typeof pkgs.emit !== 'function') return;
  const originalEmit = pkgs.emit;
  pkgs.emit = function emitHook(name, ...args) {
    record('packages.jsonl', { event: String(name), args: safe(args) });
    return originalEmit.call(this, name, ...args);
  };
}

const overwolfSnapshots = [];
const overwolfCalls = [];
let lastSnapshot = null;

function paths() {
  return Object.fromEntries(
    ['home', 'appData', 'userData', 'sessionData', 'logs', 'temp'].map((p) => {
      try {
        return [p, app.getPath(p)];
      } catch (error) {
        return [p, `error: ${error}`];
      }
    }),
  );
}

function snapshotOverwolf(label) {
  const full = describeOverwolf(label);
  if (lastSnapshot === null) {
    overwolfSnapshots.push(full);
  } else {
    const changed = { label, t: Date.now() - t0, changed: {} };
    for (const section of ['env', 'members', 'packages']) {
      for (const [key, value] of Object.entries(full[section] || {})) {
        if (JSON.stringify(value) !== JSON.stringify((lastSnapshot[section] || {})[key])) {
          changed.changed[`${section}.${key}`] = value;
        }
      }
    }
    overwolfSnapshots.push(changed);
  }
  lastSnapshot = full;
  writeJson('overwolf.json', {
    appName: app.getName(),
    appVersion: app.getVersion(),
    versions: safe(globalThis.process?.versions ?? null),
    platform: globalThis.process?.platform ?? null,
    arch: globalThis.process?.arch ?? null,
    argv: safe(globalThis.process?.argv ?? null),
    userAgentFallback: navigator.userAgent,
    paths: paths(),
    snapshots: overwolfSnapshots,
    calls: overwolfCalls,
  });
}

async function callOverwolf(label, fn) {
  const started = Date.now();
  try {
    const result = await fn();
    overwolfCalls.push({
      label,
      ok: true,
      result: safe(result),
      ms: Date.now() - started,
      t: started - t0,
    });
  } catch (error) {
    overwolfCalls.push({
      label,
      ok: false,
      error: String(error),
      ms: Date.now() - started,
      t: started - t0,
    });
  }
}

snapshotOverwolf('module-load');
hookPackageManager();
if (config.disableAnalytics) {
  app.overwolf.disableAnonymousAnalytics();
  record('events.jsonl', { kind: 'disableAnonymousAnalytics', when: 'module-load' });
}

// --- 3. Page reports, live-load cap ------------------------------------------
let liveLoads = 0;
let liveStopped = false;
let mainWindow = null;
const seenGuests = new Set();

/**
 * Logs a live ad load (`guest-load`, `guest-reload`: an ad page load that
 * can request an ad, counted against the cap) or a fill event of one
 * (`event:*`, logged with `fill: true`, not counted).
 */
function countLiveLoad(reason, detail) {
  if (config.mode !== 'live') return;
  const fill = reason.startsWith('event:');
  if (!fill) liveLoads += 1;
  record('live-loads.jsonl', {
    n: liveLoads,
    ...(fill ? { fill } : {}),
    reason,
    detail: safe(detail),
    at: new Date().toISOString(),
  });
  // The cap lets N loads run; a load beyond N is removed as it starts
  // (it is logged and counts against the run's budget).
  if (liveLoads > config.maxLiveLoads && !liveStopped) {
    liveStopped = true;
    record('events.jsonl', { kind: 'live-cap-reached', liveLoads });
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents
        .executeJavaScript(
          `document.querySelectorAll('owadview').forEach((el) => el.remove()); 'removed'`,
        )
        .catch(() => {});
    }
  }
}

function handlePageEvent(evt) {
  if (evt.kind !== 'owadview-event') return;
  // Every ad page load can request an ad: the element's first dom-ready is
  // its guest load, a later one a reload (both count).
  if (evt.event === 'dom-ready') {
    const reload = seenGuests.has(evt.cid);
    seenGuests.add(evt.cid);
    countLiveLoad(reload ? 'guest-reload' : 'guest-load', { cid: evt.cid });
  }
  if (['impression', 'display_ad_loaded'].includes(evt.event)) {
    countLiveLoad(`event:${evt.event}`, { cid: evt.cid });
  }
}

setInterval(async () => {
  let events = [];
  try {
    events = await invoke('harness_take_page_events');
  } catch {
    return;
  }
  for (const evt of events) handlePageEvent(evt);
}, 200);

// --- 4. Actions (scenario.cjs) ----------------------------------------------
const CMP_LABEL = 'ow-cmp';
const cmpLabels = new Map();

const pageEval = (code) => {
  if (!mainWindow || mainWindow.isDestroyed()) return Promise.resolve(null);
  return mainWindow.webContents.executeJavaScript(code);
};

function describeWindow(win) {
  const get = (fn) => {
    try {
      return fn();
    } catch {
      return null;
    }
  };
  return {
    windowId: win.id,
    title: get(() => win.getTitle()),
    bounds: get(() => win.getBounds()),
    contentBounds: get(() => win.getContentBounds()),
    visible: get(() => win.isVisible()),
    resizable: get(() => win.isResizable()),
    minimizable: get(() => win.isMinimizable()),
    maximizable: get(() => win.isMaximizable()),
    closable: get(() => win.isClosable()),
    alwaysOnTop: get(() => win.isAlwaysOnTop()),
    backgroundColor: get(() => win.getBackgroundColor()),
    minSize: get(() => win.getMinimumSize()),
    maxSize: get(() => win.getMaximumSize()),
    url: get(() => win.webContents.getURL()),
    listeners: get(() => win.eventNames().map((n) => [String(n), win.listenerCount(n)])),
  };
}

/** A guest-eval result: the JSON text the expression returned, parsed. */
function parseResult(result) {
  if (typeof result !== 'string') return safe(result);
  try {
    return JSON.parse(result);
  } catch {
    return result;
  }
}

// ../../app/scenario.cjs GUEST_FRAME_HOOK, with the guest's console replaced
// by a queue the harness drains (Tauri cannot read a guest's console).
const GUEST_FRAME_HOOK = `JSON.stringify((() => {
  const top = window;
  top.__parityFrameLog = top.__parityFrameLog || [];
  const summarize = (d) => { try { return typeof d === 'string' ? d.slice(0, 1500) : JSON.stringify(d).slice(0, 1500); } catch (e) { return String(d).slice(0, 200); } };
  const hook = (w, path) => {
    try {
      if (w.__parityFrameHooked) return 0;
      w.__parityFrameHooked = true;
      w.addEventListener('message', (e) => {
        try { if (top.__parityFrameLog.length < 5000) top.__parityFrameLog.push('__PARITYF__' + JSON.stringify({ path, href: w.location.href.slice(0, 200), origin: e.origin, fromParent: e.source === w.parent, data: summarize(e.data) })); } catch (err) {}
      }, true);
      return 1;
    } catch (e) { return 0; }
  };
  const walk = (w, path) => {
    let n = 0;
    for (let i = 0; i < w.frames.length; i++) {
      const f = w.frames[i];
      try { void f.location.href; } catch (e) { continue; }
      n += hook(f, path + '/' + i) + walk(f, path + '/' + i);
    }
    return n;
  };
  if (!top.__parityFrameTimer) top.__parityFrameTimer = setInterval(() => walk(top, ''), 1000);
  return walk(top, '');
})())`;

const FRAME_DRAIN = `JSON.stringify((() => { const l = window.__parityFrameLog || []; window.__parityFrameLog = []; return l; })())`;
let frameDrain = null;

/** Moves the queued frame messages of every guest into console.jsonl, every second. */
function startFrameDrain() {
  if (frameDrain) return;
  frameDrain = setInterval(async () => {
    let results = [];
    try {
      results = await invoke('harness_guest_eval', { code: FRAME_DRAIN });
    } catch {
      return;
    }
    for (const { label, result } of results) {
      const lines = parseResult(result);
      if (!Array.isArray(lines)) continue;
      for (const message of lines) {
        record('console.jsonl', { type: 'owadview', webContentsId: label, level: 0, message });
      }
    }
  }, 1000);
}

// parity-diff.mjs reads these from actions.jsonl (phase action-unsupported).
const unsupported = (name, why) => async (action) => {
  record('actions.jsonl', {
    phase: 'action-unsupported',
    do: name,
    label: action?.label ?? null,
    host: 'tauri',
    why,
  });
};

const actions = {
  async 'ow-call'({ fn, args = [], label, sync, generateFrom }) {
    const ow = app.overwolf;
    if (generateFrom !== undefined) {
      args = [ow.generateUserEmailHashes(generateFrom)];
      label = label ?? `${fn}(generateUserEmailHashes(${JSON.stringify(generateFrom)}))`;
    }
    if (sync) {
      const entry = { kind: 'ow-call-sync', fn, label };
      try {
        const value = ow[fn](...args);
        entry.returned = value && typeof value.then === 'function' ? 'promise' : safe(value);
        if (value && typeof value.then === 'function') {
          value.then(
            (v) => record('events.jsonl', { ...entry, settled: 'resolved', value: safe(v) }),
            (e) => record('events.jsonl', { ...entry, settled: 'rejected', error: String(e) }),
          );
        }
      } catch (error) {
        entry.threw = String(error);
      }
      record('events.jsonl', entry);
      return;
    }
    await callOverwolf(label ?? fn, () => ow[fn](...args));
    snapshotOverwolf(`after ${label ?? fn}`);
  },
  async 'pkg-call'({ fn, args = [], label }) {
    const pkgs = app.overwolf.packages;
    const entry = { kind: 'pkg-call', fn, label };
    try {
      const value = pkgs[fn](...args);
      entry.returned = value && typeof value.then === 'function' ? 'promise' : safe(value);
      if (value && typeof value.then === 'function') {
        try {
          entry.value = safe(await value);
          entry.settled = 'resolved';
        } catch (error) {
          entry.settled = 'rejected';
          entry.error = String(error);
        }
      }
    } catch (error) {
      entry.threw = String(error);
    }
    entry.typeofMembers = Object.fromEntries(
      ['gep', 'overlay', 'recorder', 'utility', 'crn'].map((n) => [n, typeof pkgs[n]]),
    );
    record('events.jsonl', entry);
  },
  async 'page-eval'({ code, label }) {
    const result = await pageEval(code).catch((e) => ({ error: String(e) }));
    record('events.jsonl', { kind: 'page-eval', label, result: safe(result) });
  },
  async window({ method, args = [] }) {
    const win = mainWindow;
    if (!win || win.isDestroyed()) return;
    if (method === 'emit') win.emit(...args);
    else if (method === 'show' || method === 'focus') win.showInactive();
    else win[method](...args);
    record('events.jsonl', {
      kind: 'window-action',
      method,
      args: safe(args),
      state: describeWindow(win),
    });
  },
  'crash-guests': unsupported('crash-guests', 'no API to crash a WKWebView content process'),
  async 'guest-eval'({ code, label }) {
    // Runs in every ad guest's main frame (harness-own, not in ipc.jsonl).
    for (const { label: guest, result } of await invoke('harness_guest_eval', {
      code: `JSON.stringify((() => ${code})())`,
    })) {
      record('events.jsonl', {
        kind: 'guest-eval',
        label,
        webContentsId: guest,
        result: parseResult(result),
      });
    }
  },
  async 'hook-guest-frames'({ label }) {
    // As ../../app/scenario.cjs: a capturing 'message' listener in every
    // same-origin frame of each ad guest. The guests have no IPC, so the
    // messages are queued in the guest and drained into console.jsonl
    // (__PARITYF__ lines, the ow-electron harness's shape) every second.
    for (const { label: guest, result } of await invoke('harness_guest_eval', {
      code: GUEST_FRAME_HOOK,
    })) {
      record('events.jsonl', {
        kind: 'hook-guest-frames',
        label,
        webContentsId: guest,
        result: parseResult(result),
      });
    }
    startFrameDrain();
  },
  async 'hit-probe'({ label, points, click, snapshot }) {
    // Lab checks L1-L3: what the page hits at each point, which native view
    // a click there reaches, each webview's own rendering, and (test mode,
    // only into the app's webview) one click at the named point.
    const dom = await pageEval(`window.__parityHit(${JSON.stringify(points)})`).catch((e) => ({
      error: String(e),
    }));
    const win = `bw-${mainWindow?.id ?? 1}`;
    // The page resolved selector points to CSS px; the native probe uses those.
    const resolved = (dom?.points ?? []).map(({ name, x, y }) => ({ name, x, y }));
    const native = await invoke('harness_native_probe', {
      window: win,
      embedder: win,
      points: resolved,
      snapshot: Boolean(snapshot),
      click: config.mode === 'test' ? (click ?? null) : null,
      // Windows: the window copies are kept in the run as hit-<label>-*.bmp.
      capture: `hit-${String(label).replace(/[^A-Za-z0-9_-]/g, '-')}`,
    }).catch((e) => ({ error: String(e) }));
    record('events.jsonl', { kind: 'hit-probe', label, host: 'tauri', dom, native });
  },
  'cookie-set': unsupported('cookie-set', 'not used by the compared scenarios'),
  async 'probe-guests'({ label }) {
    await invoke('harness_probe_guests', { phase: label });
  },
  introspect: unsupported('introspect', 'Electron internals'),
  listeners: async ({ label }) => {
    const out = { label, app: app.eventNames().map((n) => [String(n), app.listenerCount(n)]) };
    if (mainWindow && !mainWindow.isDestroyed()) {
      out.mainWindow = mainWindow.eventNames().map((n) => [String(n), mainWindow.listenerCount(n)]);
    }
    const pkgs = app.overwolf && app.overwolf.packages;
    if (pkgs && typeof pkgs.eventNames === 'function') {
      out.packages = pkgs.eventNames().map((n) => [String(n), pkgs.listenerCount(n)]);
    }
    record('listeners.jsonl', out);
  },
  async screencapture() {
    // Never: screencapture can raise a system permission prompt.
  },
  async snapshot({ label }) {
    snapshotOverwolf(label);
  },
  'open-window': unsupported('open-window', 'window analytics scenarios are not mirrored yet'),
  'extra-window': unsupported('extra-window', 'window analytics scenarios are not mirrored yet'),
  async 'cmp-open'({ fn, options, label }) {
    const started = Date.now();
    cmpLabels.set(label, CMP_LABEL);
    const entry = { kind: 'cmp-open', fn, label, options: safe(options), t: started - t0 };
    let promise;
    try {
      promise = options === undefined ? app.overwolf[fn]() : app.overwolf[fn](options);
    } catch (error) {
      record('events.jsonl', { ...entry, threw: String(error) });
      return;
    }
    record('events.jsonl', {
      ...entry,
      returned: promise && typeof promise.then === 'function' ? 'promise' : safe(promise),
    });
    Promise.resolve(promise).then(
      (value) =>
        record('events.jsonl', {
          kind: 'cmp-settled',
          label,
          settled: 'resolved',
          value: safe(value),
          afterMs: Date.now() - started,
        }),
      (error) =>
        record('events.jsonl', {
          kind: 'cmp-settled',
          label,
          settled: 'rejected',
          error: String(error),
          afterMs: Date.now() - started,
        }),
    );
    setTimeout(async () => {
      const state = await invoke('harness_window', { label: CMP_LABEL, action: 'state' });
      record('windows.jsonl', { kind: 'cmp-window', label, ...state });
    }, 1500);
  },
  async 'cmp-close'({ label }) {
    const state = await invoke('harness_window', {
      label: cmpLabels.get(label) ?? CMP_LABEL,
      action: 'close',
    });
    record('windows.jsonl', { kind: 'cmp-window-before-close', label, ...state });
  },
  async 'cmp-state'({ label }) {
    const state = await invoke('harness_window', {
      label: cmpLabels.get(label) ?? CMP_LABEL,
      action: 'state',
    });
    record('windows.jsonl', { kind: 'cmp-window-state', label, ...state });
  },
};

function runActions() {
  for (const action of config.actions || []) {
    setTimeout(async () => {
      record('actions.jsonl', { phase: 'start', ...action });
      try {
        await actions[action.do](action);
        record('actions.jsonl', { phase: 'done', do: action.do, label: action.label ?? null });
      } catch (error) {
        record('actions.jsonl', {
          phase: 'error',
          do: action.do,
          error: String(error && error.stack),
        });
      }
    }, action.at);
  }
}

function startTicks() {
  if (!config.tickMs) return;
  setInterval(() => record('ticks.jsonl', { at: new Date().toISOString() }), config.tickMs);
}

// --- 5. App lifecycle --------------------------------------------------------
for (const name of ['ready', 'window-all-closed', 'before-quit', 'will-quit', 'quit']) {
  app.on(name, () => record('events.jsonl', { kind: 'app', event: name }));
}
// A listener keeps the app alive when the ad window closes (close-then-quit).
app.on('window-all-closed', () => {});

async function fullRun() {
  snapshotOverwolf('ready');
  startTicks();
  if (config.calibrate) {
    // ow-electron's harness proves its JS guard runs before a window can
    // show. Here the plugin builds every window invisible natively.
    record('windows.jsonl', {
      kind: 'calibration',
      host: 'tauri',
      handlerPrecedesOptions: null,
      note: 'lab windows are built hidden, then alpha 0, then shown (plugin lab mode)',
    });
  }
  if (config.skipStartupCalls) {
    snapshotOverwolf('after-calls');
    return startWindowAndActions();
  }
  await callOverwolf('isCMPRequired', () => app.overwolf.isCMPRequired());
  await callOverwolf('packages.hasPendingUpdates', () => app.overwolf.packages.hasPendingUpdates());
  if ((config.packages || []).length > 0) {
    await callOverwolf('packages.getChannel', () =>
      app.overwolf.packages.getChannel(...config.packages),
    );
    await callOverwolf('packages.getAvailableChannels', () =>
      app.overwolf.packages.getAvailableChannels(...config.packages),
    );
  }
  snapshotOverwolf('after-calls');
  return startWindowAndActions();
}

async function startWindowAndActions() {
  if (config.noWindow) {
    runActions();
    setTimeout(quitFlow, config.durationMs);
    return;
  }
  const { width, height } = config.window;
  const [x, y] = config.windowPosition ?? [0, 0];
  mainWindow = new BrowserWindow({
    show: false,
    width,
    height,
    x,
    y,
    title: config.windowTitle,
    ...(config.windowName ? { name: config.windowName } : {}),
    skipTaskbar: true,
    focusable: false,
    webPreferences: { contextIsolation: true, nodeIntegration: false, sandbox: true },
  });
  record('windows.jsonl', { kind: 'created', ...describeWindow(mainWindow) });
  mainWindow.on('closed', () => record('windows.jsonl', { kind: 'closed', windowId: 1 }));
  if (config.present === 'transparent') {
    mainWindow.showInactive();
    if (config.windowPosition) {
      mainWindow.setPosition(x, y);
      record('windows.jsonl', {
        kind: 'positioned',
        requested: [x, y],
        bounds: mainWindow.getBounds(),
      });
    }
  }
  const query = new URLSearchParams({
    layouts: config.layouts.length ? config.layouts.join(',') : 'none',
    mode: config.mode,
    ...(config.elementAttrs ? { attrs: JSON.stringify(config.elementAttrs) } : {}),
    ...(config.elementSpec ? { spec: JSON.stringify(config.elementSpec) } : {}),
  });
  await mainWindow.loadFile('index.html', { search: query.toString() });
  runActions();
  for (const at of [30_000, 120_000, 300_000]) {
    if (at < config.durationMs) setTimeout(() => snapshotOverwolf(`t+${at / 1000}s`), at);
  }
  setTimeout(quitFlow, config.durationMs);
}

async function quitFlow() {
  record('events.jsonl', { kind: 'quit-flow-start' });
  snapshotOverwolf('before-quit');
  await invoke('harness_probe_guests', { phase: 'end' }).catch(() => {});
  // Give the probes a moment to come back before the guests go away.
  await new Promise((r) => setTimeout(r, 500));
  if (config.quitStyle === 'quit') {
    app.quit();
    return;
  }
  if (mainWindow && !mainWindow.isDestroyed()) mainWindow.close();
  setTimeout(() => app.quit(), config.closeToQuitMs ?? 2000);
}

await app.whenReady();
await (config.probeOnly
  ? (async () => {
      snapshotOverwolf('ready');
      await new Promise((r) => setTimeout(r, config.probeDelayMs ?? 1500));
      snapshotOverwolf('ready+delay');
      app.exit(0);
    })()
  : fullRun());
