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
    async 'probe-guests'({ label }) {
      await probeAllGuests(label);
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

function safeGet(fn) {
  try {
    return fn();
  } catch {
    return null;
  }
}
