// node --test windows-lab.test.mjs
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, test } from 'node:test';

import {
  classCounts,
  durationOf,
  LAB_SCENARIOS,
  resetDir,
  shard,
  stateDirs,
} from './ci/windows-lab.mjs';
import { adformatFacts, compositeAt, guestMuted } from './lib/adformat-report.mjs';
import { isTransientFsError, snapshotDir } from './lib/fs-snapshot.mjs';
import { labRequests, windowsDebugger } from './lib/tauri-host.mjs';
import {
  audioChecks,
  geometryChecks,
  labLayersChecks,
  topLabel,
  windowsChecks,
} from './lib/windows-checks.mjs';

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
  assert.deepEqual(windowsChecks('lab-layers', e, t).slice(0, checks.length), checks);
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
  // Runs without window records: only the geometry checks, with nothing to judge.
  assert.deepEqual(
    windowsChecks('sizes', e, t).map((c) => [c.id, c.scenario, c.pass]),
    [
      ['G1', 'sizes', null],
      ['G2', 'sizes', null],
    ],
  );
});

test('G1 compares the app window frame, G2 the content area (advisory)', () => {
  const loaded = (url, bounds, contentBounds) => ({
    kind: 'did-finish-load',
    url,
    bounds,
    ...(contentBounds ? { contentBounds } : {}),
  });
  const rect = (x, y, width, height) => ({ x, y, width, height });
  const e = run('e-geometry', {
    'windows.jsonl': [
      loaded('file:///x/cmp.html', rect(444, 340, 136, 39)),
      loaded(
        'file:///D:/a/app/index.html?layouts=none',
        rect(0, 0, 1000, 720),
        rect(8, 31, 984, 681),
      ),
      // Regression (Windows lab): ow-electron's own window loads an
      // index.html after the app's; it is not the app window.
      loaded('owepm://index.html/', rect(944, 500, 32, 31), rect(944, 500, 32, 31)),
    ],
  });
  const same = run('t-geometry-same', {
    'windows.jsonl': [
      loaded(
        'tauri://localhost/index.html?layouts=none',
        rect(0, 0, 1000, 720),
        rect(8, 31, 984, 681),
      ),
    ],
  });
  const other = run('t-geometry-other', {
    'windows.jsonl': [
      loaded('tauri://localhost/index.html', rect(0, 0, 1000, 760), rect(8, 20, 984, 732)),
    ],
  });
  const pass = (checks) => checks.map((c) => [c.id, c.pass, c.advisory === true]);
  assert.deepEqual(pass(geometryChecks(e, same)), [
    ['G1', true, false],
    ['G2', true, true],
  ]);
  assert.deepEqual(pass(geometryChecks(e, other)), [
    ['G1', false, false],
    ['G2', false, true],
  ]);
  assert.deepEqual(geometryChecks(e, other)[0].detail, {
    electron: rect(0, 0, 1000, 720),
    tauri: rect(0, 0, 1000, 760),
  });
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

test('a hang is dumped with the Windows SDK debugger when the runner has it', () => {
  const want = join('C:\\K', 'Windows Kits', '10', 'Debuggers', 'x64', 'cdb.exe');
  assert.equal(
    windowsDebugger({ 'ProgramFiles(x86)': 'C:\\K' }, (p) => p === want),
    want,
  );
  assert.equal(
    windowsDebugger({}, () => true),
    null,
  );
});

test('a consent window created blank counts as the page it starts loading', () => {
  // Windows lab: the startup consent window's `created` record is
  // `about:blank`; its page shows up when it starts loading (a navigation
  // record may or may not come first).
  const page = 'https://content.overwolf.com/cmp/ow-cmp-v2.html?clear=true';
  const dir = run('cmp-loaded', {
    'wc-events.jsonl': [
      {
        t: 1,
        wall: 1,
        kind: 'did-start-loading',
        label: 'ow-main',
        type: 'main',
        url: 'http://tauri.localhost/',
      },
      { t: 2, wall: 2, kind: 'created', label: 'ow-cmp-startup', type: 'cmp', url: 'about:blank' },
      { t: 3, wall: 3, kind: 'did-start-loading', label: 'ow-cmp-startup', type: 'cmp', url: page },
      {
        t: 4,
        wall: 4,
        kind: 'navigation',
        label: 'ow-cmp',
        type: 'cmp',
        url: 'https://example.com/',
        allowed: false,
      },
      {
        t: 5,
        wall: 5,
        kind: 'did-start-loading',
        label: 'ow-cmp',
        type: 'cmp',
        url: 'data:text/html,x',
      },
    ],
  });
  assert.deepEqual(
    labRequests(dir).map((r) => [r.requestType, r.url, r.label]),
    [['main frame', page, 'ow-cmp-startup']],
  );
  // Captures without load records name the page on creation.
  const old = run('cmp-created', {
    'wc-events.jsonl': [
      { t: 1, wall: 1, kind: 'created', label: 'ow-cmp', type: 'cmp', url: page },
    ],
  });
  assert.deepEqual(
    labRequests(old).map((r) => r.url),
    [page],
  );
});

test('a snapshot skips files that vanish or are locked while it reads them', () => {
  const dir = join(root, 'snap');
  mkdirSync(join(dir, 'EBWebView'), { recursive: true });
  writeFileSync(join(dir, 'EBWebView', 'kept.json'), '{}');
  const snap = snapshotDir(dir, join(root, 'snap-out'));
  assert.deepEqual(
    snap.files.map((f) => f.path.replaceAll('\\', '/')),
    ['EBWebView/kept.json'],
  );
  for (const code of ['ENOENT', 'EBUSY', 'EPERM'])
    assert.equal(isTransientFsError(Object.assign(new Error(code), { code })), true);
  assert.equal(isTransientFsError(Object.assign(new Error('EACCES'), { code: 'EACCES' })), false);
  assert.equal(isTransientFsError(null), false);
});

test('a state folder WebView2 still holds is removed after ending WebView2', () => {
  // Windows lab: `EBUSY ... EBWebView\\Default\\DIPS` stopped every shard.
  const busy = Object.assign(new Error('EBUSY'), { code: 'EBUSY' });
  let calls = 0;
  let killed = 0;
  const flaky = () => {
    calls += 1;
    if (calls === 1) throw busy;
  };
  assert.equal(
    resetDir('x', { rm: flaky, kill: () => (killed += 1) }),
    'removed after ending WebView2',
  );
  assert.deepEqual([calls, killed], [2, 1]);
  // Windows lab (shard 2, 7625c43): a WebView2 process still writing into
  // `EBWebView` while it was emptied failed the `rmdir` with `ENOTEMPTY`.
  const writing = Object.assign(new Error('ENOTEMPTY'), { code: 'ENOTEMPTY' });
  calls = 0;
  killed = 0;
  const refilled = () => {
    calls += 1;
    if (calls === 1) throw writing;
  };
  assert.equal(
    resetDir('x', { rm: refilled, kill: () => (killed += 1) }),
    'removed after ending WebView2',
  );
  assert.deepEqual([calls, killed], [2, 1]);
  assert.equal(
    resetDir('x', { rm: () => undefined, kill: () => assert.fail('no kill') }),
    'removed',
  );
  const other = Object.assign(new Error('EACCES'), { code: 'EACCES' });
  assert.throws(() =>
    resetDir('x', {
      rm: () => {
        throw other;
      },
      kill: () => assert.fail('no kill'),
    }),
  );
});
