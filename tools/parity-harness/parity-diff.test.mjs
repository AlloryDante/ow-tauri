// Tests of parity-diff.mjs's normalisation and classification rules.
//
//   node --test parity-diff.test.mjs

import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { classify, fillImpressions, normalise } from './parity-diff.mjs';

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
