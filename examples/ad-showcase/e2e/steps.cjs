// The lab steps, shared by both hosts (tauri-driver.js on ow-tauri,
// electron-main.cjs on ow-electron). They run in the main process and drive
// the window only through `webContents.executeJavaScript`: pressing the
// showcase's own buttons (by their `data-action`), choosing its selects,
// scrolling its own scroll boxes and reading `window.__showcase`. They never
// touch an ad, never send input into an ad guest and never press Restart.
//
// The one input event they create is the interstitial pass-through check
// (test mode only): a click at the page's own "Click me" button while the
// interstitial loads, sent only when the page's hit test (and, on ow-tauri,
// the window's native hit test) says it reaches that button and not the ad.
//
// host = {
//   name: 'tauri' | 'electron',
//   config: { steps, mode, adWaitMs, dwellMs, stillsDir? },
//   record(entry),                 // one line of e2e.jsonl
//   mainWindow(),                  // the showcase BrowserWindow, or null
//   quit(),
//   nativeProbe?(points, click),   // ow-tauri: the window's native hit test
//   pageClick?(x, y),              // ow-electron: a click into the app page
//   still?(name),                  // ow-tauri: an in-process still (PNG)
//   probeGuests?(phase),           // ow-tauri: the lab's guest page probe
//   windows?(),                    // the app's windows (state only)
//   restartPhase?,                 // ow-tauri restart check: 'first' | 'second' | 'third'
//   flush?(),                      // resolves once every record is written
// }
//
// The `restart` scenario (ow-tauri, test mode) is the one exception to "never
// press Restart": it starts on the parity page (no ad guest) in TEST,
// restarts in LIVE onto `parity/restarted`, then in TEST onto
// `parity/restarted-again`, both through `window.showcase.restart`. Each
// process reports its ad mode and the page it came back on, invisible (the
// lab environment is inherited); the third one quits. No phase mounts an ad
// guest, so the LIVE phase loads no ad.

'use strict';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** The scenarios and the ad guests each one mounts (the live budget). */
const SCENARIOS = {
  smoke: { route: null, loads: null },
  tour: { route: null, loads: null },
  'live-layout': { route: 'layouts/combo-classic', loads: 2 },
  'live-300x250': { route: 'sizes/300x250', loads: 1 },
  'live-reward': { route: 'reward', loads: 2 },
  'live-perf': { route: 'interstitial', loads: 1 },
  restart: { route: 'parity', loads: 0 },
};

/** The route the restart check restarts onto first (in LIVE). */
const RESTART_ROUTE = 'parity/restarted';
/** The route of the restart check's second restart (back to TEST). */
const RESTART_AGAIN_ROUTE = 'parity/restarted-again';

/**
 * The restart check's phase of a process, from the hash it started on.
 *
 * @param {string} hash - `location.hash` before the page changes it
 * @returns {'first' | 'second' | 'third'}
 */
function restartPhaseOf(hash) {
  if (hash === `#${RESTART_ROUTE}`) return 'second';
  if (hash === `#${RESTART_AGAIN_ROUTE}`) return 'third';
  return 'first';
}

/** The ad mode and route each restart check phase restarts with (none: the last). */
const RESTART_NEXT = {
  first: { mode: 'live', route: RESTART_ROUTE },
  second: { mode: 'test', route: RESTART_AGAIN_ROUTE },
  third: null,
};

/** The eight layouts of page 2, in select order. */
const LAYOUTS = [
  'combo-classic',
  'tall-duo',
  'tower-plus',
  'studio-tower',
  'tower',
  'studio',
  'studio-plus',
  'popup-studio-plus',
];

/** Statuses of a slot that has an ad (or a ready reward video). */
const FILLED = new Set(['loaded', 'ready', 'playing']);

async function runSteps(host) {
  const t0 = Date.now();
  const cfg = host.config || {};
  const mode = cfg.mode === 'live' ? 'live' : 'test';
  const adWaitMs = Number(cfg.adWaitMs) || 60000;
  const dwellMs = Number(cfg.dwellMs) || 8000;
  let seq = 0;
  let cursor = 0;
  const record = (entry) => {
    seq += 1;
    host.record({ seq, t: Date.now() - t0, host: host.name, ...entry });
  };

  const exec = async (code) => {
    const win = host.mainWindow();
    if (!win) throw new Error('no showcase window');
    return win.webContents.executeJavaScript(code);
  };
  const json = async (code) => JSON.parse(await exec(`JSON.stringify(${code})`));
  const snapshot = () => json('window.__showcase ? window.__showcase.snapshot() : null');
  const newEntries = async () => {
    const rows = await json(`window.__showcase ? window.__showcase.entries(${cursor}) : []`);
    cursor += rows.length;
    return rows;
  };
  /** Event names of every row whose cid matches `pattern`, oldest first. */
  const eventsOf = (pattern) =>
    json(`window.__showcase.entries(0).filter((e) => ${pattern}.test(e.cid)).map((e) => e.name)`);
  const count = async (pattern, name) => (await eventsOf(pattern)).filter((n) => n === name).length;
  const press = (action) =>
    exec(
      `(() => { const b = document.querySelector('[data-action=${JSON.stringify(action)}]'); if (!b) return 'missing'; if (b.disabled) return 'disabled'; b.click(); return 'pressed'; })()`,
    );
  const choose = (action, value) =>
    exec(
      `(() => { const s = document.querySelector('[data-action=${JSON.stringify(action)}]'); if (!s) return 'missing'; s.value = ${JSON.stringify(value)}; s.dispatchEvent(new Event('change')); return s.value === ${JSON.stringify(value)} ? 'chosen' : 'no such option'; })()`,
    );
  const act = async (page, action, fn) => {
    const result = await fn();
    record({ kind: 'action', page, action, result });
    return result;
  };
  const step = async (name, extra = {}) => {
    const snap = await snapshot();
    record({ kind: 'step', name, snapshot: snap, events: await newEntries(), ...extra });
    return snap;
  };
  const waitFor = async (fn, timeoutMs, everyMs = 500) => {
    const started = Date.now();
    const end = started + timeoutMs;
    for (;;) {
      const value = await fn();
      if (value) return { ok: true, ms: Date.now() - started, value };
      if (Date.now() >= end) return { ok: false, ms: Date.now() - started };
      await sleep(everyMs);
    }
  };
  const allFilled = async () => {
    const snap = await snapshot();
    const slots = (snap?.slots ?? []).filter((s) => s.display !== 'none');
    return slots.length > 0 && slots.every((s) => FILLED.has(s.status));
  };
  const still = async (name) => {
    if (!host.still || !cfg.stillsDir) return;
    // `display_ad_loaded` fires before the creative has painted: give it a
    // moment, so the still shows the ad and not an empty guest.
    await sleep(2500);
    try {
      const out = await host.still(name);
      record({ kind: 'still', name, out });
    } catch (error) {
      record({ kind: 'still', name, error: String(error) });
    }
  };
  const nav = (page) => act(page, `nav-${page}`, () => press(`nav-${page}`));

  // 1. The window and the renderer.
  let win = null;
  for (let i = 0; i < 300 && !win; i += 1) {
    win = host.mainWindow();
    if (!win) await sleep(100);
  }
  if (!win) {
    record({ kind: 'fatal', text: 'the showcase window never appeared' });
    host.quit();
    return;
  }
  let ready = false;
  for (let i = 0; i < 300 && !ready; i += 1) {
    try {
      ready = (await exec('typeof window.__showcase')) === 'object';
    } catch {
      // The page is still loading.
    }
    if (!ready) await sleep(100);
  }
  if (!ready) {
    record({ kind: 'fatal', text: 'the showcase page never started' });
    host.quit();
    return;
  }
  // Stills and comparisons use one theme, dark unless the run asks for light
  // (the lab's system appearance may be either).
  const theme = cfg.theme === 'light' ? 'light' : 'dark';
  await exec(
    `(() => { document.documentElement.dataset.theme = '${theme}'; try { localStorage.setItem('ad-showcase.theme', '${theme}'); } catch {} })()`,
  );
  await step('started', { theme });

  const scenario = cfg.steps in SCENARIOS ? cfg.steps : 'smoke';
  let loaded = false;

  if (scenario === 'restart') {
    await restartCheck();
    return;
  }
  if (scenario === 'smoke' || scenario === 'tour') {
    // 2. Page 1 renders and its ads load (test mode).
    const filled = await waitFor(allFilled, adWaitMs);
    loaded = (await snapshot())?.counts?.display_ad_loaded >= 1;
    await step('page-1', { displayAdLoaded: loaded, filled });
    if (scenario === 'tour') {
      await tour();
      await exportTimeline();
    }
  } else {
    await live();
  }

  record({ kind: 'done', displayAdLoaded: loaded });
  host.quit();

  // --------------------------------------------------------------- restart
  async function restartCheck() {
    const route = await exec('location.hash');
    const phase = host.restartPhase ?? 'first';
    const snap = await snapshot();
    await step(`restart-${phase}`, {
      route,
      mode: snap?.mode ?? null,
      pid: cfg.pid ?? null,
      windows: phase === 'first' || !host.windows ? null : await host.windows(),
    });
    const next = RESTART_NEXT[phase];
    if (!next) {
      record({ kind: 'done', restarted: true, route });
      host.quit();
      return;
    }
    record({ kind: 'restart-requested', phase, mode: next.mode, route: next.route });
    if (host.flush) await host.flush();
    const result = await exec(
      `window.showcase.restart(${JSON.stringify(next.mode)}, ${JSON.stringify(next.route)}).then(() => 'requested', (e) => 'refused: ' + String(e))`,
    );
    // A granted restart exits this process soon after (at the app's exit,
    // once the plugins are done), sometimes after the answer arrives: only
    // a refusal, or a process still running 15 s later, is a failure.
    if (result === 'requested') await sleep(15000);
    record({ kind: 'fatal', text: `restart did not exit the app (${String(result)})` });
    host.quit();
  }

  // ------------------------------------------------------------------ tour
  async function tour() {
    // Page 1: the groups, the fold demo.
    await still('p1-sizes-towers');
    await act('sizes', 'sizes-select=banners', () => choose('sizes-select', 'banners'));
    await step('sizes-banners', { filled: await waitFor(allFilled, 45000) });
    await still('p1-sizes-banners');
    await act('sizes', 'sizes-select=fold', () => choose('sizes-select', 'fold'));
    await sleep(dwellMs);
    await step('sizes-fold-waiting');
    await still('p1-sizes-fold-waiting');
    await act('sizes', 'scroll fold box', () =>
      exec(
        "(() => { const s = document.querySelector('[data-action=sizes-fold-scroller]'); if (!s) return 'missing'; s.scrollTop = s.scrollHeight; return 'scrolled'; })()",
      ),
    );
    await step('sizes-fold-scrolled', { filled: await waitFor(allFilled, 40000) });
    await still('p1-sizes-fold-scrolled');

    // Page 2: every layout, fresh containers each time.
    await nav('layouts');
    for (const id of LAYOUTS) {
      await act('layouts', `layout-select=${id}`, () => choose('layout-select', id));
      await step(`layouts-${id}`, { filled: await waitFor(allFilled, 40000) });
      await still(`p2-layout-${id}`);
    }

    // Page 3: takeover and restore.
    await nav('high-impact');
    const hi = /^hi-400x600$/;
    await step('hi-loaded', {
      takeover: await waitFor(async () => (await count(hi, 'high-impact-ad-loaded')) > 0, 60000),
    });
    await sleep(500);
    await step('hi-takeover');
    await still('p3-high-impact-takeover');
    await step('hi-removed', {
      removed: await waitFor(async () => (await count(hi, 'high-impact-ad-removed')) > 0, 60000),
    });
    await sleep(500);
    await step('hi-restored');
    await still('p3-high-impact-restored');

    // Page 4: pass-through while loading, modal after load, one per window,
    // the error path, the variants, app removal and an unknown unit.
    await nav('interstitial');
    await sleep(1000);
    await probe('perf-before', false);
    await still('p4-interstitial-before');
    await act('interstitial', 'interstitial-default', () => press('interstitial-default'));
    await sleep(300);
    await probe('perf-loading', true);
    const first = await latestPerf();
    const firstRe = new RegExp(`^${first}$`);
    await step('perf-loading', {
      loaded: await waitFor(async () => (await count(firstRe, 'display_ad_loaded')) > 0, 30000),
    });
    await sleep(800);
    await probe('perf-loaded', false);
    await step('perf-loaded');
    await still('p4-interstitial-default');
    await act('interstitial', 'interstitial-default (second)', () => press('interstitial-default'));
    await sleep(600);
    await step('perf-second');
    // The error path: a performance ad created while the window is below
    // the ad's minimum (900x500) ends with performance_ad_error, then
    // shutdown [OBS: parity harness perf-small on both hosts]. The open ad
    // is removed first (leaving the page), so the new one is the only one.
    await leaveAndReturn('interstitial');
    await act('interstitial', 'interstitial-shrink', () => press('interstitial-shrink'));
    await sleep(1500);
    await act('interstitial', 'interstitial-default (small window)', () =>
      press('interstitial-default'),
    );
    await sleep(300);
    const smallCid = await latestPerf();
    const smallRe = new RegExp(`^${smallCid}$`);
    await step('perf-shrink', {
      ended: await waitFor(
        async () =>
          (await count(smallRe, 'performance_ad_error')) > 0 &&
          ((await count(smallRe, 'shutdown')) > 0 || (await count(smallRe, 'dom:removed')) > 0),
        20000,
      ),
    });
    await sleep(1000);
    await step('perf-shrink-after');
    await act('interstitial', 'interstitial-restore-size', () =>
      press('interstitial-restore-size'),
    );
    await sleep(2000);
    for (const variant of ['red-dim', 'blur-3']) {
      // A still-open interstitial is removed by leaving the page (the app's
      // own removal: `destroyed`, no `shutdown`).
      await leaveAndReturn('interstitial');
      await act('interstitial', `interstitial-${variant}`, () => press(`interstitial-${variant}`));
      await sleep(300);
      const cid = await latestPerf();
      const re = new RegExp(`^${cid}$`);
      await step(`perf-${variant}`, {
        loaded: await waitFor(async () => (await count(re, 'display_ad_loaded')) > 0, 30000),
      });
      await sleep(800);
      await still(`p4-interstitial-${variant}`);
    }
    await leaveAndReturn('interstitial');
    await step('perf-removed-by-app');
    await act('interstitial', 'unit=showcase-test-unit', () =>
      exec(
        "(() => { const i = document.querySelector('[data-action=interstitial-unit]'); if (!i) return 'missing'; i.value = 'showcase-test-unit'; return 'typed'; })()",
      ),
    );
    await act('interstitial', 'interstitial-unit-open', () => press('interstitial-unit-open'));
    await sleep(300);
    const unitCid = await latestPerf();
    const unitRe = new RegExp(`^${unitCid}$`);
    await step('perf-unit', {
      ended: await waitFor(
        async () =>
          (await count(unitRe, 'shutdown')) > 0 || (await count(unitRe, 'display_ad_loaded')) > 0,
        30000,
      ),
    });
    await sleep(1000);
    await step('perf-unit-after');

    // Page 5: preload, ready, watch, play, complete, granted once.
    await nav('reward');
    const rw = /^rw-400x300$/;
    await step('reward-preloading');
    await step('reward-ready', {
      ready: await waitFor(async () => (await count(rw, 'video_ad_ready')) > 0, 90000),
    });
    await sleep(500);
    await step('reward-ready-hidden');
    await still('p5-reward-ready');
    await act('reward', 'reward-watch', () => press('reward-watch'));
    await step('reward-playing', {
      play: await waitFor(async () => (await count(rw, 'play')) > 0, 30000),
    });
    await still('p5-reward-playing');
    await act('reward', 'reward-hide-2s', () => press('reward-hide-2s'));
    await sleep(3500);
    await step('reward-hide-during-play');
    await step('reward-complete', {
      complete: await waitFor(async () => (await count(rw, 'complete')) > 0, 120000),
    });
    await sleep(800);
    await step('reward-granted');
    await still('p5-reward-granted');
    await step('reward-next-ready', {
      ready: await waitFor(async () => (await count(rw, 'video_ad_ready')) > 1, 30000),
    });

    // Page 6: house (nothing set up for the identity: no house action).
    await nav('house');
    await sleep(Math.max(dwellMs, 20000));
    await step('house');
    await still('p6-house');

    // Page 7: every control on one video slot.
    await nav('controls');
    const ctl = /^ctl-400x300$/;
    await step('controls-loaded', { filled: await waitFor(allFilled, 45000) });
    await still('p7-controls');
    await act('controls', 'controls-tracking-apply', () => press('controls-tracking-apply'));
    await sleep(1000);
    await act('controls', 'controls-mute (unmute)', () => press('controls-mute'));
    await sleep(1500);
    await act('controls', 'controls-mute (mute)', () => press('controls-mute'));
    await sleep(1000);
    await step('controls-tracking-mute');
    // A new ad in the slot: a display load or, for video, a new player.
    const loads = async () =>
      (await count(ctl, 'display_ad_loaded')) + (await count(ctl, 'player_loaded'));
    let before = await loads();
    await act('controls', 'controls-display (none)', () => press('controls-display'));
    await sleep(6000);
    await step('controls-display-none');
    await act('controls', 'controls-display (block)', () => press('controls-display'));
    await step('controls-display-block', {
      refilled: await waitFor(async () => (await loads()) > before, 30000),
    });
    before = await loads();
    await act('controls', 'controls-scroll-out', () => press('controls-scroll-out'));
    await sleep(6000);
    await step('controls-scrolled-out');
    await act('controls', 'controls-scroll-back', () => press('controls-scroll-back'));
    await step('controls-scrolled-back', {
      refilled: await waitFor(async () => (await loads()) > before, 30000),
    });
    before = await loads();
    await act('controls', 'controls-hide-window', () => press('controls-hide-window'));
    await sleep(9000);
    await step('controls-window-hidden-shown', {
      refilled: await waitFor(async () => (await loads()) > before, 20000),
    });
    before = await loads();
    await act('controls', 'controls-minimize', () => press('controls-minimize'));
    await sleep(9000);
    await step('controls-minimized-restored', {
      refilled: await waitFor(async () => (await loads()) > before, 20000),
    });

    // Page 8: consent and identity.
    await nav('consent');
    await act('consent', 'consent-cmp-required', () => press('consent-cmp-required'));
    await sleep(1000);
    await act('consent', 'consent-email-hashes', () => press('consent-email-hashes'));
    await sleep(1000);
    await step('consent');
    await still('p8-consent');
    // The ad privacy settings window opens like every other lab window:
    // invisible, and the app stays in the background on both hosts.
    await act('consent', 'consent-open-privacy', () => press('consent-open-privacy'));
    await sleep(6000);
    await step('consent-privacy', { windows: host.windows ? await host.windows() : null });

    // Page 9: parity.
    await nav('parity');
    await sleep(2000);
    await step('parity');
    await still('p9-parity');
  }

  async function exportTimeline() {
    await act('timeline', 'timeline-export', () => press('timeline-export'));
    const done = await waitFor(async () => {
      const text = await exec(
        "document.querySelector('.tl-export') ? document.querySelector('.tl-export').textContent : ''",
      );
      return /^(Saved|Export failed)/.test(text) ? text : null;
    }, 10000);
    record({ kind: 'export', ok: done.ok, text: done.value ?? null });
  }

  /** The cid of the newest interstitial (`perf-<n>`). */
  async function latestPerf() {
    return json(
      "window.__showcase.entries(0).filter((e) => e.name === 'control:interstitial').map((e) => e.cid).pop() || ''",
    );
  }

  async function leaveAndReturn(page) {
    await nav('parity');
    await sleep(1500);
    await nav(page);
    await sleep(1000);
  }

  /**
   * The interstitial pass-through check at the page's "Click me" button:
   * the page's hit test, the window's native hit test (ow-tauri), and with
   * `click` (test mode only, and only when both say the click reaches the
   * button) one click there.
   */
  async function probe(name, click) {
    const state = (await snapshot())?.state ?? {};
    const [x, y] = state.target ?? [0, 0];
    const page = await json(
      `(() => { const e = document.elementFromPoint(${x}, ${y}); if (!e) return null; const a = e.closest('[data-action]'); return { tag: e.tagName.toLowerCase(), action: a ? a.dataset.action : null, inOwadview: !!e.closest('owadview') }; })()`,
    );
    const toButton = page?.action === 'interstitial-click-me';
    const wantClick = click && mode === 'test' && toButton;
    let native = null;
    if (host.nativeProbe) {
      native = await host
        .nativeProbe([{ name: 'target', x, y }], wantClick ? 'target' : null)
        .catch((error) => ({ error: String(error) }));
    }
    let clicked = null;
    if (wantClick && !host.nativeProbe && host.pageClick) clicked = await host.pageClick(x, y);
    await sleep(400);
    const after = (await snapshot())?.state ?? {};
    record({
      kind: 'probe',
      name,
      point: [x, y],
      page,
      native,
      clicked: clicked ?? native?.click ?? null,
      pointerEvents: state.pointerEvents,
      overlayPointerEvents: state.overlayPointerEvents,
      clicksBefore: state.clicks,
      clicksAfter: after.clicks,
    });
  }

  // ------------------------------------------------------------------ live
  async function live() {
    // The app started on the scenario's route (`--showcase-page`); never
    // presses an ad, never shows the reward video (that would be a scripted
    // play), only creates the interstitial on the live-perf page.
    if (scenario === 'live-perf') {
      await act('interstitial', 'interstitial-default', () => press('interstitial-default'));
    }
    const until = Date.now() + Number(cfg.liveObserveMs || 90000);
    while (Date.now() < until) {
      await sleep(5000);
      await step('live-observe');
    }
    if (host.probeGuests) await host.probeGuests('end').catch(() => undefined);
    await sleep(1500);
    const snap = await step('live-end');
    loaded = (snap?.counts?.display_ad_loaded ?? 0) >= 1;
  }
}

module.exports = {
  runSteps,
  SCENARIOS,
  LAYOUTS,
  RESTART_ROUTE,
  RESTART_AGAIN_ROUTE,
  RESTART_NEXT,
  restartPhaseOf,
};
