// Tests of parity-diff.mjs's normalisation and classification rules.
//
//   node --test parity-diff.test.mjs

import assert from 'node:assert/strict';
import { mkdtempSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { colourClass, compositeAt, sortKeys } from './lib/adformat-report.mjs';
import {
  callsDifferOnlyByPendingSystemInfo,
  classify,
  compareAdformats,
  consentDuringAttach,
  fillImpressions,
  GUEST_CREATE_MS,
  guestCreationSpread,
  guestVisibility,
  labelledUserAgent,
  loadedBefore,
  matchesOwnSpan,
  normalise,
  PACKAGE_RUNTIME_FILE,
  PACKAGE_RUNTIME_REQUEST,
  packageRuntimeLog,
  sameElement,
  sentOsClick,
  withoutPackageRuntime,
  withoutPointerEvents,
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
