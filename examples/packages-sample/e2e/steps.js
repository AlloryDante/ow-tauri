// End-to-end steps for the packages sample, shared by both hosts.
//
// The same steps drive the ported sample on ow-tauri (`tauri-driver.js`,
// bundled into the hidden main webview) and the upstream sample on
// ow-electron (`electron-main.cjs`, the baseline). They run in the main
// process and act on the main window's page only through
// `webContents.executeJavaScript`: they click the sample's buttons, change
// its inputs and read back what the page shows, logs and throws. Nothing
// here is host specific; the `host` adapter supplies the few host calls.
//
// Safety: the steps never click an ad, never press "Restart App" (it would
// relaunch the app outside the runner's control) and close the main window
// only as the very last step. Dialogs and the file manager are kept closed
// by the host (ow-tauri lab mode; the baseline stubs `dialog`).
//
// CommonJS so that Node (ow-electron) and webpack (ow-tauri) both load it.

'use strict';

/**
 * The probe installed in the page: buffers console output, errors,
 * unhandled rejections, `alert` / `confirm` / `prompt` calls (stubbed, so no
 * dialog appears in either host) and the main process's `console-message`
 * log lines, and offers small DOM helpers. Idempotent.
 */
const PROBE = String.raw`(() => {
  if (window.__e2e) return 'present';
  const buf = [];
  const fmt = (v, depth = 0) => {
    try {
      if (v === undefined) return 'undefined';
      if (v === null) return 'null';
      if (typeof v === 'string') return v;
      if (typeof v === 'function') return '[function]';
      if (v instanceof Error) return v.name + ': ' + v.message;
      if (typeof Event !== 'undefined' && v instanceof Event) {
        const out = { event: v.type };
        if (v.detail !== undefined && v.detail !== null) out.detail = v.detail;
        if (v.args !== undefined) out.args = v.args;
        return JSON.stringify(out);
      }
      if (typeof v === 'object') return depth > 2 ? '[object]' : JSON.stringify(v);
      return String(v);
    } catch (e) {
      return '[unserialisable]';
    }
  };
  const push = (kind, data) => buf.push({ t: Date.now(), kind, ...data });
  for (const level of ['log', 'info', 'warn', 'error', 'debug']) {
    const original = console[level];
    console[level] = function (...args) {
      push('console', { level, text: args.map((a) => fmt(a)).join(' ') });
      return original.apply(this, args);
    };
  }
  window.addEventListener('error', (e) =>
    push('error', { text: String(e.message), source: String(e.filename || '').split('/').pop() }),
  );
  window.addEventListener('unhandledrejection', (e) => push('unhandledrejection', { text: fmt(e.reason) }));
  window.alert = (m) => push('alert', { text: String(m) });
  window.confirm = (m) => (push('confirm', { text: String(m) }), false);
  window.prompt = (m) => (push('prompt', { text: String(m) }), null);
  try {
    window.app.onMessage((message, type, args) =>
      push('app-log', { level: type, text: [message, ...(args || []).map((a) => fmt(a))].join(' ') }),
    );
  } catch (e) {
    push('probe', { text: 'app.onMessage unavailable: ' + e.message });
  }

  const visible = (el) => !!(el && (el.offsetWidth || el.offsetHeight || el.getClientRects().length));
  // A control's name: a button's text or title; for a field, its label (or
  // the first text around it), never its value, so the name stays the same
  // when the driver changes the value. Built from text nodes, not innerText:
  // the engines differ there (WebKit leaves out text clipped by a collapsed
  // container and a <select>'s options), so the names would differ by host.
  const clean = (text) => String(text || '').replace(/\s+/g, ' ').trim();
  const firstText = (root) => {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) =>
        n.parentElement && n.parentElement.closest('select, option, textarea, script, style')
          ? NodeFilter.FILTER_REJECT
          : NodeFilter.FILTER_ACCEPT,
    });
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const t = clean(n.nodeValue);
      if (t) return t;
    }
    return '';
  };
  const labelOf = (el) => {
    let text = '';
    if (el.tagName === 'BUTTON') {
      text = firstText(el) || el.getAttribute('title') || '';
    } else {
      const label = (el.id && document.querySelector('label[for="' + CSS.escape(el.id) + '"]')) || el.closest('label');
      text = label ? firstText(label) : '';
      if (!text) {
        const near = el.closest('.input-element, .setting-item, li, .row, div');
        text = near ? firstText(near) : '';
      }
      text = text || el.getAttribute('title') || el.getAttribute('placeholder') || el.getAttribute('name') || '';
    }
    return clean(text).slice(0, 60);
  };
  const controls = (root) => {
    const scope = root ? document.querySelector(root) : document;
    if (!scope) return [];
    const seen = new Map();
    const out = [];
    for (const el of scope.querySelectorAll('button, select, input, textarea')) {
      if (!visible(el) && !(el.tagName === 'INPUT' && (el.type === 'checkbox' || el.type === 'radio'))) continue;
      const kind = el.tagName === 'INPUT' ? 'input:' + (el.type || 'text') : el.tagName.toLowerCase();
      const base = kind + '|' + (el.id || '') + '|' + labelOf(el);
      const n = seen.get(base) || 0;
      seen.set(base, n + 1);
      out.push({ key: base + '|' + n, kind, id: el.id || null, label: labelOf(el), disabled: !!el.disabled, el });
    }
    return out;
  };
  const setNative = (el, value) => {
    const proto = el.tagName === 'SELECT' ? HTMLSelectElement.prototype
      : el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  };
  window.__e2e = {
    take: () => buf.splice(0),
    controls: (root) => controls(root).map(({ el, ...c }) => c),
    act: (root, key) => {
      const c = controls(root).find((x) => x.key === key);
      if (!c) return { missing: true };
      const el = c.el;
      if (el.disabled) return { disabled: true };
      if (c.kind === 'button' || c.kind === 'input:checkbox' || c.kind === 'input:radio' || c.kind === 'input:submit') {
        el.click();
        return { clicked: true, checked: el.checked };
      }
      if (c.kind === 'select') {
        const options = [...el.options].map((o) => o.value);
        const next = options.find((v) => v !== el.value);
        if (next === undefined) return { options, unchanged: el.value };
        setNative(el, next);
        return { options, from: options[0] === next ? undefined : el.value, to: next };
      }
      let value = 'e2e';
      if (c.kind === 'input:number' || c.kind === 'input:range') {
        const max = Number(el.max);
        value = String(Number.isFinite(max) && el.max !== '' ? max : Number(el.value || 0) + 1);
      } else if (c.kind === 'input:color') value = '#123456';
      else if (c.kind === 'input:email' || /mail/i.test(el.placeholder || '')) value = 'e2e@example.com';
      setNative(el, value);
      return { value: el.value };
    },
    click: (selector, index = 0) => {
      const el = document.querySelectorAll(selector)[index];
      if (!el) return { missing: true };
      el.click();
      return { clicked: true };
    },
    set: (selector, value, index = 0) => {
      const el = document.querySelectorAll(selector)[index];
      if (!el) return { missing: true };
      setNative(el, value);
      return { value: el.value };
    },
    text: (selector) => [...document.querySelectorAll(selector)].map((e) => (e.innerText || '').trim()),
    count: (selector) => document.querySelectorAll(selector).length,
    logLines: () => [...document.querySelectorAll('#TerminalTextArea .log-entry')].map((e) => e.innerText.replace(/\s+/g, ' ').trim()),
    route: () => location.hash,
    go: (hash) => { location.hash = hash; return location.hash; },
    adviews: () => [...document.querySelectorAll('owadview')].map((e) => ({
      id: e.id || null,
      cid: e.getAttribute('cid'),
      slotsize: e.getAttribute('slotsize'),
      adstyle: e.getAttribute('adstyle'),
      performance: e.hasAttribute('performance'),
      parent: e.parentElement ? (e.parentElement.id || e.parentElement.tagName.toLowerCase()) : null,
      box: [Math.round(e.getBoundingClientRect().width), Math.round(e.getBoundingClientRect().height)],
    })),
  };
  return 'installed';
})()`;

/** Every top button of the Logger page, in the order the sample shows them. */
const TOP_BUTTONS = [
  'setRequiredFeaturesBtn',
  'getInfoBtn',
  'createOSR',
  'createDPIOSR',
  'visibilityOSR',
  'trackSpecificClassIdButton',
  'disableAdsFPD',
  'disableAdsOptimization',
  'hasPendingUpdates',
  'scanGameskey',
  'getUtmParams',
];

/** Controls the generic page pass never touches (key prefixes or labels). */
const SKIP = [
  // Relaunches the app outside the runner's control.
  { label: /^Restart App$/ },
  // JSON tree toggles in log arguments: no app behaviour.
  { label: /^[▶▼►▸▾+−-]?$/, kind: 'button' },
];

function skipped(control) {
  return SKIP.find(
    (s) => (!s.kind || s.kind === control.kind) && s.label.test(control.label),
  );
}

/**
 * Runs every step.
 *
 * @param {object} host the host adapter:
 *   - `name`: `'tauri'` or `'electron'`
 *   - `record(entry)`: appends one JSON record to the run's `e2e.jsonl`
 *   - `mainWindow()`: the sample's main `BrowserWindow`, or null
 *   - `windows()`: `[{ id, url, visible }]` of the app's windows
 *   - `overwolf`: `app.overwolf`
 *   - `checkUpdates(feedUrl)`: configures the host's `autoUpdater` as the
 *     sample does, but against `feedUrl`, and resolves what it reported
 *   - `quit()`: quits the app
 *   - `config`: `{ feedUrl, adWaitMs, settleMs, performanceWaitMs }`, and
 *     for the idle run `{ idleMs, idleSampleMs, idleLayout }`
 */
async function runSteps(host) {
  const config = { adWaitMs: 8000, settleMs: 1200, performanceWaitMs: 10000, ...host.config };
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  let seq = 0;
  let muidTag = null;
  let muidValue = null;

  const redact = (value) => {
    if (!muidValue) return value;
    const text = typeof value === 'string' ? value : JSON.stringify(value);
    const out = text.split(muidValue).join(muidTag);
    return typeof value === 'string' ? out : JSON.parse(out);
  };
  const record = (entry) => host.record(redact({ seq: seq++, host: host.name, ...entry }));

  const win = () => host.mainWindow();
  const exec = async (code) => {
    const w = win();
    if (!w) throw new Error('no main window');
    return w.webContents.executeJavaScript(code);
  };
  const call = (expr) => exec(`window.__e2e ? (${expr}) : { probe: 'missing' }`);

  // Main-process output since the last step, from the host adapter.
  const mainOut = () => (host.takeMain ? host.takeMain() : []);

  async function settle(ms = config.settleMs) {
    await sleep(ms);
    let page = [];
    try {
      page = (await call('window.__e2e.take()')) || [];
      if (!Array.isArray(page)) page = [{ kind: 'probe', text: JSON.stringify(page) }];
    } catch (error) {
      page = [{ kind: 'exec-error', text: String(error && error.message) }];
    }
    return { events: page, main: mainOut(), windows: host.windows() };
  }

  async function step(page, action, target, fn, settleMs) {
    let result;
    let error = null;
    try {
      result = await fn();
    } catch (e) {
      error = String(e && e.message);
    }
    const out = await settle(settleMs);
    record({ kind: 'step', page, action, target, result, error, ...out });
    return result;
  }

  // ---------------------------------------------------------------- boot
  const started = Date.now();
  while (!win() && Date.now() - started < 30000) await sleep(200);
  if (!win()) {
    record({
      kind: 'fatal',
      text: 'the main window never appeared',
      main: host.takeMain(),
      windows: await host.windows(),
    });
    return;
  }
  let ready = false;
  while (!ready && Date.now() - started < 60000) {
    try {
      ready = await exec("document.readyState === 'complete' && !!document.querySelector('.side-bar')");
    } catch {
      // the page is still loading
    }
    if (!ready) await sleep(250);
  }
  // Let the startup log lines (uid, muid, phase) arrive.
  await sleep(1500);
  const ow = host.overwolf;
  muidValue = ow && typeof ow.muid === 'string' && ow.muid ? ow.muid : null;
  if (muidValue) {
    const digest = await globalThis.crypto.subtle.digest('SHA-256', new TextEncoder().encode(muidValue));
    muidTag = '<muid sha256:' + [...new Uint8Array(digest)].slice(0, 4).map((b) => b.toString(16).padStart(2, '0')).join('') + '>';
  }
  const probe = await exec(PROBE);
  record({
    kind: 'boot',
    ready,
    probe,
    overwolf: ow ? { uid: ow.uid, muid: muidTag, phasePercent: ow.phasePercent } : null,
    route: await call('window.__e2e.route()'),
    nav: await call("window.__e2e.text('.main-menu li')"),
    version: await call("window.__e2e.text('.electron-version')"),
    logLines: await call('window.__e2e.logLines()'),
    windows: host.windows(),
    main: mainOut(),
  });

  if (config.idleMs) {
    await idleRun();
    return;
  }

  // ---------------------------------------------------------- Logger (/)
  await step('logger', 'navigate', '#/', () => call("window.__e2e.go('#/')"));
  for (const id of TOP_BUTTONS) {
    await step(
      'logger',
      'click',
      id,
      () => call(`window.__e2e.click('#${id}')`),
      id === 'createDPIOSR' || id === 'createOSR' ? 4000 : config.settleMs,
    );
  }
  await step('logger', 'type', 'trackSpecificClassId=abc', () => call("window.__e2e.set('#trackSpecificClassId', '')"));
  await step('logger', 'click', 'trackSpecificClassIdButton (empty classId)', () =>
    call("window.__e2e.click('#trackSpecificClassIdButton')"),
  );
  await step('logger', 'type', 'log search "uid"', () => call("window.__e2e.set('.log-search', 'uid')"));
  record({ kind: 'state', page: 'logger', what: 'search result', value: await call("window.__e2e.text('.log-search-count')") });
  await step('logger', 'type', 'log search cleared', () => call("window.__e2e.set('.log-search', '')"));
  await step('logger', 'click', 'autoScrollCheckbox', () => call("window.__e2e.click('#autoScrollCheckbox')"));
  record({ kind: 'state', page: 'logger', what: 'log lines', value: await call('window.__e2e.logLines()') });
  await step('logger', 'click', 'clearTerminalTextAreaBtn', () => call("window.__e2e.click('#clearTerminalTextAreaBtn')"));
  record({ kind: 'state', page: 'logger', what: 'log lines after Clear', value: await call('window.__e2e.logLines()') });

  // ------------------------------------------------------ Ads Tester
  await step('ads-tester', 'navigate', '#/ads-tester', () => call("window.__e2e.go('#/ads-tester')"));
  const layouts = await call("[...document.querySelectorAll('#layout-select option')].map((o) => o.value)");
  record({
    kind: 'state',
    page: 'ads-tester',
    what: 'layouts',
    value: { layouts, selected: await call("document.querySelector('#layout-select').value") },
  });
  for (const layout of Array.isArray(layouts) ? layouts : []) {
    await step('ads-tester', 'select-layout', layout, () => call(`window.__e2e.set('#layout-select', ${JSON.stringify(layout)})`), 400);
    record({
      kind: 'state',
      page: 'ads-tester',
      what: 'slots',
      layout,
      value: await call("[...document.querySelectorAll('.ad-wrapper')].map((w) => ({ cls: w.className.replace(/\\s+/g, ' ').trim(), label: (w.querySelector('.ad-actions span') || {}).innerText, box: [w.querySelector('.ad-container').style.width, w.querySelector('.ad-container').style.height] }))"),
    });
    const wrappers = await call("document.querySelectorAll('.ad-wrapper').length");
    for (let i = 0; i < wrappers; i += 1) {
      await step('ads-tester', 'startAd', `${layout} #${i + 1}`, () => call(`window.__e2e.click('.ad-wrapper #startAdButton', ${i})`), 100);
    }
    await step('ads-tester', 'wait-ads', layout, async () => call('window.__e2e.adviews()'), config.adWaitMs);
    record({ kind: 'state', page: 'ads-tester', what: 'adviews', layout, value: await call('window.__e2e.adviews()') });
    for (let i = 0; i < wrappers; i += 1) {
      await step('ads-tester', 'removeAd', `${layout} #${i + 1}`, () => call(`window.__e2e.click('.ad-wrapper #stopAdButton', ${i})`), 300);
    }
  }
  await step('ads-tester', 'click', 'performanceAdButton', () => call("window.__e2e.click('#performanceAdButton')"), config.performanceWaitMs);
  record({ kind: 'state', page: 'ads-tester', what: 'adviews after performance ad', value: await call('window.__e2e.adviews()') });

  // -------------------------------------------------------- Channels
  await step('channels', 'navigate', '#/channels', () => call("window.__e2e.go('#/channels')"), 2500);
  await genericPass('channels', '.channels-section');

  // ---------------------------------------------------- App Settings
  await step('app-settings', 'navigate', '#/app-settings', () => call("window.__e2e.go('#/app-settings')"), 2500);
  record({ kind: 'state', page: 'app-settings', what: 'sections', value: await call("window.__e2e.text('.settings-options-item .option-info h3')") });
  await genericPass('app-settings', '.settings-section');

  // ---------------------------------------------- Recording settings
  await step('recording-settings', 'navigate', '#/recording-settings', () => call("window.__e2e.go('#/recording-settings')"), 2500);
  await genericPass('recording-settings', '.settings-section');

  // ------------------------------------- Updater against the local feed
  if (config.feedUrl && host.checkUpdates) {
    for (const feed of ['newer', 'same']) {
      const url = `${config.feedUrl}/${feed}`;
      let result;
      let error = null;
      try {
        result = await host.checkUpdates(url);
      } catch (e) {
        error = String(e && e.message);
      }
      record({ kind: 'step', page: 'updater', action: 'checkForUpdates', target: feed, result, error, main: mainOut() });
    }
  }

  // ------------------------------------------------- Header (last)
  await step('header', 'navigate', '#/', () => call("window.__e2e.go('#/')"));
  for (const title of ['Minimize window', 'Maximize window', 'Maximize window']) {
    await step('header', 'click', title, async () => {
      const r = await call(`window.__e2e.click('.window-actions button[title="${title}"]')`);
      return r;
    }, 1500);
    const w = win();
    record({
      kind: 'state',
      page: 'header',
      what: `window after ${title}`,
      value: w ? { minimized: w.isMinimized(), maximized: w.isMaximized(), visible: w.isVisible() } : null,
    });
  }
  await step('header', 'click', 'Close App', () => call("window.__e2e.click('.window-actions .close-btn')"), 2000);
  record({ kind: 'state', page: 'header', what: 'windows after Close App', value: host.windows() });
  record({ kind: 'done', ms: Date.now() - started });
  await sleep(500);
  host.quit();

  /**
   * The idle run (`config.idleMs`): starts every slot of one Ads Tester
   * layout (`config.idleLayout`, else the first), leaves the ads running for
   * `idleMs`, recording a sample every `idleSampleMs` (the page's and the
   * main process's output since the last one), then removes the ads and
   * closes the app. The runner samples the processes' memory meanwhile.
   */
  async function idleRun() {
    const every = config.idleSampleMs || 30000;
    await step('idle', 'navigate', '#/ads-tester', () => call("window.__e2e.go('#/ads-tester')"));
    const layouts = await call("[...document.querySelectorAll('#layout-select option')].map((o) => o.value)");
    const layout = config.idleLayout || (Array.isArray(layouts) ? layouts[0] : undefined);
    await step('idle', 'select-layout', layout, () => call(`window.__e2e.set('#layout-select', ${JSON.stringify(layout)})`), 400);
    const wrappers = await call("document.querySelectorAll('.ad-wrapper').length");
    for (let i = 0; i < wrappers; i += 1) {
      await step('idle', 'startAd', `${layout} #${i + 1}`, () => call(`window.__e2e.click('.ad-wrapper #startAdButton', ${i})`), 100);
    }
    const idleStart = Date.now();
    for (let n = 1; Date.now() - idleStart < config.idleMs; n += 1) {
      await step('idle', 'sample', String(n), async () => ({
        elapsedMs: Date.now() - idleStart,
        adviews: await call('window.__e2e.adviews()'),
        windows: host.windows().length,
      }), every);
    }
    for (let i = 0; i < wrappers; i += 1) {
      await step('idle', 'removeAd', `${layout} #${i + 1}`, () => call(`window.__e2e.click('.ad-wrapper #stopAdButton', ${i})`), 300);
    }
    await step('idle', 'click', 'Close App', () => call("window.__e2e.click('.window-actions .close-btn')"), 2000);
    record({ kind: 'done', ms: Date.now() - started });
    await sleep(500);
    host.quit();
  }

  /**
   * Acts once on every control of the page under `root`: clicks each
   * button, checkbox and radio, moves each select to another option and
   * types into each field, in document order, re-reading the page after each
   * action (React may add or remove controls).
   */
  async function genericPass(page, scope) {
    const done = new Set();
    const listed = await call(`window.__e2e.controls(${JSON.stringify(scope)})`);
    record({ kind: 'state', page, what: 'controls', value: Array.isArray(listed) ? listed.map((c) => c.key) : listed });
    for (let guard = 0; guard < 200; guard += 1) {
      const list = await call(`window.__e2e.controls(${JSON.stringify(scope)})`);
      if (!Array.isArray(list)) break;
      const next = list.find((c) => !done.has(c.key));
      if (!next) break;
      done.add(next.key);
      if (skipped(next)) {
        record({ kind: 'skip', page, target: next.key, reason: 'unsafe in the lab or no app behaviour' });
        continue;
      }
      await step(page, 'act', next.key, () => call(`window.__e2e.act(${JSON.stringify(scope)}, ${JSON.stringify(next.key)})`));
      const route = await call('window.__e2e.route()');
      if (route !== `#/${page === 'logger' ? '' : page}`) {
        // A link (e.g. "Recording settings") navigated away: come back.
        record({ kind: 'state', page, what: 'navigated', value: route });
        await step(page, 'navigate', `#/${page}`, () => call(`window.__e2e.go('#/${page}')`), 1500);
      }
    }
  }
}

module.exports = { runSteps, PROBE, TOP_BUTTONS };
