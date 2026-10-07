// Tests of parity-diff.mjs's normalisation and classification rules.
//
//   node --test parity-diff.test.mjs

import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { classify, fillImpressions, labelledUserAgent, normalise } from './parity-diff.mjs';

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

test('the test-mode unit guard is a documented deviation', () => {
  const d = classify({
    section: 'guest',
    key: 'guest parity_400x600_1 __overwolf__.unit',
    field: 'value',
    electron: 'parity-unit',
    tauri: 'testAd',
  });
  assert.equal(d.class, 'intended:deviation');
  const live = classify({
    section: 'guest',
    key: 'guest parity_400x600_1 __overwolf__.unit',
    field: 'value',
    electron: 'parity-unit',
    tauri: '',
  });
  assert.equal(live.class, 'BUG');
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
