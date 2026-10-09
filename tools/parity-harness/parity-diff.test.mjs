// Tests of parity-diff.mjs's normalisation and classification rules.
//
//   node --test parity-diff.test.mjs

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { colourClass, compositeAt, sortKeys } from './lib/adformat-report.mjs';
import {
  callsDifferOnlyByPendingSystemInfo,
  classify,
  compareAdformats,
  compareIdentity,
  consentCookies,
  consentDuringAttach,
  consentGated,
  fewerPageReloads,
  fillImpressions,
  GUEST_CREATE_MS,
  guestCreationSpans,
  guestCreationSpread,
  guestVisibility,
  labelledUserAgent,
  loadedBefore,
  matchesOwnSpan,
  normalise,
  PACKAGE_RUNTIME_FILE,
  PACKAGE_RUNTIME_REQUEST,
  packageRuntimeLog,
  pageReloadRequests,
  reloadResends,
  removedUnfilled,
  briefHideDiffers,
  minimizeDismiss,
  modalPhaseDiffers,
  sameElement,
  scenarioMismatch,
  sentOsClick,
  shapingOf,
  withoutPackageRuntime,
  withoutPointerEvents,
  withoutResends,
  within,
  wireRequests,
} from './parity-diff.mjs';

test('normalise replaces host labels, versions and volatile values', () => {
  assert.equal(
    normalise('Counter?Name=tauri_app_start&owver=tauri-2_12_1'),
    'Counter?Name=<label>_app_start&owver=<owver>',
  );
  assert.equal(normalise('Name=electron_window_closed'), 'Name=<label>_window_closed');
  assert.equal(normalise('{"t":1791349527384}'), '{"t":<ts-ms>}');
});

test('a revalidation by the Chromium HTTP cache is intended (optimised)', () => {
  const headers = classify({
    section: 'host-request',
    field: 'header-order',
    conditionalOnly: true,
  });
  assert.equal(headers.class, 'intended:optimised');
  const status = classify({
    section: 'host-request',
    field: 'status',
    revalidated: true,
    electron: 'HTTP/1.1 304',
    tauri: 'HTTP/1.1 200',
  });
  assert.equal(status.class, 'intended:optimised');
  // Any other header order or status difference stays a bug.
  assert.equal(classify({ section: 'host-request', field: 'header-order' }).class, 'BUG');
  assert.equal(classify({ section: 'host-request', field: 'status' }).class, 'BUG');
});

test('cmp-eu-only racing the analytics sequence from main_ready is variance', () => {
  const row = classify({ section: 'host-request', field: 'order', cmpFirst: true });
  assert.equal(row.class, 'variance');
  assert.match(row.why, /both start at main_ready/);
  // Any other reordering of the host requests stays a bug.
  assert.equal(classify({ section: 'host-request', field: 'order' }).class, 'BUG');
});

test('an ad document Chromium served from its cache is variance', () => {
  assert.equal(
    classify({ section: 'ad-document', key: 'first load', field: 'from-http-cache' }).class,
    'variance',
  );
});

test('only <webview> own properties may be missing on <owadview>', () => {
  assert.equal(
    classify({ section: 'element-api', field: 'own', webviewOnly: true }).class,
    'intended:os-gap',
  );
  assert.equal(classify({ section: 'element-api', field: 'own', webviewOnly: false }).class, 'BUG');
});

test('a guest unit that differs is a bug in test mode too', () => {
  // ow-tauri forwards the element's unit in test mode as ow-electron does.
  const d = classify({
    section: 'guest',
    key: 'guest parity_400x600_1 __overwolf__.unit',
    field: 'value',
    electron: 'parity-unit',
    tauri: 'testAd',
  });
  assert.equal(d.class, 'BUG');
});

test('unknown differences are bugs', () => {
  const d = classify({ section: 'guest', key: 'x', field: 'value' });
  assert.equal(d.class, 'BUG');
  assert.equal(d.why, 'no documented reason');
});

test('missing consent messages are variance only when the consent came first', () => {
  const base = {
    section: 'host-message',
    key: 'guest parity_400x600_1',
    field: 'sequence',
    electron: ['consent', 'consent', 'eHashes'],
    tauri: ['eHashes'],
  };
  assert.equal(classify({ ...base, consentBeforeGuest: true }).class, 'variance');
  assert.equal(classify({ ...base, consentBeforeGuest: false }).class, 'BUG');
});

test('extra guest loads are variance only after the OS hid the document', () => {
  const base = { section: 'element-event', key: 'parity_400x600_0 dom-ready', field: 'count' };
  assert.equal(classify({ ...base, occludedReload: true }).class, 'variance');
  assert.equal(classify({ ...base, occludedReload: false }).class, 'BUG');
});

test('repeated impression requests count as the net log counts them', () => {
  const dir = mkdtempSync(join(tmpdir(), 'parity-diff-'));
  const imp =
    'https://analyticsnew.overwolf.com/analytics/Counter?Name=owads_scl_impression&Extra=x';
  // The later probe of guest 1 repeats the earlier one's request and adds one.
  writeFileSync(join(dir, 'guest-1-after-10s.json'), JSON.stringify({ labResources: [imp] }));
  writeFileSync(join(dir, 'guest-1-end.json'), JSON.stringify({ labResources: [imp, 'x', imp] }));
  writeFileSync(join(dir, 'guest-2-end.json'), JSON.stringify({ labResources: [imp] }));
  assert.equal(fillImpressions(dir), 3);
});

const ELECTRON_UA =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) MyApp/1.0.0 Chrome/148.0.7778.280 Electron/42.11.4 Safari/537.36';
const TAURI_UA =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) MyApp/1.0.0 Version/26.5 Tauri/2.12.1 Safari/605.1.15';

test('only a user agent that follows the labelling rule is the host label', () => {
  assert.equal(labelledUserAgent(ELECTRON_UA, TAURI_UA), true);
  const ua = (tauri) =>
    classify({ section: 'host-request', field: 'header:user-agent', electron: ELECTRON_UA, tauri })
      .class;
  assert.equal(ua(TAURI_UA), 'intended:host-label');
  // No app token, no Tauri token, a kept Electron token, another platform part.
  assert.equal(ua('curl/8.0'), 'BUG');
  assert.equal(ua(TAURI_UA.replace('MyApp/1.0.0 ', '')), 'BUG');
  assert.equal(ua(TAURI_UA.replace('Tauri/2.12.1', 'Tauri')), 'BUG');
  assert.equal(ua(`${TAURI_UA} Electron/42.11.4`), 'BUG');
  assert.equal(
    ua(TAURI_UA.replace('Macintosh; Intel Mac OS X 10_15_7', 'X11; Linux x86_64')),
    'BUG',
  );
  const guest = (tauri) =>
    classify({ section: 'guest', key: 'guest g', field: 'userAgent', electron: ELECTRON_UA, tauri })
      .class;
  assert.equal(guest(TAURI_UA), 'intended:os-gap');
  assert.equal(guest('Mozilla/5.0'), 'BUG');
});

test('ad events ow-tauri never reports are bugs, other ad counts variance', () => {
  const base = { section: 'element-event', key: 'parity_400x600_0 impression', field: 'count' };
  assert.equal(classify({ ...base, electron: 3, tauri: 5 }).class, 'variance');
  assert.equal(classify({ ...base, electron: 0, tauri: 2 }).class, 'variance');
  assert.equal(classify({ ...base, electron: 2, tauri: 0 }).class, 'BUG');
});

test('a host request without a response is a bug', () => {
  assert.equal(
    classify({ section: 'host-request', field: 'no-response', electron: 'HTTP/2 200', tauri: null })
      .class,
    'BUG',
  );
  // The protocol of a completed request the lab could not read is not mirrored.
  assert.equal(
    classify({ section: 'host-request', field: 'protocol', electron: 'h2', tauri: null }).class,
    'not-mirrored',
  );
});

// ---- ad formats (compareAdformats, lib/adformat-report.mjs) ----

/** Facts of one run with sensible empty defaults. */
const facts = (over = {}) => ({
  elements: {},
  oamOptions: {},
  mute: [],
  probes: {},
  clicks: { pointer: 0, received: 0, sent: 0 },
  bounds: [],
  front: false,
  ...over,
});
const element = (over = {}) => ({
  order: ['display_ad_loaded'],
  counts: { display_ad_loaded: 1 },
  payloadKeys: {},
  removalOrder: [],
  dom: {},
  ...over,
});
const diffFormats = (e, t) => {
  const out = [];
  compareAdformats({ host: 'electron', formats: e }, { host: 'tauri', formats: t }, out);
  return out.map((d) => ({ ...d, cls: classify(d).class }));
};

test('the same ad-format facts give no differences', () => {
  const f = facts({ elements: { a: element() }, oamOptions: { a: ['{"x":1}'] } });
  assert.deepEqual(diffFormats(f, f), []);
});

test('extra ad-driven events are variance, a lifecycle event ow-tauri never fires is a bug', () => {
  const e = facts({ elements: { a: element({ order: ['display_ad_loaded', 'video_ad_ready'] }) } });
  const t = facts({ elements: { a: element() } });
  const [d] = diffFormats(t, e);
  assert.equal(d.field, 'events');
  assert.equal(d.cls, 'variance');
  const [missing] = diffFormats(e, t);
  assert.equal(missing.cls, 'BUG');
  const destroyed = diffFormats(
    facts({ elements: { a: element({ order: ['display_ad_loaded', 'destroyed'] }) } }),
    t,
  );
  assert.equal(destroyed[0].cls, 'BUG');
});

test('removal order and DOM state before the first load are compared', () => {
  const e = facts({
    elements: {
      a: element({
        removalOrder: ['unmount', 'destroyed'],
        dom: { before: { display: 'inline-flex', pointerEvents: 'auto' } },
      }),
    },
  });
  const t = facts({
    elements: {
      a: element({
        removalOrder: ['unmount'],
        dom: { before: { display: 'block', pointerEvents: 'auto' } },
      }),
    },
  });
  const out = diffFormats(e, t);
  assert.deepEqual(out.map((d) => [d.field, d.cls]).sort(), [
    ['dom-before display', 'BUG'],
    ['removal', 'BUG'],
  ]);
});

test('ad library options on the wire must match as sets per element', () => {
  const out = diffFormats(
    facts({ oamOptions: { a: ['{"autoplay":true}'] } }),
    facts({ oamOptions: { a: ['{"autoplay":true,"customTracking":null}'] } }),
  );
  assert.equal(out.length, 1);
  assert.equal(out[0].section, 'adformat-options');
  assert.equal(out[0].key, 'a');
  assert.equal(out[0].cls, 'BUG');
  // An element one host has no option record for is not compared.
  assert.deepEqual(
    diffFormats(
      facts({ oamOptions: { a: ['{}'], b: ['{"x":1}'] } }),
      facts({ oamOptions: { a: ['{}'] } }),
    ),
    [],
  );
});

test('options compare by content at every depth, not by key order', () => {
  const a = JSON.stringify(sortKeys({ size: { width: 300, height: 250 }, autoplay: true }));
  const b = JSON.stringify(sortKeys({ autoplay: true, size: { height: 250, width: 300 } }));
  assert.equal(a, b);
  assert.match(a, /"height":250/);
});

test('mute sequences are compared per guest', () => {
  const out = diffFormats(facts({ mute: [[true, false]] }), facts({ mute: [[true]] }));
  assert.equal(out[0].section, 'adformat-mute');
  assert.equal(out[0].cls, 'BUG');
});

const probe = (points, performance = [{ pointerEvents: 'auto', connected: true }]) => ({
  points,
  performance,
});

test('probe routing and colour: container points are bugs, ad-content points variance', () => {
  const e = facts({
    probes: {
      loaded: probe({
        'reward-slot': { dom: 'ad-perf', colour: 'dark' },
        'std-slot': { dom: 'ad-perf', colour: 'light' },
        corner: { dom: 'ad-perf', colour: 'other' },
      }),
    },
  });
  const t = facts({
    probes: {
      loaded: probe({
        'reward-slot': { dom: 'ad-perf', native: 'ad', colour: 'red' },
        'std-slot': { dom: 'ad-perf', native: 'ad', colour: 'dark' },
        corner: { dom: 'ad-perf', native: 'app', colour: 'dark' },
      }),
    },
  });
  const got = Object.fromEntries(diffFormats(e, t).map((d) => [`${d.key} ${d.field}`, d.cls]));
  assert.deepEqual(got, {
    'loaded reward-slot colour': 'BUG',
    'loaded std-slot colour': 'variance',
    'loaded corner native-routing': 'BUG',
    'loaded corner colour': 'variance',
  });
});

test('performance pointer-events must match at every probe', () => {
  const out = diffFormats(
    facts({ probes: { early: probe({}, [{ pointerEvents: 'none' }]) } }),
    facts({ probes: { early: probe({}, [{ pointerEvents: 'auto' }]) } }),
  );
  assert.equal(out[0].field, 'performance pointer-events');
  assert.equal(out[0].cls, 'BUG');
});

test('a synthesized click WebKit never delivers is not mirrored; a lost real click is a bug', () => {
  const e = facts({ clicks: { pointer: 2, received: 2, sent: 2 } });
  const sent = diffFormats(e, facts({ clicks: { pointer: 0, received: 0, sent: 2 } }));
  assert.equal(sent[0].cls, 'not-mirrored');
  const lost = diffFormats(e, facts({ clicks: { pointer: 2, received: 1, sent: 2 } }));
  assert.equal(lost[0].cls, 'BUG');
});

test('an app that ever became frontmost is a bug on either host', () => {
  const out = diffFormats(facts(), facts({ front: true }));
  assert.equal(out[0].section, 'adformat-front');
  assert.equal(out[0].cls, 'BUG');
});

test('window_closed status missing from the ow-electron net log is variance', () => {
  const d = classify({
    section: 'host-request',
    key: 'GET https://analyticsnew.overwolf.com/analytics/Counter <label>_window_closed #9',
    field: 'status',
    electron: null,
    tauri: 'HTTP/1.1 200',
  });
  assert.equal(d.class, 'variance');
});

test('a request ow-electron never completed in its net log is variance', () => {
  const row = (field) => ({
    section: 'host-request',
    key: 'GET https://analyticsnew.overwolf.com/analytics/Counter <label>_app_start #2',
    field,
    electron: null,
    tauri: 'x',
    electronIncomplete: true,
  });
  for (const field of ['header-order', 'status', 'protocol']) {
    assert.equal(classify(row(field)).class, 'variance');
  }
  const { electronIncomplete: _, ...complete } = row('header-order');
  assert.equal(classify(complete).class, 'BUG');
});

test('a close counter length that is its own run span is variance', () => {
  const run = (shown, closed) => [
    { key: 'GET x <label>_app_heartbeat', at: 5 },
    { key: 'GET x <label>_app_heartbeat', at: shown },
    { key: 'GET x <label>_window_closed', at: closed },
  ];
  assert.equal(matchesOwnSpan(run(70_279, 118_729), 118_729, 48), true);
  assert.equal(matchesOwnSpan(run(1_331, 62_389), 62_389, 61), true);
  assert.equal(matchesOwnSpan(run(1_331, 62_389), 62_389, 48), false);
  assert.equal(matchesOwnSpan([{ key: 'GET x <label>_app_heartbeat', at: 5 }], 900, 1), false);
  const row = {
    section: 'host-request',
    key: 'GET x <label>_window_closed #10',
    field: 'Extra.length',
    electron: 48,
    tauri: 61,
  };
  assert.equal(classify({ ...row, ownSpans: true }).class, 'variance');
  assert.equal(classify(row).class, 'BUG');
});

test('colour classes and the ow-tauri source-over composite', () => {
  assert.equal(colourClass([0.92, 0.2, 0.14, 1]), 'red');
  assert.equal(colourClass([0.1, 0.1, 0.1, 1]), 'dark');
  assert.equal(colourClass([0.94, 0.94, 0.94, 1]), 'light');
  assert.equal(colourClass([0, 0, 0, 0]), 'clear');
  assert.equal(colourClass([0.5, 0.5, 0.9, 1]), 'other');
  const tauri = compositeAt({
    host: 'tauri',
    dom: { points: [{ name: 'p' }] },
    native: {
      order: [{ label: 'embedder' }, { label: 'overlay' }],
      webviews: {},
      snapshots: {
        embedder: { samples: [{ name: 'p', rgba: [1, 0, 0, 1] }] },
        overlay: { samples: [{ name: 'p', rgba: [0, 0, 0, 0.8] }] },
      },
    },
  });
  assert.equal(colourClass(tauri.p), 'dark');
  assert.ok(Math.abs(tauri.p[0] - 0.2) < 0.01);
  const electron = compositeAt({
    host: 'electron',
    snapshots: { embedder: { samples: [{ name: 'p', rgba: [0.2, 0, 0, 1] }] } },
  });
  assert.deepEqual(electron.p, [0.2, 0, 0, 1]);
});

test('a structure rect sampled on opposite sides of the high-impact expansion is variance', () => {
  const d = { section: 'element-structure', key: 'hi', field: 'rect', electron: {}, tauri: {} };
  assert.equal(classify({ ...d, zoneTiming: true }).class, 'variance');
  assert.equal(classify({ ...d, zoneTiming: false }).class, 'BUG');
});

test('a structure sample on opposite sides of the first ad load differs only in pointer-events', () => {
  const none = [
    ['id', 'ad0'],
    ['style', 'pointer-events: none;'],
  ];
  const auto = [
    ['id', 'ad0'],
    ['style', 'pointer-events: auto;'],
  ];
  assert.deepEqual(withoutPointerEvents(none), [['id', 'ad0']]);
  assert.deepEqual(withoutPointerEvents(auto), withoutPointerEvents(none));
  assert.deepEqual(withoutPointerEvents([['style', 'color: red; pointer-events: none;']]), [
    ['style', 'color: red;'],
  ]);
  const loaded = [{ cid: 'a', t: 100 }];
  assert.equal(loadedBefore(loaded, { cid: 'a', t: 99 }), false);
  assert.equal(loadedBefore(loaded, { cid: 'a', t: 100 }), true);
  assert.equal(loadedBefore(loaded, { cid: 'b', t: 500 }), false);
  const d = {
    section: 'element-structure',
    key: 'a',
    field: 'attributes',
    electron: none,
    tauri: auto,
  };
  assert.equal(classify({ ...d, loadTiming: true }).class, 'variance');
  assert.equal(classify({ ...d, loadTiming: false }).class, 'BUG');
});

test('a guest that attached between the two startup consent messages is variance', () => {
  assert.equal(consentDuringAttach(['consent'], ['consent', 'consent']), true);
  assert.equal(consentDuringAttach([], ['consent', 'consent', 'eHashes']), false);
  assert.equal(consentDuringAttach(['eHashes'], ['consent', 'consent', 'eHashes']), true);
  assert.equal(consentDuringAttach(['consent', 'x'], ['consent', 'y']), false);
  assert.equal(consentDuringAttach(['consent'], ['consent']), false);
  // Three leading consents is no startup consent pair.
  assert.equal(consentDuringAttach(['consent'], ['consent', 'consent', 'consent']), false);
  const d = { section: 'host-message', key: 'guest g', field: 'sequence' };
  assert.equal(classify({ ...d, consentDuringAttach: true }).class, 'variance');
  assert.equal(classify({ ...d, consentDuringAttach: false }).class, 'BUG');
});

test('an ow-tauri guest that missed the startup consent messages by the D.6.5 wait is intended', () => {
  // ow-electron's guest got both, ow-tauri's none: the documented wait.
  assert.equal(consentGated(['consent', 'consent'], []), true);
  assert.equal(consentGated(['consent', 'consent', 'eHashes'], ['eHashes']), true);
  assert.equal(consentGated(['consent', 'consent'], ['consent']), true);
  // ow-tauri's guest got more of them: timing, not the wait.
  assert.equal(consentGated(['consent'], ['consent', 'consent']), false);
  // Other differences stay a bug.
  assert.equal(consentGated(['consent', 'consent', 'x'], ['y']), false);
  const d = { section: 'host-message', key: 'guest g', field: 'sequence' };
  assert.equal(
    classify({ ...d, consentDuringAttach: true, consentGated: true }).class,
    'intended:deviation',
  );
  assert.equal(
    classify({ ...d, consentDuringAttach: true, consentGated: false }).class,
    'variance',
  );
});

test('guest names follow probe write order; states sent before the guest document are ignored', () => {
  const dir = mkdtempSync(join(tmpdir(), 'parity-vis-'));
  const probe = (cid) => JSON.stringify({ overwolf: { containerId: cid } });
  // guest-2 (webContents 3) was ready and probed first.
  writeFileSync(join(dir, 'guest-2-dom-ready-0.json'), probe('big'));
  const later = new Date(Date.now() + 1000);
  writeFileSync(join(dir, 'guest-1-dom-ready-0.json'), probe('small'));
  utimesSync(join(dir, 'guest-1-dom-ready-0.json'), later, later);
  writeFileSync(
    join(dir, 'events.jsonl'),
    [3, 4]
      .map((id) => JSON.stringify({ kind: 'guest-probe', webContentsId: id, label: 'dom-ready-0' }))
      .join('\n'),
  );
  const vis = (id, state, url) =>
    JSON.stringify({
      via: 'webContents._sendInternal',
      type: 'owadview',
      webContentsId: id,
      url,
      args: JSON.stringify(['GUEST_INSTANCE_VISIBILITY_CHANGE', state]),
    });
  writeFileSync(
    join(dir, 'ipc.jsonl'),
    [
      vis(3, 'visible', ''),
      vis(4, 'visible', ''),
      vis(4, 'hidden', ''),
      vis(3, 'visible', 'https://ad/'),
      vis(4, 'hidden', 'https://ad/'),
      vis(3, 'hidden', 'https://ad/'),
    ].join('\n'),
  );
  assert.deepEqual(guestVisibility(dir), { big: ['visible', 'hidden'], small: ['hidden'] });
});

test('extra unmute/mute pairs from extra video plays are variance, other mute differences bugs', () => {
  const played = (n) => ({ a: element({ counts: { play: n } }) });
  const more = diffFormats(
    facts({ elements: played(1), mute: [[true, false, true]] }),
    facts({ elements: played(2), mute: [[true, false, true, false, true]] }),
  );
  assert.equal(more[0].cls, 'variance');
  const samePlays = diffFormats(
    facts({ elements: played(1), mute: [[true, false, true]] }),
    facts({ elements: played(1), mute: [[true, false, true, false, true]] }),
  );
  assert.equal(samePlays[0].cls, 'BUG');
  const wrongState = diffFormats(
    facts({ elements: played(1), mute: [[true, false, true]] }),
    facts({ elements: played(2), mute: [[true, false]] }),
  );
  assert.equal(wrongState[0].cls, 'BUG');
});

test('a consent message only one host sent before the guest attached is variance', () => {
  const d = {
    section: 'host-message',
    key: 'guest g',
    field: 'sequence',
    electron: [],
    tauri: ['consent', 'consent'],
  };
  assert.equal(classify({ ...d, consentBeforeGuest: true }).class, 'variance');
  assert.equal(classify({ ...d, consentBeforeGuest: false }).class, 'BUG');
});

test('extra guest loads the ad page asked for after hidden are variance', () => {
  const d = {
    section: 'element-event',
    key: 'slot dom-ready',
    field: 'count',
    electron: 1,
    tauri: 2,
  };
  assert.equal(classify({ ...d, hiddenReload: true }).class, 'variance');
  assert.equal(classify({ ...d, hiddenReload: false }).class, 'BUG');
});

test("ow-electron's package manager traffic on Windows is a documented deviation", () => {
  for (const key of [
    'GET https://analyticsnew.overwolf.com/analytics/Counter electron_pm_launch',
    'GET https://analyticsnew.overwolf.com/analytics/Counter electron_pm_loaded',
    'GET https://analyticsnew.overwolf.com/analytics/Counter electron_cs_error',
    'POST https://tracking.overwolf.com/tracking/InsertStats 400029',
    'POST https://tracking.overwolf.com/tracking/InsertStats 400037',
    'POST https://tracking.overwolf.com/tracking/InsertStats 400043',
    'POST https://tracking.overwolf.com/tracking/InsertStats 400046',
  ])
    assert.ok(PACKAGE_RUNTIME_REQUEST.test(key), key);
  for (const key of [
    'POST https://tracking.overwolf.com/tracking/InsertStats 400025',
    'GET https://analyticsnew.overwolf.com/analytics/Counter electron_app_launch',
  ])
    assert.ok(!PACKAGE_RUNTIME_REQUEST.test(key), key);
  assert.ok(PACKAGE_RUNTIME_FILE.test('logs\\owpm.log'));
  assert.ok(PACKAGE_RUNTIME_FILE.test('logs/owpm.log'));
  assert.ok(!PACKAGE_RUNTIME_FILE.test('logs/main.log'));
  assert.deepEqual(withoutPackageRuntime({ firstLaunch: true, 'owepm.enabled': true }), {
    firstLaunch: true,
  });
  assert.equal(
    classify({ section: 'host-request', field: 'missing', packageRuntime: true }).class,
    'intended:deviation',
  );
  assert.equal(
    classify({ section: 'host-request', field: 'missing', packageRuntime: false }).class,
    'BUG',
  );
});

test('an ow-electron getSystemInformation() that answered {} is variance', () => {
  const info = { gpus: [], cpu: 'CPU', displays: [] };
  assert.ok(
    callsDifferOnlyByPendingSystemInfo(
      { getSystemInformation: {}, getCustomTracking: { a: 1 } },
      { getSystemInformation: info, getCustomTracking: { a: 1 } },
    ),
  );
  assert.ok(
    !callsDifferOnlyByPendingSystemInfo(
      { getSystemInformation: {}, getCustomTracking: { a: 1 } },
      { getSystemInformation: info, getCustomTracking: { a: 2 } },
    ),
  );
  assert.ok(
    !callsDifferOnlyByPendingSystemInfo(
      { getSystemInformation: { cpu: 'x' } },
      { getSystemInformation: info },
    ),
  );
  assert.equal(
    classify({ section: 'guest', field: 'value', systemInfoPending: true }).class,
    'variance',
  );
  assert.equal(
    classify({ section: 'guest', field: 'value', systemInfoPending: false }).class,
    'BUG',
  );
});

test('embedder focus after a system click into one host only is variance', () => {
  // Windows lab: the ow-tauri app gets a SendInput click (which activates
  // its window); ow-electron's click is a synthetic sendInputEvent.
  const dir = mkdtempSync(join(tmpdir(), 'os-click-'));
  const line = (e) => JSON.stringify(e) + '\n';
  writeFileSync(
    join(dir, 'events.jsonl'),
    line({ kind: 'hit-probe', click: { sent: true } }) +
      line({ kind: 'hit-probe', native: { click: { sent: false } } }),
  );
  assert.equal(sentOsClick(dir), false);
  writeFileSync(
    join(dir, 'events.jsonl'),
    line({ kind: 'hit-probe', native: { click: { sent: true, inputs: 3 } } }),
  );
  assert.equal(sentOsClick(dir), true);
  const focus = { section: 'guest', key: 'guest x __overwolf__.windowFocused', field: 'value' };
  assert.equal(classify({ ...focus, osClickFocus: true }).class, 'variance');
  assert.equal(classify({ ...focus, osClickFocus: false }).class, 'BUG');
});

test('a probe that carries its webContents id names its guest without file times', () => {
  // CI artifacts come back with one time on every file.
  const dir = mkdtempSync(join(tmpdir(), 'parity-vis-id-'));
  const probe = (cid, id) => JSON.stringify({ overwolf: { containerId: cid }, webContentsId: id });
  writeFileSync(join(dir, 'guest-1-dom-ready-0.json'), probe('std', 5));
  writeFileSync(join(dir, 'guest-2-dom-ready-0.json'), probe('reward', 4));
  writeFileSync(
    join(dir, 'events.jsonl'),
    [4, 5]
      .map((id) => JSON.stringify({ kind: 'guest-probe', webContentsId: id, label: 'dom-ready-0' }))
      .join('\n'),
  );
  const vis = (id, state) =>
    JSON.stringify({
      via: 'webContents._sendInternal',
      type: 'owadview',
      webContentsId: id,
      url: 'https://ad/',
      args: JSON.stringify(['GUEST_INSTANCE_VISIBILITY_CHANGE', state]),
    });
  writeFileSync(
    join(dir, 'ipc.jsonl'),
    [vis(4, 'visible'), vis(5, 'visible'), vis(4, 'hidden')].join('\n'),
  );
  assert.deepEqual(guestVisibility(dir), { reward: ['visible', 'hidden'], std: ['visible'] });
});

test("guest attach reports spread over the guests' creation are a platform gap", () => {
  // Windows lab (sizes): seven 400025 reports 70 ms apart on ow-electron and
  // 447 ms on ow-tauri, allowed 320 ms for a plain burst.
  const kinds = Array(7).fill('400025');
  assert.equal(guestCreationSpread(kinds, 447, 320), true);
  assert.equal(guestCreationSpread(kinds, 320 + GUEST_CREATE_MS * 6 + 1, 320), false);
  assert.equal(guestCreationSpread([...kinds.slice(1), '400023'], 447, 320), false);
  assert.equal(guestCreationSpread(['400025'], 447, 320), false);
  const d = {
    section: 'host-request',
    field: 'timing',
    guestCreation: true,
  };
  assert.equal(classify(d).class, 'intended:os-gap');
  assert.equal(classify({ ...d, guestCreation: false }).class, 'BUG');
});

test('element structure samples pair by cid, not by the order the pages reported them', () => {
  const a = { cid: 'parity_400x300_0', t: 10 };
  const b = { cid: 'parity_400x300_1', t: 9 };
  assert.equal(sameElement(a, [b, { ...a, t: 11 }]).t, 11);
  assert.equal(sameElement({ cid: 'other' }, [b]), b);
  assert.equal(sameElement({ cid: null }, [b, a]), b);
  assert.equal(sameElement(a, []), undefined);
});

test("ow-electron's log written only by the package manager is package runtime state", () => {
  // Windows lab (A): the package manager's remote config fetch timed out.
  const owpm = [
    "[2026-10-07 21:06:57.058] [info] ow-electron 42.11.4 session start - app 'Parity Harness' 0.1.0 - uid x - pid 1",
    "[2026-10-07 21:06:57.058] [error] [owpm] request 'https://example.invalid/config' timeout",
    '[2026-10-07 21:06:57.067] [error] [owpm] failed to get owepm remote config Error: abort',
    '    at ClientRequest.<anonymous> (node:electron/js2c/browser_init:2:94037)',
  ].join('\r\n');
  assert.equal(packageRuntimeLog(owpm), true);
  assert.equal(packageRuntimeLog(`${owpm}\r\n[2026-10-07 21:07:00.000] [error] [ads] boom`), false);
  assert.equal(packageRuntimeLog(owpm.split('\r\n')[0]), false);
  assert.equal(packageRuntimeLog(''), false);
  assert.equal(packageRuntimeLog(null), false);
});

test('a burst due while ow-tauri created guest webviews is variance', () => {
  // Windows lab (adstyle-probe): guests created from 3902 to 4999 ms; the
  // window-shown heartbeat left at 4308 ms and its 400023 at 5006 ms.
  const ev = (wall, kind, label) => ({ wall, kind, type: 'owadview', label });
  const spans = guestCreationSpans([
    ev(3902, 'created', 'g3'),
    ev(4034, 'created', 'g2'),
    ev(4162, 'created', 'g1'),
    ev(4302, 'transparent-native', 'g3'),
    ev(4302, 'created', 'g4'),
    ev(4484, 'created', 'g5'),
    ev(4997, 'transparent-native', 'g5'),
    ev(4998, 'created', 'g8'),
    ev(4999, 'transparent-native', 'g8'),
    ev(9000, 'created', 'later'),
    { wall: 4000, kind: 'created', type: 'cmp' },
  ]);
  assert.deepEqual(spans, [
    { start: 3902, end: 4999 },
    { start: 9000, end: 9000 },
  ]);
  assert.equal(within([4308, 5006], spans), true);
  assert.equal(within([4308, 5200], spans), false);
  assert.equal(within([1665, 1682], spans), false);
  assert.equal(within([], spans), false);
  const d = { section: 'host-request', field: 'timing', guestCreation: false };
  assert.equal(classify({ ...d, duringGuestCreation: true }).class, 'variance');
  assert.equal(classify({ ...d, duringGuestCreation: false }).class, 'BUG');
});

test("the guests' wire records give the request shaping to compare", () => {
  const lib =
    'https://content.overwolf.com/libs/ads/latest/owads.min.js?uid=u&phase=7&window=index';
  const records = [
    {
      method: 'Network.requestWillBeSent',
      requestId: '1',
      url: 'https://www.overwolf.com/monsdk/electron/latest/adview.html',
      resourceType: 'Document',
    },
    {
      method: 'Network.requestWillBeSentExtraInfo',
      requestId: '1',
      headers: { Referer: 'https://www.overwolf.com/u' },
    },
    {
      method: 'Network.requestWillBeSent',
      requestId: '2',
      url: 'https://ads.example/a.js?x=1',
      resourceType: 'Script',
    },
    {
      method: 'Network.requestWillBeSentExtraInfo',
      requestId: '2',
      headers: { Origin: 'https://www.overwolf.com' },
    },
    {
      method: 'Network.requestWillBeSent',
      requestId: '3',
      url: 'https://ads.example/b.png?y=2',
      resourceType: 'Image',
    },
    { method: 'Network.requestWillBeSentExtraInfo', requestId: '3', headers: {} },
    { method: 'Network.requestWillBeSentExtraInfo', requestId: '9', headers: {} },
  ];
  const netlog = [
    {
      url: lib,
      sentHeaders: [
        'Referer: https://www.overwolf.com/',
        'x-ow-uid: u',
        'x-ow-phase: 7',
        'x-ow-window: index',
      ],
    },
  ];
  const wire = wireRequests(records, netlog);
  assert.equal(wire.length, 4);
  const s = shapingOf(wire);
  assert.equal(s.subresources, 2);
  assert.equal(s.withOrigin, 1);
  assert.deepEqual(s.withoutOrigin, ['https://ads.example/b.png']);
  assert.deepEqual(s.adLibrary, {
    'x-ow-uid': 'u',
    'x-ow-phase': '7',
    'x-ow-window': 'index',
    origin: null,
    referer: 'https://www.overwolf.com/',
  });
  // A recorded ad library request wins over the net log.
  const own = wireRequests(
    [
      { method: 'Network.requestWillBeSent', requestId: '4', url: lib, resourceType: 'Script' },
      {
        method: 'Network.requestWillBeSentExtraInfo',
        requestId: '4',
        headers: { 'X-OW-UID': 'v' },
      },
    ],
    netlog,
  );
  assert.equal(shapingOf(own).adLibrary['x-ow-uid'], 'v');
  assert.equal(shapingOf([]).adLibrary, null);
  assert.equal(s.withoutOriginFirstHop, 1);
});

test('a redirect hop without Origin is the platform gap, a first request is a bug', () => {
  // Windows lab (A): a cookie-sync pixel redirected twice across origins;
  // the hops after the first carried Origin: null.
  const hop = (url) => [
    { method: 'Network.requestWillBeSent', requestId: '43', url, resourceType: 'Image' },
  ];
  const extra = (origin) => ({
    method: 'Network.requestWillBeSentExtraInfo',
    requestId: '43',
    headers: { Origin: origin },
  });
  const wire = wireRequests(
    [
      ...hop('https://a.example/p'),
      extra('https://www.overwolf.com'),
      ...hop('https://b.example/q'),
      extra('null'),
      ...hop('https://c.example/r'),
      extra('null'),
    ],
    [],
  );
  assert.deepEqual(
    wire.map((r) => [r.url, r.hop]),
    [
      ['https://a.example/p', 0],
      ['https://b.example/q', 1],
      ['https://c.example/r', 2],
    ],
  );
  const s = shapingOf(wire);
  assert.equal(s.withOrigin, 1);
  assert.equal(s.withoutOriginFirstHop, 0);
  assert.equal(
    classify({ section: 'request-shaping', field: 'redirect-hop' }).class,
    'intended:os-gap',
  );
  assert.equal(classify({ section: 'request-shaping', field: 'missing' }).class, 'BUG');
});

test('a zone the app removed before its ad loaded on ow-tauri is the consent wait, not a bug', () => {
  const el = (removedAfter, firstLoadAt) => ({ removedAfter, firstLoadAt });
  const e = { formats: { elements: { hi60: el(2791, 2716) } } };
  const removed = { formats: { elements: { hi60: el(6134, null) } } };
  const kept = { formats: { elements: { hi60: el(null, null) } } };
  const loaded = { formats: { elements: { hi60: el(6134, 6060) } } };
  assert.equal(removedUnfilled('hi60', e, removed), true);
  assert.equal(removedUnfilled('hi60', e, kept), false);
  assert.equal(removedUnfilled('hi60', e, loaded), false);
  assert.equal(removedUnfilled('other', e, removed), false);
  const count = {
    section: 'element-event',
    key: 'hi60 display_ad_loaded',
    field: 'count',
    electron: 2,
    tauri: 0,
  };
  assert.equal(classify({ ...count, removedUnfilled: true }).class, 'intended:deviation');
  assert.equal(classify({ ...count, removedUnfilled: false }).class, 'BUG');
  const events = {
    section: 'adformat-element',
    key: 'hi60',
    field: 'events',
    missing: ['display_ad_loaded'],
  };
  assert.equal(
    classify({ ...events, adDriven: true, removedUnfilled: true }).class,
    'intended:deviation',
  );
  assert.equal(classify({ ...events, adDriven: false, removedUnfilled: true }).class, 'BUG');
});

test('a probe that saw the performance modal loaded on one host only compares two phases', () => {
  const facts = (probeT, modalT) => ({ probes: { 'perf-loading': { t: probeT } }, modalT });
  // Windows lab: ow-electron's modal loaded 208 ms before its loading probe.
  assert.equal(modalPhaseDiffers(facts(6749, 6541), facts(9262, 12563), 'perf-loading'), true);
  assert.equal(modalPhaseDiffers(facts(6749, 7000), facts(9262, 12563), 'perf-loading'), false);
  assert.equal(modalPhaseDiffers(facts(6749, null), facts(9262, null), 'perf-loading'), false);
  assert.equal(modalPhaseDiffers(facts(6749, 6541), facts(9262, 12563), 'other'), false);
  const d = { section: 'adformat-probe', key: 'perf-loading control', field: 'page-routing' };
  assert.equal(classify({ ...d, modalPhaseDiffers: true }).class, 'variance');
  assert.equal(classify({ ...d, modalPhaseDiffers: false }).class, 'BUG');
});

test('a reward play after a brief hide measured differently is variance', () => {
  // Windows lab: a 50 ms hide read 59 ms on ow-electron, 44 ms on ow-tauri.
  const e = { hiddenSpans: { s1: [59], s2: [502] } };
  assert.equal(briefHideDiffers('s1', e, { hiddenSpans: { s1: [44] } }), true);
  assert.equal(briefHideDiffers('s1', e, { hiddenSpans: { s1: [61] } }), false);
  assert.equal(briefHideDiffers('s2', e, { hiddenSpans: { s2: [525] } }), false);
  assert.equal(briefHideDiffers('s1', e, { hiddenSpans: {} }), false);
  const d = { section: 'element-event', key: 's1 play', field: 'count', electron: 1, tauri: 0 };
  assert.equal(classify({ ...d, briefHide: true }).class, 'variance');
  assert.equal(classify({ ...d, briefHide: false }).class, 'BUG');
});

test('a performance dismiss after a minimize is variance, other events are not', () => {
  const run = { actions: [{ do: 'window', method: 'minimize' }] };
  const plain = { actions: [] };
  assert.equal(minimizeDismiss(['performance_ad_dismiss'], run, run), true);
  assert.equal(minimizeDismiss(['performance_ad_dismiss'], run, plain), false);
  assert.equal(minimizeDismiss(['performance_ad_dismiss', 'shutdown'], run, run), false);
  assert.equal(minimizeDismiss([], run, run), false);
  const count = {
    section: 'element-event',
    key: 'null performance_ad_dismiss',
    field: 'count',
    electron: 1,
    tauri: 0,
  };
  assert.equal(classify({ ...count, minimizeDismiss: true }).class, 'variance');
  assert.equal(classify({ ...count, minimizeDismiss: false }).class, 'BUG');
});

const REWARD_ONE_SLOT = { describe: 'reward, one slot, hide during play', steps: [{ at: 15000 }] };
const REWARD_TWO_SLOTS = { describe: 'reward, two slots', steps: [] };
const runMeta = (host, options) => ({ runId: `${host}-run`, host, options });

test('captures of different scenario definitions, layouts or modes are a mismatch', () => {
  const base = { scenarioDef: REWARD_ONE_SLOT, layouts: ['400x300'], mode: 'test' };
  assert.deepEqual(scenarioMismatch(runMeta('electron', base), runMeta('tauri', base)), []);
  assert.deepEqual(
    scenarioMismatch(
      runMeta('electron', {
        ...base,
        scenarioDef: REWARD_TWO_SLOTS,
        layouts: ['400x300', '400x600'],
      }),
      runMeta('tauri', base),
    ),
    ['scenarioDef', 'layouts'],
  );
  assert.deepEqual(
    scenarioMismatch(runMeta('electron', { ...base, mode: 'live' }), runMeta('tauri', base)),
    ['mode'],
  );
  // An older capture without the field is not compared on it.
  assert.deepEqual(
    scenarioMismatch(runMeta('electron', { mode: 'test' }), runMeta('tauri', base)),
    [],
  );
  assert.deepEqual(scenarioMismatch(null, runMeta('tauri', base)), []);
});

test('the CLI refuses a scenario mismatch unless it is allowed, and then flags it', () => {
  const root = mkdtempSync(join(tmpdir(), 'parity-diff-scenario-'));
  const capture = (name, meta) => {
    const dir = join(root, name);
    mkdirSync(dir);
    writeFileSync(join(dir, 'meta.json'), JSON.stringify(meta));
    return dir;
  };
  const base = { scenarioDef: REWARD_ONE_SLOT, layouts: ['400x300'], mode: 'test' };
  const e = capture('E', runMeta('electron', { ...base, scenarioDef: REWARD_TWO_SLOTS }));
  const t = capture('T', runMeta('tauri', base));
  const script = fileURLToPath(new URL('./parity-diff.mjs', import.meta.url));
  const refused = spawnSync(process.execPath, [script, e, t], { encoding: 'utf8' });
  assert.equal(refused.status, 2);
  assert.match(refused.stderr, /differ in scenarioDef/);
  assert.equal(existsSync(join(t, 'parity-diff.json')), false);

  const allowed = spawnSync(process.execPath, [script, e, t, '--allow-scenario-mismatch'], {
    encoding: 'utf8',
  });
  assert.notEqual(allowed.status, 2, allowed.stderr);
  assert.match(allowed.stdout, /Warning: the runs differ in scenarioDef/);
  assert.equal(existsSync(join(t, 'parity-diff.json')), true);
});

test('page reload requests count per element, a repeat within a second once', () => {
  const dir = mkdtempSync(join(tmpdir(), 'parity-reload-'));
  writeFileSync(
    join(dir, 'events.jsonl'),
    JSON.stringify({ kind: 'guest-probe', webContentsId: 3, label: 'dom-ready-0' }),
  );
  writeFileSync(
    join(dir, 'guest-1-dom-ready-0.json'),
    JSON.stringify({ overwolf: { containerId: 'slot' } }),
  );
  const ask = (t) =>
    JSON.stringify({
      t,
      dir: 'page->host',
      via: 'session-ipc-message',
      channel: 'GUEST_ADVIEW_RELOAD',
      webContentsId: 3,
      type: 'owadview',
    });
  writeFileSync(
    join(dir, 'ipc.jsonl'),
    [ask(37122), ask(44934), ask(59056), ask(59059)].join('\n'),
  );
  assert.deepEqual(pageReloadRequests(dir), { slot: 3 });

  const tauri = mkdtempSync(join(tmpdir(), 'parity-reload-'));
  const reload = (t, label) => JSON.stringify({ t, kind: 'reload', label, type: 'owadview' });
  writeFileSync(
    join(tauri, 'wc-events.jsonl'),
    [reload(57259, 'owad-1'), reload(123538, 'owad-1')].join('\n'),
  );
  assert.deepEqual(pageReloadRequests(tauri), { 'owad-1': 2 });
});

test('fewer guest loads are variance only when ow-tauri honoured every reload its page asked for', () => {
  const vis = { slot: ['visible', 'hidden', 'visible', 'hidden', 'visible'] };
  const e = {
    elementEvents: { 'slot dom-ready': 3 },
    visibility: vis,
    reloadRequests: { slot: 2 },
  };
  const t = {
    elementEvents: { 'slot dom-ready': 2 },
    visibility: vis,
    reloadRequests: { slot: 1 },
  };
  assert.equal(fewerPageReloads('slot', e, t), true);
  // ow-electron requests not recorded (older capture): its loads alone.
  assert.equal(fewerPageReloads('slot', { ...e, reloadRequests: {} }, t), true);
  // A request ow-tauri received and never loaded is a dropped reload.
  assert.equal(fewerPageReloads('slot', e, { ...t, reloadRequests: { slot: 2 } }), false);
  // The page was told something else: the host's visibility is suspect.
  assert.equal(
    fewerPageReloads('slot', e, { ...t, visibility: { slot: ['visible', 'hidden', 'visible'] } }),
    false,
  );
  // Never hidden: the page has no reason to ask.
  const shown = { slot: ['visible'] };
  assert.equal(
    fewerPageReloads('slot', { ...e, visibility: shown }, { ...t, visibility: shown }),
    false,
  );
  // ow-electron loads its requests do not account for.
  assert.equal(fewerPageReloads('slot', { ...e, reloadRequests: { slot: 1 } }, t), false);
  // Never loaded on ow-tauri, or not fewer.
  assert.equal(fewerPageReloads('slot', e, { ...t, elementEvents: {} }), false);
  assert.equal(fewerPageReloads('slot', t, t), false);

  const d = {
    section: 'element-event',
    key: 'slot dom-ready',
    field: 'count',
    electron: 3,
    tauri: 2,
  };
  assert.equal(classify({ ...d, fewerPageReloads: true }).class, 'variance');
  assert.equal(classify({ ...d, fewerPageReloads: false }).class, 'BUG');
});

test('customTracking re-sends that follow extra page reloads are variance', () => {
  const vis = { slot: ['visible', 'hidden', 'visible', 'hidden', 'visible', 'hidden'] };
  const e = { elementEvents: { 'slot dom-ready': 4 }, visibility: vis, reloadRequests: {} };
  const t = {
    elementEvents: { 'slot dom-ready': 3 },
    visibility: vis,
    reloadRequests: { slot: 2 },
  };
  const at = [
    'consent',
    'customTracking',
    'eHashes',
    'customTracking',
    'customTracking',
    'window-hidden',
    'customTracking',
  ];
  const bt = [
    'consent',
    'customTracking',
    'eHashes',
    'customTracking',
    'window-hidden',
    'customTracking',
  ];
  assert.deepEqual(withoutResends(bt), ['consent', 'eHashes', 'window-hidden']);
  assert.equal(reloadResends('slot', at, bt, e, t), true);
  // One re-send more than the load difference.
  assert.equal(reloadResends('slot', [...at, 'customTracking'], bt, e, t), false);
  // A load ow-tauri dropped explains nothing.
  assert.equal(reloadResends('slot', at, bt, e, { ...t, reloadRequests: { slot: 3 } }), false);
  // Extra ow-tauri loads count only up to the reloads its page asked for after hidden.
  assert.equal(reloadResends('slot', bt, at, t, { ...e, pageReloads: { slot: 1 } }), true);
  assert.equal(reloadResends('slot', bt, at, t, { ...e, pageReloads: {} }), false);
  // Same count: nothing to set aside.
  assert.equal(reloadResends('slot', at, at, e, t), false);

  const d = {
    section: 'host-message',
    key: 'guest slot',
    field: 'sequence',
    electron: at,
    tauri: bt,
  };
  assert.equal(classify({ ...d, reloadResends: true }).class, 'variance');
  assert.equal(classify({ ...d, reloadResends: false }).class, 'BUG');
});

test("a consent cookie's lifetime counts from when it was recorded, not from the diff", () => {
  const dir = mkdtempSync(join(tmpdir(), 'parity-cookies-'));
  const startedAt = '2020-01-01T00:00:00.000Z';
  const start = Date.parse(startedAt);
  const year = 365 * 86_400_000;
  const cookie = (name, at) => ({
    name,
    domain: '.overwolf.com',
    path: '/',
    expirationDate: (at + year) / 1000,
  });
  writeFileSync(
    join(dir, 'cookie-changes.jsonl'),
    [
      // ow-electron: `t` only (ms after the run started).
      { t: 5000, cause: 'explicit', removed: false, cookie: cookie('euconsent-v2', start + 5000) },
      // ow-tauri: `wall` (Unix ms of the read).
      { t: 9000, wall: start + 7000, removed: false, cookie: cookie('acconsent', start + 7000) },
    ]
      .map((e) => JSON.stringify(e))
      .join('\n') + '\n',
  );
  const got = consentCookies(dir, startedAt);
  assert.deepEqual(
    got.map((c) => [c.name, c.lifetimeDays]),
    [
      ['euconsent-v2', 365],
      ['acconsent', 365],
    ],
  );
  // Without the run's start, a `t`-only record still measures from now
  // (the old reading): years later the lifetime has run out.
  assert.ok(consentCookies(dir)[0].lifetimeDays < 0);
});

test('a Tauri-native identity compares the data members only, and reports the API surface', () => {
  const electron = {
    overwolf: {
      appName: 'App',
      appVersion: '1.0.0',
      platform: 'darwin',
      arch: 'arm64',
      snapshots: [
        {
          label: 'module-load',
          env: { OVERWOLF_APP_UID: 'uid-1' },
          members: {
            __settings__: { type: 'object', value: { a: 1 } },
            disableAdsFPD: { type: 'function', length: 0 },
            muid: { type: 'string', value: 'm-1' },
            phasePercent: { type: 'number', value: 24 },
            uid: { type: 'string', value: 'uid-1' },
            utmParams: { type: 'undefined', value: null },
          },
        },
        { label: 'after disableAdsFPD', changed: { 'members.__settings__': { a: 2 } } },
      ],
      calls: [],
    },
  };
  const tauri = (uid) => ({
    actions: [],
    overwolf: {
      appName: 'App',
      appVersion: '1.0.0',
      platform: 'darwin',
      arch: 'arm64',
      apiSurface: ['disableAdsFPD', 'getInfo'],
      snapshots: [
        {
          label: 'module-load',
          surface: 'tauri-plugin-overwolf-api',
          env: {},
          members: {
            muid: { type: 'string', value: 'm-1' },
            phasePercent: { type: 'number', value: 24 },
            uid: { type: 'string', value: uid },
            utmParams: { type: 'undefined', value: null },
          },
        },
        { label: 'after disableAdsFPD', changed: {} },
      ],
      calls: [],
    },
  });
  const rows = [];
  compareIdentity(electron, tauri('uid-1'), rows);
  const compared = rows.filter((r) => !r.informational && r.field !== 'versions');
  assert.deepEqual(compared, []);
  assert.deepEqual(
    rows.filter((r) => r.informational).map((r) => r.key),
    ['api surface', 'env', 'process.versions'],
  );
  assert.deepEqual(rows.find((r) => r.key === 'api surface').tauri, ['disableAdsFPD', 'getInfo']);
  // A different uid is still a difference.
  const other = [];
  compareIdentity(electron, tauri('uid-2'), other);
  assert.deepEqual(
    other.filter((r) => !r.informational).map((r) => r.key),
    ['app.overwolf.uid'],
  );
  // An ow-electron-shaped snapshot (no surface) keeps the full comparison.
  const legacy = tauri('uid-1');
  delete legacy.overwolf.snapshots[0].surface;
  const full = [];
  compareIdentity(electron, legacy, full);
  assert.ok(full.some((r) => r.key === 'env' && !r.informational));
  assert.ok(full.some((r) => r.key === 'app.overwolf' && r.field === 'members'));
});
