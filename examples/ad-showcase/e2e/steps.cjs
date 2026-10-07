// The lab steps, shared by both hosts (tauri-driver.js on ow-tauri,
// electron-main.cjs on ow-electron). They run in the main process and drive
// the window only through `webContents.executeJavaScript`: pressing the
// showcase's own buttons (by their `data-action`) and reading
// `window.__showcase`. They never touch an ad, never send input into an ad
// guest and never press Restart.
//
// host = {
//   name: 'tauri' | 'electron',
//   config: { steps: 'smoke' | 'tour', adWaitMs, dwellMs },
//   record(entry),            // one line of e2e.jsonl
//   mainWindow(),             // the showcase BrowserWindow, or null
//   quit(),
// }

'use strict';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Pages in sidebar order, with the actions the tour presses on each. */
const TOUR = [
  { id: 'sizes', actions: [] },
  { id: 'layouts', actions: ['layout-recreate'] },
  { id: 'high-impact', actions: [] },
  {
    id: 'interstitial',
    actions: ['interstitial-click-me', 'interstitial-default', 'interstitial-default'],
  },
  { id: 'reward', actions: [] },
  { id: 'house', actions: [] },
  {
    id: 'controls',
    actions: [
      'controls-tracking-apply',
      'controls-mute',
      'controls-display',
      'controls-display',
      'controls-scroll-out',
      'controls-scroll-back',
    ],
  },
  { id: 'consent', actions: ['consent-cmp-required', 'consent-email-hashes'] },
  { id: 'parity', actions: [] },
];

async function runSteps(host) {
  const t0 = Date.now();
  const cfg = host.config || {};
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
  const snapshot = () =>
    exec('JSON.stringify(window.__showcase ? window.__showcase.snapshot() : null)').then(
      JSON.parse,
    );
  const newEntries = async () => {
    const rows = JSON.parse(
      await exec(`JSON.stringify(window.__showcase ? window.__showcase.entries(${cursor}) : [])`),
    );
    cursor += rows.length;
    return rows;
  };
  const press = (action) =>
    exec(
      `(() => { const b = document.querySelector('[data-action=${JSON.stringify(action)}]'); if (!b) return 'missing'; if (b.disabled) return 'disabled'; b.click(); return 'pressed'; })()`,
    );
  const step = async (name, extra = {}) => {
    record({
      kind: 'step',
      name,
      snapshot: await snapshot(),
      events: await newEntries(),
      ...extra,
    });
  };

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
  await step('started');

  // 2. Page 1 renders and an ad loads (test mode).
  let loaded = false;
  const until = Date.now() + adWaitMs;
  while (Date.now() < until) {
    const snap = await snapshot();
    if (snap && snap.counts && snap.counts.display_ad_loaded >= 1) {
      loaded = true;
      break;
    }
    await sleep(500);
  }
  await step('page-1', { displayAdLoaded: loaded });

  // 3. The tour: every page and its buttons (next lane; not part of smoke).
  if (cfg.steps === 'tour') {
    for (const page of TOUR) {
      record({ kind: 'action', page: page.id, result: await press(`nav-${page.id}`) });
      await sleep(dwellMs);
      for (const action of page.actions) {
        record({ kind: 'action', page: page.id, action, result: await press(action) });
        await sleep(1500);
      }
      await step(`page-${page.id}`);
    }
  }

  record({ kind: 'done', displayAdLoaded: loaded });
  host.quit();
}

module.exports = { runSteps, TOUR };
