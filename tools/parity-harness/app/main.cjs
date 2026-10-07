// Parity harness: Electron main process run under @overwolf/ow-electron.
//
// Observes, through Electron's public APIs only, what ow-electron does for an
// app that shows <owadview> ads: network traffic of every webContents (Chrome
// DevTools Protocol), optional session.webRequest hooks, app.overwolf values
// and package-manager events, cookies, the ad guest's window.__overwolf__ and
// the postMessages it receives. Everything is written to the run directory
// passed by run.mjs; nothing is sent anywhere.
//
// Safety: no window is ever shown (dock hidden first, every BrowserWindow is
// created hidden and kept hidden), and no input event is ever sent, so no ad
// can be clicked. See ../README.md.

'use strict';

const { app, BrowserWindow, session } = require('electron');
const fs = require('node:fs');
const path = require('node:path');

// --- 0. Hide from the Dock before anything else -----------------------------
if (process.platform === 'darwin' && app.dock) {
  app.dock.hide();
}

// --- 1. Configuration --------------------------------------------------------
const CONFIG_ENV = 'PARITY_HARNESS_CONFIG';
const config = JSON.parse(fs.readFileSync(process.env[CONFIG_ENV], 'utf8'));
const runDir = config.runDir;
const t0 = Date.now();
/** Round-2 instrumentation (scenario.cjs); set once its dependencies exist. */
let scenario = null;

/** Append one JSON line to `<runDir>/<file>`. */
function record(file, entry) {
  const line = JSON.stringify({ t: Date.now() - t0, ...entry });
  fs.appendFileSync(path.join(runDir, file), line + '\n');
}

/** Write a pretty JSON file into the run directory. */
function writeJson(file, value) {
  fs.writeFileSync(path.join(runDir, file), JSON.stringify(value, null, 2) + '\n');
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
  if (Buffer.isBuffer(value)) return { buffer: value.toString('base64') };
  const out = {};
  for (const key of Object.keys(value)) out[key] = safe(value[key], depth + 1, seen);
  return out;
}

function log(message, extra) {
  record('events.jsonl', { kind: 'log', message, ...(extra ? { extra: safe(extra) } : {}) });
}

process.on('uncaughtException', (error) => {
  record('events.jsonl', { kind: 'uncaught', message: String(error && error.stack) });
});

// --- 2. Window safety: nothing ever becomes visible --------------------------
const visibilityMethods = [
  'show',
  'showInactive',
  'focus',
  'moveTop',
  'maximize',
  'setFullScreen',
  'setSimpleFullScreen',
  'setKiosk',
  'restore',
  'flashFrame',
];

const originalShowInactive = BrowserWindow.prototype.showInactive;
const originalSetOpacity = BrowserWindow.prototype.setOpacity;
const originalSetIgnoreMouseEvents = BrowserWindow.prototype.setIgnoreMouseEvents;
const originalSetFocusable = BrowserWindow.prototype.setFocusable;
for (const method of visibilityMethods) {
  const original = BrowserWindow.prototype[method];
  if (typeof original !== 'function') continue;
  BrowserWindow.prototype[method] = function guarded(...args) {
    record('windows.jsonl', {
      kind: 'visibility-call',
      method,
      windowId: this.id,
      present: config.present,
    });
    // Transparent mode: a show request becomes an inactive show of a window
    // that has opacity 0 and ignores the mouse. Every other call is dropped.
    if (config.present === 'transparent' && (method === 'show' || method === 'showInactive')) {
      makeInvisible(this);
      originalShowInactive.call(this);
    }
    return undefined;
  };
}

// Calls that could make a window visible, hit-testable or focusable again are
// pinned: opacity stays 0, the mouse is ignored, the window is not focusable.
const pinned = [
  ['setOpacity', originalSetOpacity, 0],
  ['setIgnoreMouseEvents', originalSetIgnoreMouseEvents, true],
  ['setFocusable', originalSetFocusable, false],
];
for (const [method, original, value] of pinned) {
  BrowserWindow.prototype[method] = function pinnedCall(...args) {
    record('windows.jsonl', { kind: 'pinned-call', method, windowId: this.id, args: safe(args) });
    return original.call(this, value);
  };
}

/** The window exists for the OS but cannot be seen, hit or focused. */
function makeInvisible(win) {
  try {
    originalSetOpacity.call(win, 0);
    originalSetIgnoreMouseEvents.call(win, true);
    originalSetFocusable.call(win, false);
    win.setSkipTaskbar(true);
  } catch (error) {
    log('makeInvisible failed', { error: String(error) });
  }
}

function enforceHidden(win) {
  const info = () => ({
    windowId: win.id,
    title: win.getTitle(),
    bounds: win.getBounds(),
    visible: win.isVisible(),
  });
  record('windows.jsonl', { kind: 'created', ...info() });
  // This handler runs before the constructor applies its options (title,
  // position, opacity, show; see the calibration in scenario.cjs), so even a
  // window built with show:true is shown at opacity 0, then hidden below.
  makeInvisible(win);
  if (config.present === 'hidden' && win.isVisible()) win.hide();
  win.on('show', () => {
    record('windows.jsonl', { kind: 'show-event', ...info() });
    if (config.present === 'hidden') win.hide();
    else makeInvisible(win);
  });
  win.on('ready-to-show', () => record('windows.jsonl', { kind: 'ready-to-show', ...info() }));
  win.on('closed', () => record('windows.jsonl', { kind: 'closed', windowId: win.id }));
  win.webContents.on('did-finish-load', () =>
    record('windows.jsonl', { kind: 'did-finish-load', url: win.webContents.getURL(), ...info() }),
  );
}

app.on('browser-window-created', (_event, win) => enforceHidden(win));

// --- 3. Main-process JS network hooks (passive wrappers) ---------------------
// If ow-electron sends analytics through Electron's JS `net`/`fetch`, these
// wrappers see the request including its body. If it uses native code, the
// Chromium net log (run.mjs passes --log-net-log) is the source of truth.
function hookMainProcessNetwork() {
  const electron = require('electron');
  const net = electron.net;
  if (net && typeof net.request === 'function') {
    const originalRequest = net.request.bind(net);
    net.request = function request(options) {
      if (scenario) {
        if (typeof options === 'string') options = scenario.rewriteUrl(options);
        else if (options && typeof options.url === 'string')
          options = { ...options, url: scenario.rewriteUrl(options.url) };
      }
      const req = originalRequest(options);
      const entry = { api: 'net.request', options: safe(options), headers: {}, body: [] };
      const setHeader = req.setHeader.bind(req);
      req.setHeader = (name, value) => {
        entry.headers[name] = value;
        return setHeader(name, value);
      };
      const write = req.write.bind(req);
      req.write = (chunk, ...rest) => {
        entry.body.push(String(chunk));
        return write(chunk, ...rest);
      };
      const end = req.end.bind(req);
      req.end = (chunk, ...rest) => {
        if (chunk && typeof chunk !== 'function') entry.body.push(String(chunk));
        record('main-js-net.jsonl', entry);
        return end(chunk, ...rest);
      };
      req.on('response', (res) =>
        record('main-js-net.jsonl', {
          api: 'net.request:response',
          options: safe(options),
          status: res.statusCode,
          headers: res.headers,
        }),
      );
      return req;
    };
  }
  const wrapFetch = (owner, key, label) => {
    const original = owner && owner[key];
    if (typeof original !== 'function') return;
    owner[key] = async function fetchHook(input, init) {
      const url = typeof input === 'string' ? input : input && (input.url || String(input));
      let body = init && init.body;
      if (body && typeof body !== 'string') body = '[non-string body]';
      record('main-js-net.jsonl', {
        api: label,
        url,
        method: (init && init.method) || 'GET',
        headers: safe(init && init.headers),
        body: body ?? null,
      });
      const response = await original.call(this, input, init);
      record('main-js-net.jsonl', {
        api: `${label}:response`,
        url,
        status: response.status,
        headers: Object.fromEntries(response.headers.entries()),
      });
      return response;
    };
  };
  wrapFetch(net, 'fetch', 'net.fetch');
  wrapFetch(globalThis, 'fetch', 'globalThis.fetch');
  for (const moduleName of ['http', 'https']) {
    const mod = require(moduleName);
    for (const fn of ['request', 'get']) {
      const original = mod[fn];
      mod[fn] = function nodeRequest(...args) {
        record('main-js-net.jsonl', { api: `${moduleName}.${fn}`, args: safe(args.slice(0, 2)) });
        return original.apply(this, args);
      };
    }
  }
}
hookMainProcessNetwork();

// --- 4. app.overwolf and package manager ------------------------------------
function describeOverwolf(label) {
  const ow = app.overwolf;
  const snapshot = {
    label,
    present: Boolean(ow),
    env: { OVERWOLF_APP_UID: process.env.OVERWOLF_APP_UID ?? null },
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
      // Only the public surface: functions and primitive values. Object-valued
      // members are implementation details and are not recorded.
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
let lastSnapshot = null;
function snapshotOverwolf(label) {
  const full = describeOverwolf(label);
  if (lastSnapshot === null) {
    overwolfSnapshots.push(full);
  } else {
    // Later snapshots keep only what changed since the previous one.
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
    versions: process.versions,
    platform: process.platform,
    arch: process.arch,
    argv: process.argv,
    userAgentFallback: app.userAgentFallback,
    paths: Object.fromEntries(
      ['home', 'appData', 'userData', 'sessionData', 'logs', 'temp'].map((p) => {
        try {
          return [p, app.getPath(p)];
        } catch (error) {
          return [p, `error: ${error}`];
        }
      }),
    ),
    snapshots: overwolfSnapshots,
    calls: overwolfCalls,
  });
}

const overwolfCalls = [];
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

// --- 5. Sessions: cookies and optional webRequest hooks ----------------------
const sessions = new Set();
function trackSession(ses, label) {
  if (sessions.has(ses)) return;
  sessions.add(ses);
  record('events.jsonl', { kind: 'session', label, storagePath: ses.storagePath ?? null });
  ses.cookies.on('changed', (_e, cookie, cause, removed) =>
    record('cookie-changes.jsonl', {
      storagePath: ses.storagePath ?? null,
      cause,
      removed,
      cookie: {
        ...cookie,
        value:
          cookie.value.length > 300
            ? `${cookie.value.slice(0, 300)}…[${cookie.value.length}]`
            : cookie.value,
      },
    }),
  );
  if (!config.webRequest) return;
  // WARNING (README): Electron keeps one listener per webRequest event and
  // session. If ow-electron registers its own listener from JS, these replace
  // it. Compare with a netlog-only run before trusting header observations.
  const wr = ses.webRequest;
  const file = 'webrequest.jsonl';
  wr.onBeforeSendHeaders((details, callback) => {
    record(file, {
      phase: 'onBeforeSendHeaders',
      ...pickDetails(details),
      requestHeaders: details.requestHeaders,
    });
    callback({ requestHeaders: details.requestHeaders });
  });
  wr.onSendHeaders((details) =>
    record(file, {
      phase: 'onSendHeaders',
      ...pickDetails(details),
      requestHeaders: details.requestHeaders,
    }),
  );
  wr.onHeadersReceived((details, callback) => {
    record(file, {
      phase: 'onHeadersReceived',
      ...pickDetails(details),
      statusCode: details.statusCode,
      responseHeaders: details.responseHeaders,
    });
    callback({});
  });
  wr.onCompleted((details) =>
    record(file, {
      phase: 'onCompleted',
      ...pickDetails(details),
      statusCode: details.statusCode,
      fromCache: details.fromCache,
    }),
  );
  wr.onErrorOccurred((details) =>
    record(file, { phase: 'onErrorOccurred', ...pickDetails(details), error: details.error }),
  );
}

function pickDetails(d) {
  return {
    id: d.id,
    url: d.url,
    method: d.method,
    resourceType: d.resourceType,
    webContentsId: d.webContentsId ?? null,
    frameUrl: d.frame ? safeFrameUrl(d.frame) : null,
    referrer: d.referrer,
    uploadData: d.uploadData
      ? d.uploadData.map((part) =>
          part.bytes ? { bytes: part.bytes.toString('utf8') } : safe(part),
        )
      : undefined,
  };
}

function safeFrameUrl(frame) {
  try {
    return frame.url;
  } catch {
    return null;
  }
}

async function dumpCookies(label) {
  const result = [];
  for (const ses of sessions) {
    try {
      const cookies = await ses.cookies.get({});
      result.push({ storagePath: ses.storagePath ?? null, cookies });
    } catch (error) {
      result.push({ storagePath: ses.storagePath ?? null, error: String(error) });
    }
  }
  writeJson(`cookies-${label}.json`, result);
}

app.on('session-created', (ses) => trackSession(ses, 'session-created'));

// --- 6. webContents: CDP network capture, guest inspection -------------------
// Prefix of the harness's own executeJavaScript code (ipc.jsonl skips it).
const OWN_MARKER = '/*parity-harness*/';
const MESSAGE_HOOK = `(() => {
  if (window.__owParityHarnessHooked) return;
  Object.defineProperty(window, '__owParityHarnessHooked', { value: true });
  const emit = window.__owParityHarnessEmit;
  if (typeof emit !== 'function') return;
  const summarize = (data) => {
    try {
      const text = typeof data === 'string' ? data : JSON.stringify(data);
      return text && text.length > 4096 ? text.slice(0, 4096) + '…[' + text.length + ']' : text;
    } catch (e) { return '[unserializable ' + Object.prototype.toString.call(data) + ']'; }
  };
  window.addEventListener('message', (e) => {
    try {
      emit(JSON.stringify({ kind: 'message', href: location.href, origin: e.origin,
        fromParent: e.source === window.parent && window.parent !== window,
        fromSelf: e.source === window, data: summarize(e.data) }));
    } catch (err) {}
  }, true);
  // Page state changes the host can cause (visibility, focus, size).
  const state = (reason) => {
    try {
      const ow = window.__overwolf__;
      emit(JSON.stringify({ kind: 'page-state', reason, href: location.href,
        visibilityState: document.visibilityState, hidden: document.hidden, hasFocus: document.hasFocus(),
        inner: [innerWidth, innerHeight], dpr: devicePixelRatio,
        owWindowFocused: ow ? ow.windowFocused : null, owWindowTitle: ow ? ow.windowTitle : null }));
    } catch (err) {}
  };
  for (const name of ['visibilitychange', 'focus', 'blur', 'resize', 'pagehide', 'pageshow', 'freeze', 'resume']) {
    (name === 'visibilitychange' || name === 'freeze' || name === 'resume' ? document : window)
      .addEventListener(name, () => state(name), true);
  }
  state('hooked');
})();`;

const GUEST_PROBE = `(() => {
  const describe = (v, depth) => {
    if (v === null || v === undefined) return v === null ? null : { undefined: true };
    const t = typeof v;
    if (t === 'function') {
      const src = Function.prototype.toString.call(v);
      return { function: true, length: v.length, native: /\\[native code\\]/.test(src) };
    }
    if (t !== 'object') return v;
    if (depth > 5) return '[depth]';
    if (Array.isArray(v)) return v.map((x) => describe(x, depth + 1));
    const out = {};
    for (const k of Object.getOwnPropertyNames(v)) {
      try { out[k] = describe(v[k], depth + 1); } catch (e) { out[k] = { threw: String(e) }; }
    }
    return out;
  };
  const ow = window.__overwolf__;
  const descriptors = {};
  if (ow) for (const k of Object.getOwnPropertyNames(ow)) {
    const d = Object.getOwnPropertyDescriptor(ow, k);
    descriptors[k] = { writable: d.writable, configurable: d.configurable, enumerable: d.enumerable, accessor: !!(d.get || d.set) };
  }
  const calls = {};
  if (ow) for (const k of ['muid', 'getSystemInformation', 'getCustomTracking']) {
    if (typeof ow[k] === 'function') { try { calls[k] = describe(ow[k](), 0); } catch (e) { calls[k] = { threw: String(e) }; } }
  }
  const top = Object.getOwnPropertyDescriptor(window, '__overwolf__');
  let storage = {};
  try { for (let i = 0; i < localStorage.length; i++) { const k = localStorage.key(i); storage[k] = localStorage.getItem(k); } } catch (e) { storage = { error: String(e) }; }
  return {
    href: location.href, referrer: document.referrer, userAgent: navigator.userAgent,
    visibilityState: document.visibilityState, hasFocus: document.hasFocus(),
    cookie: document.cookie, localStorage: storage,
    gcType: typeof window.gc,
    overwolfPresent: !!ow,
    overwolfWindowDescriptor: top ? { writable: top.writable, configurable: top.configurable, enumerable: top.enumerable, accessor: !!(top.get || top.set) } : null,
    overwolfFrozen: ow ? Object.isFrozen(ow) : null,
    overwolf: describe(ow, 0),
    descriptors, calls,
    innerSize: [innerWidth, innerHeight], devicePixelRatio,
  };
})()`;

// What a consent page sees: the globals the host provides to it.
const CMP_PROBE = `(() => {
  const fnInfo = (o) => {
    if (!o) return null;
    const out = {};
    for (const k of Object.getOwnPropertyNames(o)) {
      const v = o[k];
      out[k] = typeof v === 'function'
        ? { function: true, length: v.length, native: /\\[native code\\]/.test(Function.prototype.toString.call(v)) }
        : typeof v;
    }
    return out;
  };
  let storage = {};
  try { for (let i = 0; i < localStorage.length; i++) { const k = localStorage.key(i); storage[k] = (localStorage.getItem(k) || '').slice(0, 300); } } catch (e) { storage = { error: String(e) }; }
  return { href: location.href, cmp: fnInfo(window.cmp), privacy: fnInfo(window.privacy),
    overwolf: typeof window.overwolf, closeNative: /\\[native code\\]/.test(Function.prototype.toString.call(window.close)),
    cookie: document.cookie.slice(0, 2000), localStorage: storage, userAgent: navigator.userAgent };
})()`;

const contentsInfo = new Map();
let guestCount = 0;

function isAdGuest(wc) {
  const url = wc.getURL();
  return wc.getType() !== 'window' && /overwolf\.com\/.*adview/i.test(url);
}

function attachCdp(wc, label, late) {
  const dbg = wc.debugger;
  try {
    dbg.attach('1.3');
  } catch (error) {
    record('events.jsonl', {
      kind: 'cdp-attach-failed',
      webContentsId: wc.id,
      error: String(error),
    });
    return;
  }
  const file = 'cdp-network.jsonl';
  const wcId = wc.id;
  const send = (method, params = {}, sessionId) =>
    (sessionId
      ? dbg.sendCommand(method, params, sessionId)
      : dbg.sendCommand(method, params)
    ).catch((error) => {
      record('events.jsonl', {
        kind: 'cdp-error',
        webContentsId: wcId,
        method,
        error: String(error),
      });
      return null;
    });
  const bodyWanted = new Map();
  const setupSession = async (sessionId, targetInfo) => {
    await send('Network.enable', { maxPostDataSize: 65536 }, sessionId);
    await send('Runtime.addBinding', { name: '__owParityHarnessEmit' }, sessionId);
    await send('Page.enable', {}, sessionId);
    await send('Page.addScriptToEvaluateOnNewDocument', { source: MESSAGE_HOOK }, sessionId);
    // Late attach (or a child target): hook the document that is already running.
    if (late || sessionId) await send('Runtime.evaluate', { expression: MESSAGE_HOOK }, sessionId);
    await send(
      'Target.setAutoAttach',
      { autoAttach: true, waitForDebuggerOnStart: false, flatten: true },
      sessionId,
    );
    if (sessionId) await send('Runtime.runIfWaitingForDebugger', {}, sessionId);
    record('events.jsonl', {
      kind: 'cdp-session',
      webContentsId: wcId,
      label,
      late: Boolean(late),
      sessionId: sessionId ?? null,
      target: targetInfo ?? null,
    });
  };
  dbg.on('message', (_event, method, params, sessionId) => {
    const base = { webContentsId: wcId, label, sessionId: sessionId || null, method };
    switch (method) {
      case 'Target.attachedToTarget':
        setupSession(params.sessionId, {
          type: params.targetInfo.type,
          url: params.targetInfo.url,
        });
        break;
      case 'Runtime.bindingCalled':
        if (params.name === '__owParityHarnessEmit') {
          let payload;
          try {
            payload = JSON.parse(params.payload);
          } catch {
            payload = { raw: params.payload };
          }
          record('guest-messages.jsonl', {
            webContentsId: wcId,
            sessionId: sessionId || null,
            ...payload,
          });
        }
        break;
      case 'Network.requestWillBeSent': {
        const r = params.request;
        record(file, {
          ...base,
          requestId: params.requestId,
          url: r.url,
          urlFragment: r.urlFragment,
          httpMethod: r.method,
          headers: r.headers,
          postData:
            r.postData ?? (r.postDataEntries ? r.postDataEntries.map((e) => e.bytes) : undefined),
          resourceType: params.type,
          frameId: params.frameId,
          documentURL: params.documentURL,
          initiator: params.initiator && { type: params.initiator.type, url: params.initiator.url },
          referrerPolicy: r.referrerPolicy,
          redirectResponse: params.redirectResponse
            ? { status: params.redirectResponse.status, headers: params.redirectResponse.headers }
            : undefined,
        });
        const host = safeHost(r.url);
        if (host.endsWith('overwolf.com') && ['XHR', 'Fetch', 'Document'].includes(params.type)) {
          bodyWanted.set(params.requestId, sessionId || undefined);
        }
        break;
      }
      case 'Network.requestWillBeSentExtraInfo':
        record(file, {
          ...base,
          requestId: params.requestId,
          headers: params.headers,
          associatedCookies: (params.associatedCookies || []).map((c) => ({
            name: c.cookie.name,
            domain: c.cookie.domain,
            blocked: c.blockedReasons,
          })),
        });
        break;
      case 'Network.responseReceived':
        record(file, {
          ...base,
          requestId: params.requestId,
          url: params.response.url,
          status: params.response.status,
          headers: params.response.headers,
          mimeType: params.response.mimeType,
          protocol: params.response.protocol,
          remoteIPAddress: params.response.remoteIPAddress,
          fromDiskCache: params.response.fromDiskCache,
          resourceType: params.type,
        });
        break;
      case 'Network.responseReceivedExtraInfo':
        record(file, {
          ...base,
          requestId: params.requestId,
          statusCode: params.statusCode,
          headers: params.headers,
        });
        break;
      case 'Network.loadingFailed':
        record(file, {
          ...base,
          requestId: params.requestId,
          errorText: params.errorText,
          blockedReason: params.blockedReason,
        });
        bodyWanted.delete(params.requestId);
        break;
      case 'Network.loadingFinished':
        if (bodyWanted.has(params.requestId)) {
          const sid = bodyWanted.get(params.requestId);
          bodyWanted.delete(params.requestId);
          send('Network.getResponseBody', { requestId: params.requestId }, sid).then((res) => {
            if (!res) return;
            const body = res.base64Encoded ? '[base64]' : res.body;
            record('cdp-bodies.jsonl', {
              webContentsId: wcId,
              requestId: params.requestId,
              body: body && body.length > 65536 ? body.slice(0, 65536) + '…' : body,
            });
          });
        }
        break;
      case 'Page.frameNavigated':
        record('events.jsonl', {
          kind: 'frame-navigated',
          webContentsId: wcId,
          sessionId: sessionId || null,
          url: params.frame.url,
          parentId: params.frame.parentId ?? null,
        });
        break;
      default:
        break;
    }
  });
  dbg.on('detach', (_e, reason) =>
    record('events.jsonl', { kind: 'cdp-detach', webContentsId: wcId, reason }),
  );
  setupSession(undefined);
}

function safeHost(url) {
  try {
    return new URL(url).hostname;
  } catch {
    return '';
  }
}

async function probeGuest(wc, label) {
  try {
    const result = await wc.executeJavaScript(OWN_MARKER + GUEST_PROBE, false);
    const index = contentsInfo.get(wc.id).guestIndex;
    writeJson(`guest-${index}-${label}.json`, result);
    record('events.jsonl', { kind: 'guest-probe', webContentsId: wc.id, label, href: result.href });
  } catch (error) {
    record('events.jsonl', {
      kind: 'guest-probe-failed',
      webContentsId: wc.id,
      label,
      error: String(error),
    });
  }
}

app.on('web-contents-created', (_event, wc) => observeContents(wc, 'web-contents-created'));

// Some webContents (the <owadview> guest among them) may not be announced
// through 'web-contents-created'; a poll picks them up.
function pollContents() {
  for (const wc of require('electron').webContents.getAllWebContents()) {
    if (!contentsInfo.has(wc.id)) {
      observeContents(wc, 'poll');
      // A poll can find the guest after its dom-ready; inspect it right away.
      if (isAdGuest(wc)) onGuestDomReady(wc, contentsInfo.get(wc.id));
    }
  }
}

function onGuestDomReady(wc, info) {
  if (info.guestIndex === undefined) {
    guestCount += 1;
    info.guestIndex = guestCount;
    onGuestLoad(wc);
  } else {
    // A reload of the ad page can request an ad as well.
    countLiveLoad('guest-reload', { webContentsId: wc.id, url: safeUrl(wc) });
  }
  probeGuest(wc, `dom-ready-${info.domReadyCount ?? 0}`);
  info.domReadyCount = (info.domReadyCount ?? 0) + 1;
  setTimeout(() => !wc.isDestroyed() && probeGuest(wc, 'after-10s'), 10_000);
}

function observeContents(wc, via) {
  const info = { id: wc.id, type: wc.getType(), via, created: Date.now() - t0, url: safeUrl(wc) };
  contentsInfo.set(wc.id, info);
  record('webcontents.jsonl', { kind: 'created', ...info });
  trackSession(wc.session, `webContents ${wc.id}`);
  if (config.cdp !== false) {
    // Attaching the DevTools protocol while an <owadview> guest initialises
    // breaks its preload (observed: "sandboxed_renderer.bundle.js script failed
    // to run" and a reload). Guests are attached after their first dom-ready;
    // the net log still records their early requests.
    const isGuest = info.type !== 'window' && info.type !== 'browserView';
    if (!isGuest) attachCdp(wc, info.type, false);
    else if (info.url && !wc.isLoading()) attachCdp(wc, info.type, true);
    else wc.once('dom-ready', () => attachCdp(wc, info.type, true));
  }
  const ev = (name, extra) =>
    record('webcontents.jsonl', {
      kind: name,
      id: wc.id,
      type: wc.getType(),
      url: safeUrl(wc),
      ...extra,
    });
  wc.on('did-start-navigation', (details) => {
    if (details.isMainFrame) ev('did-start-navigation', { navUrl: details.url });
  });
  wc.on('did-navigate', (_e, url, code) =>
    ev('did-navigate', { navUrl: url, httpResponseCode: code }),
  );
  wc.on('dom-ready', () => {
    ev('dom-ready');
    if (isAdGuest(wc)) onGuestDomReady(wc, info);
    else if (/overwolf\.com\/.*\/cmp\//.test(safeUrl(wc) || '')) {
      wc.executeJavaScript(OWN_MARKER + CMP_PROBE, false)
        .then((result) =>
          record('cmp-pages.jsonl', { webContentsId: wc.id, type: wc.getType(), ...result }),
        )
        .catch((error) =>
          record('events.jsonl', { kind: 'cmp-probe-failed', error: String(error) }),
        );
    }
  });
  wc.on('did-finish-load', () => ev('did-finish-load'));
  wc.on('did-fail-load', (_e, code, desc, url, isMainFrame) =>
    ev('did-fail-load', { code, desc, failUrl: url, isMainFrame }),
  );
  wc.on('render-process-gone', (_e, details) => ev('render-process-gone', details));
  wc.on('did-attach-webview', (_e, guest) => ev('did-attach-webview', { guestId: guest.id }));
  wc.on('did-create-window', (win, details) =>
    ev('did-create-window', { windowId: win.id, details: safe(details) }),
  );
  wc.on('destroyed', () => record('webcontents.jsonl', { kind: 'destroyed', id: info.id }));
  wc.on('console-message', (...args) => {
    // Electron 42 passes (event) with fields; older versions (event, level, message, line, sourceId).
    const e = args[0];
    const message = typeof e === 'object' && e && 'message' in e ? e.message : args[2];
    const level = typeof e === 'object' && e && 'level' in e ? e.level : args[1];
    if (typeof message !== 'string') return;
    if (message.startsWith('__PARITY__')) {
      const pageEvent = JSON.parse(message.slice('__PARITY__'.length));
      record('page-events.jsonl', { webContentsId: wc.id, ...pageEvent });
      handlePageEvent(pageEvent);
    } else if (!String(safeUrl(wc)).startsWith('file:')) {
      record('console.jsonl', {
        webContentsId: wc.id,
        type: wc.getType(),
        level,
        message: message.slice(0, 2000),
      });
    }
  });
}

function safeUrl(wc) {
  try {
    return wc.getURL();
  } catch {
    return null;
  }
}

// --- 7. Live-ad guard ---------------------------------------------------------
// In live mode every ad page load (a new guest or a reload) counts as an ad
// load; fill events are logged next to them. N loads run; a load beyond the
// cap removes every <owadview> as it starts.
let liveLoads = 0;
let liveStopped = false;
let mainWindow = null;
let firstGuestAt = null;

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
          `${OWN_MARKER}document.querySelectorAll('owadview').forEach((el) => el.remove()); 'removed'`,
        )
        .catch(() => {});
    }
  }
}

function onGuestLoad(wc) {
  if (firstGuestAt === null) {
    firstGuestAt = Date.now();
    setTimeout(() => dumpCookies('after-first-ad'), 15_000);
  }
  countLiveLoad('guest-load', { webContentsId: wc.id, url: safeUrl(wc) });
}

function handlePageEvent(evt) {
  if (evt.kind === 'owadview-event' && ['impression', 'display_ad_loaded'].includes(evt.event)) {
    countLiveLoad(`event:${evt.event}`, { cid: evt.cid });
  }
}

// --- 7b. Round-2 instrumentation ---------------------------------------------------
config.appEntry = __filename;
scenario = require('./scenario.cjs')({
  config,
  record,
  safe,
  log,
  t0,
  makeInvisible,
  originalShowInactive,
  callOverwolf,
  snapshotOverwolf,
  getMainWindow: () => mainWindow,
  probeGuest,
  contentsInfo,
  ownMarker: OWN_MARKER,
});
const featureServerReady = scenario.startFeatureServer();

// --- 8. Lifecycle ---------------------------------------------------------------
for (const name of [
  'will-finish-launching',
  'ready',
  'window-all-closed',
  'before-quit',
  'will-quit',
  'quit',
]) {
  app.on(name, () => record('events.jsonl', { kind: 'app', event: name }));
}
app.on('window-all-closed', () => {
  // Keep running until the timed quit, like an app with a tray icon would.
});

async function probeOnly() {
  snapshotOverwolf('ready');
  await new Promise((r) => setTimeout(r, config.probeDelayMs ?? 1500));
  snapshotOverwolf('ready+delay');
  app.exit(0);
}

async function fullRun() {
  snapshotOverwolf('ready');
  trackSession(session.defaultSession, 'defaultSession');
  scenario.startTicks();
  if (config.calibrate && !scenario.calibrate()) {
    // The guard could not be shown to act before a constructor show: drop
    // every action that opens a window Overwolf builds.
    config.actions = (config.actions || []).filter((a) => a.do !== 'cmp-open');
    record('events.jsonl', { kind: 'calibration-failed', note: 'cmp-open actions dropped' });
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
  await dumpCookies('startup');
  setInterval(pollContents, 500).unref();
  if (config.noWindow) {
    scenario.runActions();
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
  if (config.present === 'transparent') {
    makeInvisible(mainWindow);
    BrowserWindow.prototype.showInactive.call(mainWindow);
    if (config.windowPosition) {
      // Some window managers move a window on show; put it back and record where it is.
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
  await mainWindow.loadFile(path.join(__dirname, 'index.html'), { search: query.toString() });
  scenario.runActions();

  for (const at of [30_000, 120_000, 300_000]) {
    if (at < config.durationMs) setTimeout(() => snapshotOverwolf(`t+${at / 1000}s`), at);
  }
  setTimeout(quitFlow, config.durationMs);
}

async function quitFlow() {
  record('events.jsonl', { kind: 'quit-flow-start' });
  snapshotOverwolf('before-quit');
  await dumpCookies('end');
  for (const wc of require('electron').webContents.getAllWebContents()) {
    if (contentsInfo.get(wc.id)?.guestIndex !== undefined && !wc.isDestroyed()) {
      await probeGuest(wc, 'end');
    }
  }
  if (config.quitStyle === 'quit') {
    // Quit with the window still open, as a tray app's "Exit" menu would.
    app.quit();
    return;
  }
  if (mainWindow && !mainWindow.isDestroyed()) mainWindow.close();
  setTimeout(() => app.quit(), config.closeToQuitMs ?? 2000);
}

app.whenReady().then(async () => {
  if (process.platform === 'darwin' && typeof app.setActivationPolicy === 'function') {
    app.setActivationPolicy('accessory');
  }
  await featureServerReady;
  return config.probeOnly ? probeOnly() : fullRun();
});
