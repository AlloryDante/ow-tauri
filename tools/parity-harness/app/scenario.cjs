// Parity harness, round-2 instrumentation (loaded by main.cjs).
//
// - IPC observation: every message the host sends to a webContents frame
//   (WebFrameMain send/_sendInternal/postMessage/executeJavaScript) and every
//   IPC message a page sends to the host (the webContents '-ipc-*' events),
//   for ad guests and consent pages alike. Written to ipc.jsonl.
// - A timed action script (config.actions): app.overwolf calls, element and
//   window changes, guest crashes, consent windows, extra windows.
// - A local stand-in for the consent feature flag (config.features), served
//   in-process, so isCMPRequired() can be observed against chosen responses.
// - A window monitor (macOS): an external process that lists this process's
//   windows through CGWindowListCopyWindowInfo, to prove nothing became visible.
//
// Observation only: nothing here changes what ow-electron sends, except the
// feature-flag stand-in, which only answers the one request it is told to.

'use strict';

const { app, BrowserWindow, webContents: webContentsModule } = require('electron');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

/**
 * @param {{config: any, record: Function, safe: Function, log: Function, t0: number,
 *   makeInvisible: Function, originalShowInactive: Function, callOverwolf: Function,
 *   snapshotOverwolf: Function, getMainWindow: () => any, probeGuest: Function,
 *   contentsInfo: Map<number, any>, originals: Record<string, Function>}} ctx
 */
module.exports = function install(ctx) {
  const { config, record, safe, log, t0 } = ctx;
  // The harness's own executeJavaScript code starts with this marker; such
  // calls are not recorded as host->page traffic.
  const OWN = ctx.ownMarker;
  const isOwn = (args) => {
    try {
      return JSON.stringify(args).includes(OWN);
    } catch {
      return false;
    }
  };

  // --- IPC observation --------------------------------------------------------
  const truncate = (value, limit = 20000) => {
    let text;
    try {
      text = JSON.stringify(safe(value));
    } catch {
      text = String(value);
    }
    return text && text.length > limit ? `${text.slice(0, limit)}…[${text.length}]` : text;
  };

  const wcInfo = (wc) => {
    try {
      return { webContentsId: wc.id, type: wc.getType(), url: wc.getURL().slice(0, 200) };
    } catch {
      return { webContentsId: null };
    }
  };

  // config.failGuestLoads: the first N ad guest navigations (loadURL) are
  // pointed at a closed local port, so the ad page load fails with a
  // connection error; shows how ow-electron retries a failed ad page.
  let failedLoads = 0;
  function maybeFailLoad(wc, args) {
    if (!config.failGuestLoads || failedLoads >= config.failGuestLoads) return;
    if (safeGet(() => wc.getType()) !== 'owadview') return;
    failedLoads += 1;
    const original = args[0];
    args[0] = 'https://127.0.0.1:9/monsdk/electron/latest/adview.html';
    record('events.jsonl', {
      kind: 'guest-load-redirected',
      n: failedLoads,
      from: original,
      to: args[0],
    });
  }

  let webContentsWrapped = false;
  let framesWrapped = false;

  function wrapFramePrototype(frame) {
    if (framesWrapped || !frame) return;
    const proto = Object.getPrototypeOf(frame);
    if (!proto) return;
    framesWrapped = true;
    for (const method of ['send', '_send', '_sendInternal', 'postMessage', 'executeJavaScript']) {
      const original = proto[method];
      if (typeof original !== 'function') continue;
      proto[method] = function framePeek(...args) {
        let owner = null;
        try {
          owner = webContentsModule.fromFrame ? webContentsModule.fromFrame(this) : null;
        } catch {
          owner = null;
        }
        if (!isOwn(args)) {
          record('ipc.jsonl', {
            dir: 'host->page',
            via: `frame.${method}`,
            ...(owner ? wcInfo(owner) : {}),
            frameUrl: safeGet(() => this.url.slice(0, 200)),
            isMainFrame: safeGet(() => this.parent === null),
            args: truncate(
              method === 'executeJavaScript' ? [String(args[0]).slice(0, 2000)] : args,
            ),
          });
        }
        return original.apply(this, args);
      };
    }
  }

  function wrapWebContentsPrototype(wc) {
    if (webContentsWrapped) return;
    const proto = Object.getPrototypeOf(wc);
    if (!proto) return;
    webContentsWrapped = true;
    for (const method of [
      'send',
      '_sendInternal',
      'postMessage',
      'sendToFrame',
      'executeJavaScript',
      'executeJavaScriptInIsolatedWorld',
      'insertCSS',
      'setAudioMuted',
      'reload',
      'reloadIgnoringCache',
      'loadURL',
      'forcefullyCrashRenderer',
      'setBackgroundThrottling',
      'setZoomFactor',
      'setVisualZoomLevelLimits',
      'invalidate',
      'focus',
      'setWindowOpenHandler',
    ]) {
      const original = proto[method];
      if (typeof original !== 'function') continue;
      proto[method] = function wcPeek(...args) {
        if (method === 'loadURL') maybeFailLoad(this, args);
        if (method === 'setWindowOpenHandler' && typeof args[0] === 'function')
          args[0] = observeOpenHandler(this, args[0]);
        if (!isOwn(args)) {
          record('ipc.jsonl', {
            dir: 'host->page',
            via: `webContents.${method}`,
            ...wcInfo(this),
            args: truncate(
              ['executeJavaScript', 'executeJavaScriptInIsolatedWorld'].includes(method)
                ? args.map((a) => (typeof a === 'string' ? a.slice(0, 2000) : a))
                : method === 'setWindowOpenHandler'
                  ? ['[handler]']
                  : args,
            ),
          });
        }
        return original.apply(this, args);
      };
    }
    // Page -> host IPC arrives as '-ipc-*' events on the webContents.
    const originalEmit = proto.emit;
    proto.emit = function emitPeek(name, ...args) {
      const isIpc = typeof name === 'string' && name.startsWith('-ipc');
      let entry = null;
      if (isIpc) {
        const [event, ...rest] = args;
        entry = {
          dir: 'page->host',
          via: name,
          ...wcInfo(this),
          frame: safeGet(() => event.senderFrame && event.senderFrame.url.slice(0, 200)),
          args: truncate(rest),
        };
        // Capture what the host answers to invoke/sync messages.
        try {
          if (event && event._replyChannel && typeof event._replyChannel.sendReply === 'function') {
            const reply = event._replyChannel.sendReply.bind(event._replyChannel);
            event._replyChannel.sendReply = (value) => {
              record('ipc.jsonl', {
                dir: 'host->page',
                via: `${name}:reply`,
                ...wcInfo(this),
                channel: rest[1],
                reply: truncate(value),
              });
              return reply(value);
            };
          }
        } catch {
          // reply capture is best effort
        }
      } else if (
        typeof name === 'string' &&
        !['console-message', 'devtools-reload-page'].includes(name) &&
        safeGet(() => this.getType()) !== 'window'
      ) {
        record('wc-events.jsonl', { event: name, ...wcInfo(this) });
      }
      const result = originalEmit.call(this, name, ...args);
      if (config.gestureFixture && NAVIGATION_EVENTS.has(name)) {
        // Every listener (ow-electron's included) has run by now, so the
        // event says whether the host cancelled the navigation.
        const event = args[0];
        record('click-outs.jsonl', {
          kind: name,
          ...wcInfo(this),
          navUrl: safeGet(() => event.url) ?? (typeof args[1] === 'string' ? args[1] : undefined),
          isMainFrame: safeGet(() => event.isMainFrame),
          prevented: Boolean(event && event.defaultPrevented),
        });
      }
      if (entry) {
        const event = args[0];
        if (event && event.returnValue !== undefined)
          entry.returnValue = truncate(event.returnValue);
        record('ipc.jsonl', entry);
      }
      return result;
    };
    log('webContents prototype instrumented');
  }

  // --- Click-out observation (gesture-timing) -------------------------------
  // The window-open handler a host installs on a webContents is wrapped so
  // each call is recorded with what the handler answered. main.cjs replaces
  // shell.openExternal with a recorder when config.stubOpenExternal is set,
  // and the gesture fixture only ever opens URLs of an unregistered scheme,
  // so nothing can reach the system browser either way.
  function observeOpenHandler(wc, handler) {
    const wcId = safeGet(() => wc.id);
    return function observedOpenHandler(details) {
      const entry = {
        kind: 'window-open-handler',
        webContentsId: wcId,
        type: safeGet(() => wc.getType()),
        url: details && details.url,
        frameName: details && details.frameName,
        disposition: details && details.disposition,
        features: details && details.features,
        referrer: details && safeGet(() => details.referrer.url),
        hasPostBody: Boolean(details && details.postBody),
      };
      let result;
      try {
        result = handler.call(this, details);
        entry.returned = safe(result);
      } catch (error) {
        entry.threw = String(error);
        record('click-outs.jsonl', entry);
        throw error;
      }
      record('click-outs.jsonl', entry);
      return result;
    };
  }

  // Page -> host calls that do not surface as webContents '-ipc-*' events
  // still reach main-process JavaScript as an event on some emitter. With
  // config.emitterTrace every such event is recorded (name, emitter class,
  // a short argument summary), except Node's own stream/socket chatter.
  if (config.emitterTrace) {
    const EventEmitter = require('node:events');
    const originalEmit = EventEmitter.prototype.emit;
    const NOISE = new Set([
      'data',
      'end',
      'finish',
      'close',
      'drain',
      'readable',
      'response',
      'error',
      'socket',
      'lookup',
      'connect',
      'prefinish',
      'resume',
      'pause',
      'newListener',
      'removeListener',
      'timeout',
      'free',
      'agentRemove',
      'ready',
      'listening',
      'request',
      'console-message',
      '-console-message',
      'login',
      'redirect',
      'abort',
      'aborted',
      'unpipe',
      'pipe',
      'exit',
    ]);
    let depth = 0;
    EventEmitter.prototype.emit = function tracedEmit(name, ...args) {
      if (depth === 0 && typeof name === 'string' && !NOISE.has(name)) {
        depth += 1;
        try {
          const ctor = (this && this.constructor && this.constructor.name) || typeof this;
          const isWc = safeGet(() => typeof this.getType === 'function');
          if (!isWc || name.startsWith('-')) {
            record('emitter-trace.jsonl', {
              emitter: ctor,
              wc: isWc ? wcInfo(this) : null,
              event: name,
              args: truncate(
                args.map((a) =>
                  a && typeof a === 'object' && 'senderFrame' in a
                    ? { ipcEvent: true, senderId: safeGet(() => a.sender.id) }
                    : a,
                ),
                600,
              ),
            });
          }
        } catch {
          // tracing is best effort
        } finally {
          depth -= 1;
        }
      }
      return originalEmit.call(this, name, ...args);
    };
  }

  // Electron 42 delivers page -> host IPC as '-ipc-*' events on the Session.
  let sessionWrapped = false;
  function wrapSessionPrototype(ses) {
    if (sessionWrapped || !ses) return;
    const proto = Object.getPrototypeOf(ses);
    if (!proto) return;
    sessionWrapped = true;
    const originalEmit = proto.emit;
    proto.emit = function sessionEmitPeek(name, ...args) {
      // Responses to executeJavaScript (the harness's own probes) are skipped.
      if (
        typeof name === 'string' &&
        name.startsWith('-ipc') &&
        !(typeof args[1] === 'string' && args[1].startsWith('RENDERER_WEB_FRAME_METHOD_RESPONSE'))
      ) {
        const [event, ...rest] = args;
        // Electron passes (event, channel, args) here.
        const channel = typeof rest[0] === 'string' ? rest[0] : null;
        const sender = safeGet(() => event.sender);
        const entry = {
          dir: 'page->host',
          via: `session${name}`,
          channel,
          ...(sender ? wcInfo(sender) : {}),
          frameUrl: safeGet(() => event.senderFrame.url.slice(0, 200)),
          args: truncate(channel === null ? rest : rest.slice(1)),
        };
        try {
          if (event && event._replyChannel && typeof event._replyChannel.sendReply === 'function') {
            const reply = event._replyChannel.sendReply.bind(event._replyChannel);
            event._replyChannel.sendReply = (value) => {
              record('ipc.jsonl', {
                dir: 'host->page',
                via: `session${name}:reply`,
                channel,
                ...(sender ? wcInfo(sender) : {}),
                reply: truncate(value),
              });
              return reply(value);
            };
          }
        } catch {
          // reply capture is best effort
        }
        const result = originalEmit.call(this, name, ...args);
        if (event && event.returnValue !== undefined)
          entry.returnValue = truncate(event.returnValue);
        record('ipc.jsonl', entry);
        return result;
      }
      return originalEmit.call(this, name, ...args);
    };
    log('session prototype instrumented');
  }
  app.on('session-created', (ses) => wrapSessionPrototype(ses));

  app.on('web-contents-created', (_e, wc) => {
    wrapSessionPrototype(wc.session);
    wrapWebContentsPrototype(wc);
    try {
      wrapFramePrototype(wc.mainFrame);
    } catch {
      wc.once('did-start-navigation', () => wrapFramePrototype(wc.mainFrame));
    }
  });

  /** executeJavaScript that ipc.jsonl does not record (the harness's own probes). */
  function ownExec(wc, code) {
    return wc.executeJavaScript(`${OWN}${code}`, false);
  }

  // --- Feature-flag stand-in ---------------------------------------------------
  // config.features = { match: 'experiments/cmp-eu-only', responses: [{status, body, delayMs}] }
  // Each matching host request takes the next response (the last one repeats).
  let featurePort = null;
  let featureHits = 0;
  function startFeatureServer() {
    return Promise.all([startFeatureStandIn(), startFixtureServer()]);
  }
  function startFeatureStandIn() {
    if (!config.features) return Promise.resolve();
    const responses = config.features.responses;
    const server = http.createServer((req, res) => {
      const response = responses[Math.min(featureHits, responses.length - 1)];
      featureHits += 1;
      record('features.jsonl', {
        kind: 'served',
        n: featureHits,
        url: req.url,
        headers: req.rawHeaders,
        response,
      });
      const finish = () => {
        if (response.status === 'drop') {
          req.socket.destroy();
          return;
        }
        res.writeHead(response.status ?? 200, { 'content-type': 'application/json' });
        res.end(response.body ?? '');
      };
      if (response.delayMs) setTimeout(finish, response.delayMs);
      else finish();
    });
    return new Promise((resolve) => {
      server.listen(0, '127.0.0.1', () => {
        featurePort = server.address().port;
        server.unref();
        log('feature stand-in listening', { port: featurePort });
        resolve();
      });
    });
  }

  // --- Gesture fixture (loopback page, never an ad) ---------------------------
  // config.gestureFixture: a local page the gesture-timing scenario loads into
  // an ad guest in place of the ad page. It opens URLs of an unregistered
  // scheme only (`owparity-canary://`), so even an open that reached the OS
  // would find no application to open it.
  let fixtureOrigin = null;
  function startFixtureServer() {
    if (!config.gestureFixture) return Promise.resolve();
    const server = http.createServer((req, res) => {
      record('fixture.jsonl', { kind: 'served', url: req.url, headers: req.rawHeaders });
      if (req.url.startsWith('/landing')) {
        res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
        res.end('<!doctype html><meta charset="utf-8"><title>landing</title>landing\n');
        return;
      }
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      res.end(GESTURE_FIXTURE);
    });
    return new Promise((resolve) => {
      server.listen(0, '127.0.0.1', () => {
        fixtureOrigin = `http://127.0.0.1:${server.address().port}`;
        server.unref();
        log('gesture fixture listening', { origin: fixtureOrigin });
        resolve();
      });
    });
  }

  /** The ad guest the gesture fixture was loaded into (by webContents id). */
  let fixtureGuestId = null;
  const fixtureGuest = () => {
    if (fixtureGuestId === null) return null;
    const wc = webContentsModule.fromId(fixtureGuestId);
    return wc && !wc.isDestroyed() ? wc : null;
  };
  /** True only while the guest shows the loopback fixture (input may be sent). */
  const onFixture = (wc) =>
    Boolean(wc && fixtureOrigin && safeGet(() => wc.getURL().startsWith(`${fixtureOrigin}/`)));

  /** Rewrites a matching host request URL to the stand-in. Called from main.cjs's net hook. */
  function rewriteUrl(url) {
    if (!config.features || featurePort === null || typeof url !== 'string') return url;
    if (!url.includes(config.features.match)) return url;
    const local = `http://127.0.0.1:${featurePort}/${config.features.match}`;
    record('features.jsonl', { kind: 'rewrite', from: url, to: local });
    return local;
  }

  // The window monitor (lib/window-monitor.swift) is started by run.mjs.

  function screencapture(label) {
    // Opt-in (--screencapture): on recent macOS a capture from a background
    // tool can raise a system permission prompt.
    if (process.platform !== 'darwin' || !config.screencapture) return;
    const file = path.join(config.runDir, `screen-${label}.png`);
    const child = spawn('/usr/sbin/screencapture', ['-x', '-m', '-t', 'png', file], {
      stdio: 'ignore',
    });
    child.on('exit', (code) =>
      record('events.jsonl', { kind: 'screencapture', label, file, code }),
    );
  }

  // --- Windows created by the harness or by ow-electron -------------------------
  const extraWindows = new Map();
  const createdWindows = [];
  // Windows created within 2 s of a cmp-open action belong to that action.
  let pendingCmp = null;
  app.on('browser-window-created', (_e, win) => {
    createdWindows.push(win);
    if (pendingCmp && Date.now() - pendingCmp.at < 2000) adoptCmpWindow(win, pendingCmp.label);
    // Options are applied after this event (title, position, opacity, show).
    // Record the state once the constructor has returned.
    setImmediate(() => {
      if (win.isDestroyed()) return;
      record('windows.jsonl', { kind: 'post-ctor', ...describeWindow(win) });
    });
  });

  function describeWindow(win) {
    const parent = safeGet(() => win.getParentWindow());
    return {
      windowId: win.id,
      title: safeGet(() => win.getTitle()),
      bounds: safeGet(() => win.getBounds()),
      contentBounds: safeGet(() => win.getContentBounds()),
      visible: safeGet(() => win.isVisible()),
      opacity: safeGet(() => win.getOpacity()),
      modal: safeGet(() => win.isModal()),
      parentId: parent ? parent.id : null,
      resizable: safeGet(() => win.isResizable()),
      minimizable: safeGet(() => win.isMinimizable()),
      maximizable: safeGet(() => win.isMaximizable()),
      closable: safeGet(() => win.isClosable()),
      alwaysOnTop: safeGet(() => win.isAlwaysOnTop()),
      skipTaskbar: undefined,
      backgroundColor: safeGet(() => win.getBackgroundColor()),
      minSize: safeGet(() => win.getMinimumSize()),
      maxSize: safeGet(() => win.getMaximumSize()),
      menuBarVisible: safeGet(() => win.isMenuBarVisible()),
      url: safeGet(() => win.webContents.getURL()),
      webContentsId: safeGet(() => win.webContents.id),
      listeners: safeGet(() => win.eventNames().map((n) => [String(n), win.listenerCount(n)])),
    };
  }

  /** `<stateDir>/ow-electron.json` now: existence, size, hash, text and top-level keys. */
  function readStateFile() {
    if (!config.stateDir) return { error: 'no stateDir in config' };
    const file = path.join(config.stateDir, 'ow-electron.json');
    let siblings = null;
    try {
      siblings = fs.readdirSync(config.stateDir).sort();
    } catch {
      siblings = null;
    }
    if (!fs.existsSync(file)) return { exists: false, siblings };
    const bytes = fs.readFileSync(file);
    const text = bytes.toString('utf8');
    let keys = null;
    let parseError = null;
    try {
      const parsed = JSON.parse(text);
      keys = parsed && typeof parsed === 'object' ? Object.keys(parsed) : typeof parsed;
    } catch (error) {
      parseError = String(error);
    }
    return {
      exists: true,
      size: bytes.length,
      sha256: require('node:crypto').createHash('sha256').update(bytes).digest('hex'),
      keys,
      parseError,
      text: text.length > 20000 ? `${text.slice(0, 20000)}…[${text.length}]` : text,
      siblings,
    };
  }

  /**
   * Calibration: proves that 'browser-window-created' runs before the
   * constructor applies options (opacity, position, show). A hidden window is
   * built with opacity 0.5 at an off-screen position; the handler must see
   * the defaults (opacity 1) for the guard to be effective before a show.
   */
  function calibrate() {
    let seenInHandler = null;
    const handler = (_e, win) => {
      seenInHandler = { opacity: win.getOpacity(), bounds: win.getBounds(), title: win.getTitle() };
    };
    app.prependListener('browser-window-created', handler);
    const win = new BrowserWindow({
      show: false,
      opacity: 0.5,
      x: -20000,
      y: -20000,
      width: 120,
      height: 80,
      title: 'calibration',
      focusable: false,
      skipTaskbar: true,
    });
    app.removeListener('browser-window-created', handler);
    const after = { opacity: win.getOpacity(), bounds: win.getBounds(), title: win.getTitle() };
    win.destroy();
    const result = {
      kind: 'calibration',
      inHandler: seenInHandler,
      afterCtor: after,
      handlerPrecedesOptions:
        Boolean(seenInHandler) && seenInHandler.opacity !== 0.5 && after.opacity === 0.5,
    };
    record('windows.jsonl', result);
    return result.handlerPrecedesOptions;
  }

  // --- Action script ------------------------------------------------------------
  const pageEval = (code) => {
    const win = ctx.getMainWindow();
    if (!win || win.isDestroyed()) return Promise.resolve(null);
    return ownExec(win.webContents, code);
  };

  const guests = () =>
    webContentsModule
      .getAllWebContents()
      .filter((wc) => !wc.isDestroyed() && wc.getType() !== 'window' && /adview/.test(wc.getURL()));

  async function probeAllGuests(label) {
    for (const wc of guests()) {
      const info = ctx.contentsInfo.get(wc.id);
      if (info && info.guestIndex !== undefined) await ctx.probeGuest(wc, label);
    }
  }

  function snapshotListeners(label) {
    const out = { label, app: app.eventNames().map((n) => [String(n), app.listenerCount(n)]) };
    const win = ctx.getMainWindow();
    if (win && !win.isDestroyed()) {
      out.mainWindow = win.eventNames().map((n) => [String(n), win.listenerCount(n)]);
      out.mainWebContents = win.webContents
        .eventNames()
        .map((n) => [String(n), win.webContents.listenerCount(n)]);
    }
    out.guests = guests().map((wc) => ({
      webContentsId: wc.id,
      events: wc.eventNames().map((n) => [String(n), wc.listenerCount(n)]),
    }));
    const pkgs = app.overwolf && app.overwolf.packages;
    if (pkgs && typeof pkgs.eventNames === 'function') {
      out.packages = pkgs.eventNames().map((n) => [String(n), pkgs.listenerCount(n)]);
    }
    record('listeners.jsonl', out);
  }

  const cmpWindows = [];
  function adoptCmpWindow(win, label) {
    if (cmpWindows.includes(win)) return;
    cmpWindows.push(win);
    win.__parityLabel = label;
    setImmediate(() => {
      if (!win.isDestroyed())
        record('windows.jsonl', { kind: 'cmp-window', label, ...describeWindow(win) });
    });
    win.webContents.on('did-finish-load', () =>
      record('windows.jsonl', { kind: 'cmp-window-loaded', label, ...describeWindow(win) }),
    );
    win.on('closed', () =>
      record('windows.jsonl', { kind: 'cmp-window-closed', label, windowId: win.id }),
    );
    // Views inside the window (the consent page itself may live in a child view).
    setTimeout(() => {
      if (win.isDestroyed()) return;
      const views = safeGet(() => win.contentView.children) || [];
      record('windows.jsonl', {
        kind: 'cmp-window-views',
        label,
        windowId: win.id,
        views: views.map((v) => ({
          bounds: safeGet(() => v.getBounds()),
          webContentsId: safeGet(() => v.webContents.id),
          url: safeGet(() => v.webContents.getURL()),
        })),
      });
    }, 1500);
  }

  const actions = {
    async 'ow-call'({ fn, args = [], label, sync, generateFrom }) {
      const ow = app.overwolf;
      // JSON has no `undefined`: { $undefined: true } stands for it, so a
      // scenario can pass `undefined` explicitly (setUserEmailHashes(undefined)).
      args = args.map((a) =>
        a && typeof a === 'object' && !Array.isArray(a) && a.$undefined === true ? undefined : a,
      );
      if (generateFrom !== undefined) {
        // setUserEmailHashes(generateUserEmailHashes(<email>)), as the docs show.
        args = [ow.generateUserEmailHashes(generateFrom)];
        label = label ?? `${fn}(generateUserEmailHashes(${JSON.stringify(generateFrom)}))`;
      }
      if (sync) {
        // Call without awaiting inside try, to tell a sync throw from a rejection.
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
      await ctx.callOverwolf(label ?? fn, () => ow[fn](...args));
      ctx.snapshotOverwolf(`after ${label ?? fn}`);
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
      const win = ctx.getMainWindow();
      if (!win || win.isDestroyed()) return;
      if (method === 'showInactive') {
        ctx.makeInvisible(win);
        ctx.originalShowInactive.call(win);
      } else if (method === 'emit') {
        win.emit(...args);
      } else {
        win[method](...args);
      }
      record('events.jsonl', {
        kind: 'window-action',
        method,
        args: safe(args),
        state: describeWindow(win),
      });
    },
    async 'heartbeat-pause'({ ms }) {
      // heartbeat-silence (§5.2 #11): ow-electron has no guest heartbeat to
      // pause; its run is the control.
      record('events.jsonl', { kind: 'heartbeat-pause', ms, control: true });
    },
    async 'crash-guests'({ which = 'all' }) {
      const list = guests();
      for (const wc of which === 'first' ? list.slice(0, 1) : list) {
        record('events.jsonl', { kind: 'crash-guest', webContentsId: wc.id, url: wc.getURL() });
        wc.forcefullyCrashRenderer();
      }
    },
    async 'cookie-set'({ details }) {
      const { session } = require('electron');
      await session.defaultSession.cookies
        .set(details)
        .catch((e) => record('events.jsonl', { kind: 'cookie-set-failed', error: String(e) }));
      record('events.jsonl', { kind: 'cookie-set', name: details.name });
    },
    async 'guest-eval'({ code, label }) {
      // Runs in every ad guest's main frame (harness-own, not in ipc.jsonl).
      for (const wc of guests()) {
        const result = await ownExec(wc, code).catch((e) => ({ error: String(e) }));
        record('events.jsonl', {
          kind: 'guest-eval',
          label,
          webContentsId: wc.id,
          result: safe(result),
        });
      }
    },
    async 'hook-guest-frames'({ label }) {
      // Listens for postMessages arriving in the guest's same-origin child
      // frames (the ad library frame) and logs them to the guest console
      // with a __PARITYF__ prefix (console.jsonl). Observation only.
      for (const wc of guests()) {
        const result = await ownExec(wc, GUEST_FRAME_HOOK).catch((e) => ({ error: String(e) }));
        record('events.jsonl', {
          kind: 'hook-guest-frames',
          label,
          webContentsId: wc.id,
          result: safe(result),
        });
      }
    },
    async 'hit-probe'({ label, points, click, snapshot }) {
      // Lab checks L1-L3: what the page hits at each point (on ow-electron the
      // guest is part of the page, so the DOM hit is the input routing), one
      // test-mode click into the app's own control (never into an ad), and a
      // capture of the composited window and of each guest.
      const win = ctx.getMainWindow();
      const dom = await pageEval(`window.__parityHit(${JSON.stringify(points)})`).catch((e) => ({
        error: String(e),
      }));
      const out = { kind: 'hit-probe', label, host: 'electron', dom: safe(dom) };
      // L5: what each ad guest's audio is set to right now.
      out.guestMuted = guests().map((wc) => wc.isAudioMuted());
      if (click) {
        // The page resolved selector points to CSS px.
        const hit = dom?.points?.find((p) => p.name === click);
        const point = hit && hit.x >= 0 ? hit : null;
        if (config.mode !== 'test') out.click = { sent: false, refused: 'not in test mode' };
        else if (!point || !win || win.isDestroyed())
          out.click = { sent: false, refused: 'no such point' };
        else if (hit?.target?.kind !== 'app')
          out.click = {
            name: click,
            sent: false,
            refused: 'the click would not reach an app control',
            target: hit?.target ?? null,
          };
        else {
          const at = { x: Math.round(point.x), y: Math.round(point.y), button: 'left' };
          win.webContents.sendInputEvent({ type: 'mouseDown', clickCount: 1, ...at });
          win.webContents.sendInputEvent({ type: 'mouseUp', clickCount: 1, ...at });
          out.click = { name: click, sent: true, target: hit.target };
        }
      }
      if (snapshot && win && !win.isDestroyed()) {
        out.snapshots = {};
        const shot = async (key, wc, pts, cssWidth) => {
          try {
            out.snapshots[key] = sampleImage(await wc.capturePage(), pts, cssWidth);
          } catch (e) {
            out.snapshots[key] = { error: String(e) };
          }
        };
        await shot('embedder', win.webContents, dom?.points ?? [], win.getContentBounds().width);
        for (const wc of guests()) await shot(`guest-${wc.id}`, wc, []);
      }
      record('events.jsonl', out);
    },
    async 'probe-guests'({ label }) {
      await probeAllGuests(label);
    },
    async 'state-file'({ label }) {
      // ow-electron's state file as it is right now (state-file.jsonl).
      record('state-file.jsonl', { label, ...readStateFile() });
    },
    async 'guest-fixture'({ label }) {
      // gesture-timing: the first ad guest leaves the ad page for the
      // loopback fixture. From here on no ad is shown in that guest.
      const wc = guests()[0];
      const entry = { kind: 'guest-fixture', label, origin: fixtureOrigin };
      if (!wc || !fixtureOrigin) {
        record('click-outs.jsonl', { ...entry, refused: 'no ad guest or no fixture server' });
        return;
      }
      fixtureGuestId = wc.id;
      entry.webContentsId = wc.id;
      entry.from = safeGet(() => wc.getURL());
      await wc.loadURL(`${fixtureOrigin}/fixture`).catch((e) => {
        entry.loadError = String(e);
      });
      entry.url = safeGet(() => wc.getURL());
      entry.ready = await ownExec(wc, 'typeof window.__gc').catch((e) => String(e));
      record('click-outs.jsonl', entry);
    },
    async 'gesture-case'({ id, kind, delay = 0, input = 'mouse', readAfterMs }) {
      // One click (or key press, or none) in the fixture, then the fixture's
      // own action after `delay` ms. Input is only ever sent to the loopback
      // fixture page, never to an ad.
      const wc = fixtureGuest();
      const entry = { kind: 'gesture-case', id, caseKind: kind, delay, input };
      if (config.mode !== 'test') entry.refused = 'not in test mode';
      else if (!onFixture(wc)) entry.refused = 'the guest does not show the fixture';
      if (entry.refused) {
        record('click-outs.jsonl', entry);
        return;
      }
      const url = `owparity-canary://case-${id}`;
      const armed = await ownExec(
        wc,
        `window.__gc.arm(${JSON.stringify({ id, kind, delay, url })})`,
      ).catch((e) => ({ error: String(e) }));
      entry.armed = safe(armed);
      entry.url = url;
      if (kind === 'no-gesture') {
        entry.fired = await ownExec(wc, 'window.__gc.fire()').catch((e) => String(e));
      } else if (kind === 'embedder-click') {
        // A trusted click in the app's own page (not the guest), then the
        // guest opens 200 ms later from script.
        const main = ctx.getMainWindow();
        if (main && !main.isDestroyed()) {
          const at = { x: 2, y: 2, button: 'left', clickCount: 1 };
          main.webContents.sendInputEvent({ type: 'mouseDown', ...at });
          main.webContents.sendInputEvent({ type: 'mouseUp', ...at });
          entry.embedderClick = at;
        }
        await new Promise((r) => setTimeout(r, 200));
        entry.fired = await ownExec(wc, 'window.__gc.fire()').catch((e) => String(e));
      } else if (input === 'key') {
        entry.focused = await ownExec(wc, 'window.__gc.focusTarget()').catch((e) => String(e));
        wc.sendInputEvent({ type: 'keyDown', keyCode: 'Return' });
        wc.sendInputEvent({ type: 'char', keyCode: 'Return' });
        wc.sendInputEvent({ type: 'keyUp', keyCode: 'Return' });
      } else if (armed && armed.point) {
        const at = { ...armed.point, button: 'left', clickCount: 1 };
        wc.sendInputEvent({ type: 'mouseDown', ...at });
        wc.sendInputEvent({ type: 'mouseUp', ...at });
        entry.click = at;
      }
      record('click-outs.jsonl', entry);
      setTimeout(
        async () => {
          const live = fixtureGuest();
          const log = onFixture(live)
            ? await ownExec(live, 'window.__gc.log.splice(0)').catch((e) => ({ error: String(e) }))
            : { error: 'the guest left the fixture', url: safeGet(() => live.getURL()) };
          record('click-outs.jsonl', { kind: 'fixture-log', id, log: safe(log) });
        },
        readAfterMs ?? delay + 1500,
      );
    },
    async introspect({ label }) {
      // Names only: which own members and IPC listeners exist on the guests.
      const { ipcMain } = require('electron');
      const names = (o) =>
        o && typeof o.eventNames === 'function' ? o.eventNames().map(String) : null;
      record('introspect.jsonl', {
        label,
        ipcMain: names(ipcMain),
        guests: guests().map((wc) => ({
          webContentsId: wc.id,
          ownNames: Object.getOwnPropertyNames(wc),
          ownEmit: Object.prototype.hasOwnProperty.call(wc, 'emit'),
          ipc: names(wc.ipc),
          frameIpc: names(safeGet(() => wc.mainFrame.ipc)),
          hostWebContentsId: safeGet(() => wc.hostWebContents && wc.hostWebContents.id),
        })),
      });
    },
    async listeners({ label }) {
      snapshotListeners(label);
    },
    async screencapture({ label }) {
      screencapture(label);
    },
    async snapshot({ label }) {
      ctx.snapshotOverwolf(label);
    },
    async 'open-window'({ key, options = {}, file = 'blank.html', url, query, show = true }) {
      const win = new BrowserWindow({
        show: false,
        width: 400,
        height: 300,
        x: 0,
        y: 0,
        skipTaskbar: true,
        focusable: false,
        webPreferences: { contextIsolation: true, sandbox: true },
        ...options,
      });
      extraWindows.set(key, win);
      const target = path.join(path.dirname(config.appEntry), file);
      if (!fs.existsSync(target)) {
        fs.writeFileSync(
          target,
          `<!doctype html><meta charset="utf-8"><title>page ${path.basename(file)}</title><body style="background:#202020"></body>\n`,
        );
      }
      if (url) await win.loadURL(url).catch(() => {});
      else await win.loadFile(target, query ? { search: query } : undefined).catch(() => {});
      if (show) {
        ctx.makeInvisible(win);
        ctx.originalShowInactive.call(win);
      }
      record('events.jsonl', {
        kind: 'open-window',
        key,
        options: safe(options),
        file: url ? null : file,
        url: url ?? null,
        state: describeWindow(win),
      });
    },
    async 'extra-window'({ key, method, args = [] }) {
      const win = extraWindows.get(key);
      if (!win || win.isDestroyed()) return;
      if (method === 'showInactive') {
        ctx.makeInvisible(win);
        ctx.originalShowInactive.call(win);
      } else {
        win[method](...args);
      }
      record('events.jsonl', { kind: 'extra-window', key, method, args: safe(args) });
    },
    async 'cmp-open'({ fn, options, label }) {
      const started = Date.now();
      pendingCmp = { label, at: started };
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
            openWindows: cmpWindows.filter((w) => !w.isDestroyed()).map((w) => w.id),
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
    },
    async 'cmp-close'({ label }) {
      for (const win of cmpWindows) {
        if (win.__parityLabel === label && !win.isDestroyed()) {
          record('windows.jsonl', {
            kind: 'cmp-window-before-close',
            label,
            ...describeWindow(win),
          });
          win.close();
        }
      }
    },
    async 'cmp-state'({ label }) {
      for (const win of cmpWindows) {
        if (win.__parityLabel === label && !win.isDestroyed()) {
          record('windows.jsonl', { kind: 'cmp-window-state', label, ...describeWindow(win) });
        }
      }
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

  // Long runs: a heartbeat line so gaps (sleep) are visible in the capture.
  function startTicks() {
    if (!config.tickMs) return;
    setInterval(() => {
      record('ticks.jsonl', { at: new Date().toISOString() });
    }, config.tickMs).unref();
  }

  return {
    startFeatureServer,
    rewriteUrl,
    calibrate,
    runActions,
    startTicks,
    ownExec,
    describeWindow,
  };
};

// The gesture-timing fixture (served on loopback, loaded into an ad guest in
// place of the ad page; never an ad). `__gc.arm(case)` prepares one case and
// returns the point to click; the click (or Return on the focused button)
// runs the case's action after `delay` ms: window.open, two opens, a script
// top-level navigation, or the default action of a target=_blank link. Every
// URL uses the unregistered `owparity-canary` scheme. `__gc.log` collects
// what the page saw: the trigger's isTrusted, navigator.userActivation
// before and after, and whether window.open returned a window.
const NAVIGATION_EVENTS = new Set(['will-navigate', 'will-frame-navigate', 'will-redirect']);

const GESTURE_FIXTURE = `<!doctype html>
<meta charset="utf-8">
<title>gesture fixture</title>
<style>
  html, body { margin: 0; background: #1d2733; }
  #b, #a { position: fixed; left: 0; width: 300px; height: 120px; display: block; }
  #b { top: 0; }
  #a { top: 125px; background: #2f3d4d; color: #fff; }
</style>
<button id="b">fixture button</button>
<a id="a" target="_blank" href="#">fixture link</a>
<script>
  const g = (window.__gc = { log: [], armed: null });
  const b = document.getElementById('b');
  const a = document.getElementById('a');
  const act = () => {
    try { return { isActive: navigator.userActivation.isActive, hasBeenActive: navigator.userActivation.hasBeenActive }; }
    catch (e) { return null; }
  };
  g.arm = (c) => {
    g.armed = c;
    const el = c.kind === 'anchor' ? a : b;
    if (c.kind === 'anchor') a.href = c.url;
    const r = el.getBoundingClientRect();
    return { point: { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) }, act: act() };
  };
  g.focusTarget = () => { b.focus(); return document.activeElement === b; };
  const perform = (c, t0, trigger) => {
    const r = { id: c.id, ev: 'action', kind: c.kind, trigger, sinceTriggerMs: Math.round(performance.now() - t0), actBefore: act() };
    try {
      if (c.kind === 'open-twice') {
        const w1 = window.open(c.url + '-1', '_blank');
        const w2 = window.open(c.url + '-2', '_blank');
        r.result = [w1 === null ? 'null' : 'window', w2 === null ? 'null' : 'window'];
      } else if (c.kind === 'top') {
        location.href = c.url;
        r.result = 'assigned';
      } else {
        const w = window.open(c.url, '_blank');
        r.result = w === null ? 'null' : 'window';
      }
    } catch (e) { r.error = String(e); }
    r.actAfter = act();
    g.log.push(r);
  };
  const onTrigger = (ev) => {
    const c = g.armed;
    if (!c) return;
    g.armed = null;
    g.log.push({ id: c.id, ev: ev.type, target: ev.currentTarget.id, trusted: ev.isTrusted, act: act() });
    if (c.kind === 'anchor') return;
    ev.preventDefault();
    const t0 = performance.now();
    if (c.delay > 0) setTimeout(() => perform(c, t0, ev.type), c.delay);
    else perform(c, t0, ev.type);
  };
  b.addEventListener('click', onTrigger);
  a.addEventListener('click', onTrigger);
  g.fire = () => {
    const c = g.armed;
    g.armed = null;
    if (!c) return 'not armed';
    perform(c, performance.now(), 'script');
    return 'fired';
  };
</script>
`;

// Installed by the 'hook-guest-frames' action: every same-origin frame below
// the ad guest's top frame gets a capturing 'message' listener (re-checked
// every second for new frames). Messages are logged as __PARITYF__<json>.
const GUEST_FRAME_HOOK = `(() => {
  const summarize = (d) => { try { return typeof d === 'string' ? d.slice(0, 1500) : JSON.stringify(d).slice(0, 1500); } catch (e) { return String(d).slice(0, 200); } };
  const hook = (w, path) => {
    try {
      if (w.__parityFrameHooked) return 0;
      w.__parityFrameHooked = true;
      w.addEventListener('message', (e) => {
        try { console.log('__PARITYF__' + JSON.stringify({ path, href: w.location.href.slice(0, 200), origin: e.origin, fromParent: e.source === w.parent, data: summarize(e.data) })); } catch (err) {}
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
  if (!window.__parityFrameTimer) window.__parityFrameTimer = setInterval(() => walk(window, ''), 1000);
  return walk(window, '');
})()`;

/**
 * The colour (0-1 rgba, rounded to 0.001) of a NativeImage at each point (CSS
 * px from the image's top left; `cssWidth` is the captured width in CSS px)
 * and a 16x16 grid of alpha counts. The shape
 * matches the Tauri harness's native snapshot samples.
 */
function sampleImage(image, points, cssWidth) {
  const { width, height } = image.getSize();
  if (!width || !height) return { error: 'empty snapshot' };
  const bitmap = image.toBitmap();
  const scale = cssWidth ? width / cssWidth : 1;
  const round = (v) => Math.round((v / 255) * 1000) / 1000;
  const rgba = (px, py) => {
    const col = Math.floor(px);
    const row = Math.floor(py);
    if (col < 0 || row < 0 || col >= width || row >= height) return null;
    const i = (row * width + col) * 4;
    // BGRA (premultiplied) in Electron's toBitmap.
    return [bitmap[i + 2], bitmap[i + 1], bitmap[i], bitmap[i + 3]].map(round);
  };
  let transparent = 0;
  let opaque = 0;
  let total = 0;
  for (let i = 0; i < 16; i += 1) {
    for (let j = 0; j < 16; j += 1) {
      const c = rgba(((i + 0.5) * width) / 16, ((j + 0.5) * height) / 16);
      if (!c) continue;
      total += 1;
      if (c[3] < 0.01) transparent += 1;
      else if (c[3] > 0.99) opaque += 1;
    }
  }
  return {
    pixels: [width, height],
    points: [width / scale, height / scale],
    samples: points.map((p) => ({
      name: p.name,
      local: [p.x, p.y],
      rgba: rgba(p.x * scale, p.y * scale),
    })),
    grid: { total, transparent, opaque },
  };
}

function safeGet(fn) {
  try {
    return fn();
  } catch {
    return null;
  }
}
