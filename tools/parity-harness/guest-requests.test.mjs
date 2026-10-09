// Tests of parity-diff.mjs's guest request (CONTRACT E.5) and gesture
// (DESIGN §5.2 #12) comparators.
//
//   node --test guest-requests.test.mjs

import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import {
  classify,
  compareGestures,
  compareGuestRequests,
  gestureCases,
  guestRequestFacts,
  guestRequestOf,
  guestRequests,
} from './parity-diff.mjs';

/** A Counter URL as the ad page sends it (Extra: JSON, URL-encoded twice). */
function counter(name, adUid = '1791511657125_173275458') {
  const extra = encodeURIComponent(JSON.stringify({ sdk_ver: '2.312.3', ad_uid: adUid }));
  return `https://analyticsnew.overwolf.com/analytics/Counter?CurrentVersion=x&Name=${name}&Extra=${encodeURIComponent(extra)}`;
}

const STATS = 'https://analyticsnew.overwolf.com/tracking/InsertStats?Stats=true';

/** The requests of `n` documents, each with its own session id. */
function documents(n, extra = []) {
  const out = [];
  for (let i = 0; i < n; i++) {
    const uid = `179151165712${i}_${i + 1}`;
    for (const name of ['owads_first_load', 'owads_oam_path', 'oam_first_load'])
      out.push(guestRequestOf(counter(name, uid)));
  }
  // The other names belong to the first document.
  return [...out, ...extra.map((u) => ({ ...guestRequestOf(u), session: '1791511657120_1' }))];
}

function run(requests, more = {}) {
  return { guestRequests: { source: 'test', requests }, elementEvents: {}, actions: [], ...more };
}

function diff(e, t) {
  const out = [];
  compareGuestRequests(e, t, out);
  return out.filter((d) => d.field !== 'recorded').map((d) => ({ ...d, cls: classify(d).class }));
}

test('guestRequestOf reads the Counter name, the session id and InsertStats kinds', () => {
  assert.deepEqual(guestRequestOf(counter('owads_first_load')), {
    name: 'owads_first_load',
    session: '1791511657125_173275458',
  });
  assert.deepEqual(guestRequestOf(STATS, '{"Kind":400051,"Extra":"x"}'), { stats: 400051 });
  assert.deepEqual(guestRequestOf(STATS), { stats: null });
  assert.equal(guestRequestOf(counter('electron_cmp_accept_full_launch')), null);
  assert.equal(guestRequestOf('https://track1.aniview.com/track?r=1'), null);
  assert.equal(guestRequestOf('not a url'), null);
});

test('guestRequestFacts counts names, documents, session ids', () => {
  const f = guestRequestFacts([
    ...documents(2),
    guestRequestOf(STATS, '{"Kind":400051}'),
    { name: 'oam_fpid', session: null },
    { name: 'oam_fpid', session: 'bad' },
  ]);
  assert.equal(f.loads, 2);
  assert.equal(f.stats, 1);
  assert.deepEqual(f.kinds, [400051]);
  assert.equal(f.sessions, 3);
  assert.equal(f.unsessioned, 1);
  assert.equal(f.malformed, 1);
});

test('the same stream gives no rows', () => {
  assert.deepEqual(diff(run(documents(2)), run(documents(2))), []);
});

test('a per-document or strict name one host does not send is a BUG', () => {
  const rows = diff(
    run(documents(2, [counter('owads_shutdown'), counter('oam_general_error')])),
    run(documents(2)),
  );
  assert.deepEqual(
    rows.map((r) => [r.key, r.field, r.cls]),
    [
      ['oam_general_error', 'missing', 'BUG'],
      ['owads_shutdown', 'missing', 'BUG'],
    ],
  );
  const perDoc = diff(run(documents(2)), run(documents(2, [counter('owads_oam_path')])));
  assert.deepEqual(
    perDoc.map((r) => [r.key, r.field, r.cls]),
    [['owads_oam_path', 'per-document', 'BUG']],
  );
});

test('ad-driven counts are variance', () => {
  const rows = diff(
    run(documents(1, [counter('oam_provider_loaded')])),
    run(documents(1, [counter('oam_provider_loaded'), counter('oam_provider_loaded')])),
  );
  assert.deepEqual(
    rows.map((r) => [r.key, r.cls]),
    [['oam_provider_loaded', 'variance']],
  );
});

test('more documents are variance only as the dom-ready counts differ', () => {
  const dom = (n) => ({ elementEvents: { 'parity_300x250_0 dom-ready': n } });
  const e = run(documents(2, [counter('owads_ad_container_duration')]), dom(2));
  const t = run(
    documents(3, [counter('owads_ad_container_duration'), counter('owads_ad_container_duration')]),
    dom(3),
  );
  assert.deepEqual(
    diff(e, t).map((r) => [r.key, r.field, r.cls]),
    [
      ['owads_ad_container_duration', 'count', 'variance'],
      ['owads_first_load', 'documents', 'variance'],
    ],
  );
  // The element events saw as many documents: the ad page's stream differs.
  const same = run(t.guestRequests.requests, dom(2));
  assert.deepEqual(
    diff(e, same).map((r) => [r.key, r.cls]),
    [
      ['owads_ad_container_duration', 'BUG'],
      ['owads_first_load', 'BUG'],
    ],
  );
});

test('InsertStats posts and session ids are compared', () => {
  const stats = guestRequestOf(STATS, '{"Kind":400051}');
  assert.deepEqual(
    diff(run([...documents(1), stats]), run(documents(1))).map((r) => [r.key, r.cls]),
    [['InsertStats Kind 400051', 'BUG']],
  );
  const shared = documents(2).map((r) => ({ ...r, session: '1791511657125_1' }));
  assert.deepEqual(
    diff(run(documents(2)), run(shared)).map((r) => [r.key, r.field, r.cls]),
    [['session ids', 'per-document', 'BUG']],
  );
  const missing = documents(2).map((r) => ({ ...r, session: null }));
  assert.ok(
    diff(run(documents(2)), run(missing)).some(
      (r) => r.field === 'requests without ad_uid' && r.cls === 'BUG',
    ),
  );
  const malformed = documents(2).map((r, i) => ({ ...r, session: `x${i}` }));
  assert.ok(diff(run(documents(2)), run(malformed)).some((r) => r.field === 'format'));
});

test('rows from the guest probes are marked partial and never BUG', () => {
  const out = [];
  compareGuestRequests(
    run(documents(2, [counter('owads_shutdown')])),
    { ...run(documents(2)), guestRequests: { source: 'probes', requests: documents(2) } },
    out,
  );
  const rows = out.filter((d) => d.field !== 'recorded');
  assert.equal(rows.length, 1);
  assert.equal(classify(rows[0]).class, 'not-mirrored');
});

test('a run without a record gives one informational row', () => {
  const out = [];
  compareGuestRequests(run(documents(1)), { guestRequests: { source: null, requests: [] } }, out);
  assert.equal(out.length, 1);
  assert.equal(out[0].informational, true);
});

test('guestRequests reads each host’s record', () => {
  const dir = mkdtempSync(join(tmpdir(), 'gr-'));
  writeFileSync(
    join(dir, 'netlog-requests.json'),
    JSON.stringify([
      { url: counter('owads_first_load'), initiator: 'https://www.overwolf.com' },
      { url: counter('owads_first_load'), initiator: 'not an origin' },
      {
        url: STATS,
        initiator: 'https://www.overwolf.com',
        uploadBody: { text: '{"Kind":400051}' },
      },
    ]),
  );
  const e = guestRequests(dir, 'electron');
  assert.equal(e.source, 'netlog');
  assert.equal(e.requests.length, 2);
  const t = mkdtempSync(join(tmpdir(), 'gr-'));
  assert.equal(guestRequests(t, 'tauri').source, null);
  writeFileSync(
    join(t, 'guest-1-end.json'),
    JSON.stringify({ labResources: [counter('owads_first_load')] }),
  );
  assert.equal(guestRequests(t, 'tauri').source, 'probes');
  writeFileSync(
    join(t, 'guest-network.jsonl'),
    [1, 1, 2]
      .map((id) => JSON.stringify({ label: 'owad-1', requestId: id, url: counter('oam_fpid') }))
      .join('\n'),
  );
  const wire = guestRequests(t, 'tauri');
  assert.equal(wire.source, 'guest-network');
  assert.equal(wire.requests.length, 2);
  writeFileSync(
    join(t, 'guest-requests.jsonl'),
    JSON.stringify({ label: 'owad-1', url: counter('oam_fpid') }),
  );
  assert.equal(guestRequests(t, 'tauri').source, 'guest-requests');
});

/** A gesture-timing capture: one case per `[id, kind, result]`, opens per host. */
function gestureDir(host, cases, opens) {
  const dir = mkdtempSync(join(tmpdir(), 'gs-'));
  const lines = cases.flatMap(([id, caseKind, result]) => [
    {
      kind: 'gesture-case',
      id,
      caseKind,
      delay: 0,
      input: 'mouse',
      url: `owparity-canary://case-${id}`,
    },
    { kind: 'fixture-log', id, log: [{ ev: 'action', result }] },
  ]);
  writeFileSync(join(dir, 'click-outs.jsonl'), lines.map((l) => JSON.stringify(l)).join('\n'));
  const urls = opens.map((id) => `owparity-canary://case-${id}`);
  if (host === 'tauri')
    writeFileSync(
      join(dir, 'wc-events.jsonl'),
      urls.map((url) => JSON.stringify({ kind: 'open-external', label: 'owad-1', url })).join('\n'),
    );
  else
    writeFileSync(
      join(dir, 'open-external.jsonl'),
      urls.map((url) => JSON.stringify({ method: 'openExternal', args: [url] })).join('\n'),
    );
  return gestureCases(dir, host);
}

function gestureDiff(e, t, actions = []) {
  const out = [];
  compareGestures({ gestures: e }, { gestures: t, actions }, out);
  return out.map((d) => ({ ...d, cls: classify(d).class }));
}

test('gestureCases counts the external opens of each case', () => {
  const g = gestureDir(
    'electron',
    [
      [1, 'open', 'null'],
      [2, 'open-late', 'null'],
    ],
    [1, 1],
  );
  assert.deepEqual(
    g.map((c) => [c.id, c.opens, c.pageResult]),
    [
      [1, 2, ['null']],
      [2, 0, ['null']],
    ],
  );
  assert.equal(gestureDir('tauri', [[1, 'open', 'null']], [1])[0].opens, 1);
});

test('the same gesture outcomes give no rows', () => {
  const cases = [
    [1, 'open', 'null'],
    [2, 'open-late', 'null'],
  ];
  assert.deepEqual(
    gestureDiff(gestureDir('electron', cases, [1]), gestureDir('tauri', cases, [1])),
    [],
  );
});

test('an open without activation, a missing open or another page result is a BUG', () => {
  const cases = [
    [1, 'open', 'null'],
    [2, 'open-late', 'null'],
  ];
  const rows = gestureDiff(
    gestureDir('electron', cases, [1]),
    gestureDir(
      'tauri',
      [
        [1, 'open', 'window'],
        [2, 'open-late', 'null'],
      ],
      [2],
    ),
  );
  assert.deepEqual(
    rows.map((r) => [r.key, r.field, r.cls]),
    [
      ['case 1 open', 'opens', 'BUG'],
      ['case 1 open', 'page-result', 'BUG'],
      ['case 2 open-late', 'opens', 'BUG'],
    ],
  );
});

test('a case ow-tauri did not run is a BUG unless the action was unsupported', () => {
  const e = gestureDir('electron', [[1, 'open', 'null']], [1]);
  assert.deepEqual(
    gestureDiff(e, []).map((r) => [r.field, r.cls]),
    [['missing', 'BUG']],
  );
  assert.deepEqual(gestureDiff(e, [], [{ phase: 'action-unsupported', do: 'gesture-case' }]), []);
});
