// node --test windows-lab.test.mjs
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, test } from 'node:test';

import { classCounts, durationOf, LAB_SCENARIOS, shard, stateDirs } from './ci/windows-lab.mjs';
import { adformatFacts, compositeAt, guestMuted } from './lib/adformat-report.mjs';
import { audioChecks, labLayersChecks, topLabel, windowsChecks } from './lib/windows-checks.mjs';

const root = mkdtempSync(join(tmpdir(), 'windows-lab-test-'));
after(() => rmSync(root, { recursive: true, force: true }));

function run(name, files) {
  const dir = join(root, name);
  mkdirSync(dir, { recursive: true });
  for (const [file, lines] of Object.entries(files)) {
    writeFileSync(join(dir, file), lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  }
  return dir;
}

const RED = [1, 0, 0, 1];
const WHITE = [1, 1, 1, 1];
const DARK = [0.2, 0.2, 0.2, 1];

/** An ow-electron hit probe: the composed window's samples. */
const eProbe = (label, colours, extra = {}) => ({
  kind: 'hit-probe',
  label,
  host: 'electron',
  dom: { points: Object.keys(colours).map((name) => ({ name, x: 1, y: 1 })) },
  snapshots: {
    embedder: { samples: Object.entries(colours).map(([name, rgba]) => ({ name, rgba })) },
  },
  ...extra,
});

/** An ow-tauri hit probe on Windows (native_win.rs shape). */
const tProbe = (label, colours, native = {}) => ({
  kind: 'hit-probe',
  label,
  host: 'tauri',
  dom: { points: Object.keys(colours).map((name) => ({ name, x: 1, y: 1 })) },
  native: {
    platform: 'windows',
    order: [{ label: 'bw-1' }, { label: 'owad-bw-1-1' }, { label: 'owad-bw-1-3' }],
    webviews: {
      'bw-1': { region: { kind: 'none' }, muted: false },
      'owad-bw-1-1': { region: { kind: 'none' }, backgroundArgb: [0, 255, 255, 255] },
      'owad-bw-1-3': { region: { kind: 'none' }, backgroundArgb: [0, 255, 255, 255] },
    },
    hits: [],
    composite: {
      print: { samples: Object.entries(colours).map(([name, rgba]) => ({ name, rgba })) },
    },
    ...native,
  },
});

test('the composed Windows window copy is the colour a viewer sees', () => {
  const probe = tProbe('early', { 'reward-slot': RED });
  assert.deepEqual(compositeAt(probe), { 'reward-slot': RED });
  // macOS-style per-webview snapshots still composite bottom to top.
  const mac = {
    host: 'tauri',
    dom: { points: [{ name: 'p' }] },
    native: {
      order: [{ label: 'bw-1' }, { label: 'g' }],
      snapshots: {
        'bw-1': { samples: [{ name: 'p', rgba: RED }] },
        g: { samples: [{ name: 'p', rgba: [0, 0, 0, 0] }] },
      },
    },
  };
  assert.deepEqual(compositeAt(mac), { p: [1, 0, 0, 1] });
});

test('guest mute states come from isAudioMuted() and from WebView2 IsMuted', () => {
  assert.deepEqual(guestMuted({ host: 'electron', guestMuted: [true, false] }), [false, true]);
  assert.equal(guestMuted({ host: 'electron' }), null);
  const t = tProbe('m', {});
  t.native.webviews['owad-bw-1-1'].muted = true;
  t.native.webviews['owad-bw-1-3'].muted = false;
  assert.deepEqual(guestMuted(t), [false, true]);
  // macOS has no native reading: no comparison.
  assert.equal(guestMuted({ host: 'tauri', native: { webviews: { g: {} } } }), null);
});

test('lab-layers: L1-W, L2 and L3-W pass on a transparent, raised, pass-through guest', () => {
  const e = run('e-ok', {
    'events.jsonl': [
      eProbe('early', { 'reward-slot': RED }),
      eProbe('before-perf', { 'reward-slot': RED }),
      eProbe('perf-loaded', { control: DARK }),
    ],
  });
  const loadingNative = {
    webviews: {
      'bw-1': { region: { kind: 'none' } },
      'owad-bw-1-3': { region: { kind: 'empty' } },
    },
    hits: [{ name: 'control', target: { label: 'bw-1' } }],
    click: { name: 'control', sent: true },
  };
  const loadedNative = {
    webviews: { 'owad-bw-1-3': { region: { kind: 'none' } } },
    hits: [{ name: 'control', target: { label: 'owad-bw-1-3' } }],
    click: { name: 'control', sent: false, refused: 'the click would not reach the app webview' },
  };
  const t = run('t-ok', {
    'events.jsonl': [
      tProbe('early', { 'reward-slot': RED }),
      tProbe('before-perf', { 'reward-slot': RED }),
      tProbe('perf-loading', {}, loadingNative),
      tProbe('perf-loaded', { control: DARK }, loadedNative),
      tProbe('after-remount', {}),
    ],
    'wc-events.jsonl': [
      { kind: 'passthrough', label: 'owad-bw-1-3', on: true },
      { kind: 'passthrough', label: 'owad-bw-1-3', on: false },
    ],
    'page-events.jsonl': [{ kind: 'app-click' }],
  });
  const checks = labLayersChecks(e, t);
  assert.deepEqual(
    checks.map((c) => [c.id, c.probe, c.pass]),
    [
      ['L1-W', 'early reward-slot', true],
      ['L1-W', 'before-perf reward-slot', true],
      ['L1-W', 'perf-loaded control', true],
      ['L2', 'perf-loading', true],
      ['L2', 'perf-loaded', true],
      ['L2', 'after-remount', true],
      ['L3-W', 'perf-loading', true],
      ['L3-W', 'perf-loaded', true],
    ],
  );
  assert.deepEqual(windowsChecks('lab-layers', e, t), checks);
});

test('lab-layers: an opaque guest, a buried guest and a lost click fail', () => {
  const e = run('e-bad', {
    'events.jsonl': [eProbe('early', { 'reward-slot': RED })],
  });
  const t = run('t-bad', {
    'events.jsonl': [
      tProbe('early', { 'reward-slot': WHITE }),
      tProbe(
        'perf-loading',
        {},
        {
          order: [{ label: 'owad-bw-1-3' }, { label: 'owad-bw-1-1' }],
          webviews: { 'owad-bw-1-3': { region: { kind: 'none' } } },
          hits: [{ name: 'control', target: { label: 'owad-bw-1-3' } }],
          click: { sent: false },
        },
      ),
    ],
    'wc-events.jsonl': [{ kind: 'passthrough', label: 'owad-bw-1-3', on: true }],
  });
  const byProbe = Object.fromEntries(
    labLayersChecks(e, t).map((c) => [`${c.id} ${c.probe}`, c.pass]),
  );
  assert.equal(byProbe['L1-W early reward-slot'], false);
  assert.equal(byProbe['L1-W before-perf reward-slot'], null);
  assert.equal(byProbe['L2 perf-loading'], false);
  assert.equal(byProbe['L3-W perf-loading'], false);
  // No display_ad_loaded: nothing to judge after it.
  assert.equal(byProbe['L3-W perf-loaded'], null);
});

test('audio: L5 compares the mute states read back at each probe', () => {
  const e = run('e-audio', {
    'events.jsonl': [
      eProbe('mute-initial', {}, { guestMuted: [true, true] }),
      eProbe('mute-after-unmute', {}, { guestMuted: [false, false] }),
    ],
  });
  const muted = (a, b) => ({
    webviews: { 'bw-1': { muted: false }, g1: { muted: a }, g2: { muted: b } },
  });
  const t = run('t-audio', {
    'events.jsonl': [
      tProbe('mute-initial', {}, muted(true, true)),
      tProbe('mute-after-unmute', {}, muted(true, false)),
      tProbe('mute-after-mute', {}, muted(true, true)),
    ],
  });
  assert.deepEqual(
    audioChecks(e, t).map((c) => [c.probe, c.pass]),
    [
      ['mute-initial', true],
      ['mute-after-unmute', false],
      ['mute-after-mute', null],
    ],
  );
  assert.deepEqual(windowsChecks('sizes', e, t), []);
});

test('the top child window ignores hidden and unlabelled windows', () => {
  assert.equal(
    topLabel({
      native: {
        order: [
          { label: 'bw-1' },
          { label: 'g1' },
          { label: null, class: 'X' },
          { label: 'g2', hidden: true },
        ],
      },
    }),
    'g1',
  );
  assert.equal(topLabel(undefined), null);
});

test('shards split the lab by run time and cover every scenario once', () => {
  const all = [1, 2, 3, 4].flatMap((i) => shard(LAB_SCENARIOS, i, 4));
  assert.deepEqual([...all].sort(), [...LAB_SCENARIOS].sort());
  const totals = [1, 2, 3, 4].map((i) =>
    shard(LAB_SCENARIOS, i, 4).reduce((n, s) => n + durationOf(s), 0),
  );
  assert.ok(Math.max(...totals) - Math.min(...totals) <= 150, String(totals));
  assert.equal(durationOf('A'), 90);
  assert.deepEqual(shard(['audio', 'lab-layers'], 1, 1), ['audio', 'lab-layers']);
});

test('the state folders of both hosts are reset between runs', () => {
  const dirs = stateDirs({ APPDATA: 'R', LOCALAPPDATA: 'L' });
  assert.deepEqual(dirs, [
    join('R', 'ow-electron'),
    join('R', 'Parity Harness'),
    join('L', 'Parity Harness'),
    join('R', 'dev.ow-tauri.parity-harness'),
    join('L', 'dev.ow-tauri.parity-harness'),
  ]);
  assert.deepEqual(stateDirs({}), []);
});

test('class counts read parity-diff.json', () => {
  assert.deepEqual(
    classCounts({ diffs: [{ class: 'BUG' }, { class: 'variance' }, { class: 'BUG' }] }),
    {
      BUG: 2,
      variance: 1,
    },
  );
  assert.deepEqual(classCounts(null), {});
});

test('a Windows probe with a mute reading reaches the ad-format facts', () => {
  const dir = run('facts', {
    'events.jsonl': [
      {
        ...tProbe('mute-initial', {}),
        native: { webviews: { 'bw-1': { muted: false }, g: { muted: true } } },
      },
    ],
  });
  assert.deepEqual(adformatFacts(dir).probes['mute-initial'].guestMuted, [true]);
});
