// Tests of the memory gate helpers (lib/proc-sampler.mjs, lib/mem-summary.mjs).
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { GROWTH_LIMIT, foreignPids, summarizeSamples } from './lib/mem-summary.mjs';
import { ownedProcesses, parsePs, parseTopFootprints } from './lib/proc-sampler.mjs';

const MB = 1024;
const sample = (t, procs) => ({ t, procs });
const proc = (pid, name, mb) => ({ pid, name, footprintKb: mb * MB });

test('ps and top output parse', () => {
  assert.deepEqual(parsePs('  12   1  2048 /Applications/X.app/Contents/MacOS/X\n'), [
    { pid: 12, ppid: 1, rssKb: 2048, name: 'X' },
  ]);
  const fp = parseTopFootprints('PID    MEM\n12     150M+\n13     900K\n14 1.5G-\n');
  assert.equal(fp.get(12), 150 * MB);
  assert.equal(fp.get(13), 900);
  assert.equal(fp.get(14), 1.5 * MB * MB);
});

test('owned processes: the app, its children and new WebKit processes of its responsible process', () => {
  const ps = [
    { pid: 10, ppid: 1, name: 'app' },
    { pid: 11, ppid: 10, name: 'helper' },
    { pid: 20, ppid: 1, name: 'com.apple.WebKit.WebContent' },
    { pid: 21, ppid: 1, name: 'com.apple.WebKit.WebContent' },
    { pid: 22, ppid: 1, name: 'com.apple.WebKit.GPU' },
  ];
  const owners = new Map([
    [10, 5],
    [20, 5],
    [21, 5],
    [22, 9],
  ]);
  const got = ownedProcesses(ps, owners, 10, new Set([21]), 5).map((p) => p.pid);
  assert.deepEqual(got, [10, 11, 20]);
});

test('flat guests pass the gate, a growing guest fails it', () => {
  const flat = Array.from({ length: 60 }, (_, i) =>
    sample(i * 10_000, [proc(1, 'app', 50), proc(2, 'com.apple.WebKit.WebContent', 150 + (i % 3))]),
  );
  const ok = summarizeSamples(flat, { bucketS: 120 });
  assert.equal(ok.gate.pass, true);
  assert.equal(ok.largest[0], 151);
  const growing = Array.from({ length: 60 }, (_, i) =>
    sample(i * 10_000, [proc(1, 'app', 50), proc(2, 'com.apple.WebKit.WebContent', 150 + i * 5)]),
  );
  const bad = summarizeSamples(growing, { bucketS: 120 });
  assert.equal(bad.gate.pass, false);
  assert.ok(bad.gate.lastMb > bad.gate.firstMb * (1 + GROWTH_LIMIT));
});

test('a foreign WebKit app group and short-lived pids are dropped', () => {
  const rows = Array.from({ length: 20 }, (_, i) => {
    const procs = [proc(1, 'app', 50), proc(2, 'com.apple.WebKit.WebContent', 100)];
    if (i >= 5 && i <= 10) {
      procs.push(
        proc(30, 'com.apple.WebKit.GPU', 40),
        proc(31, 'com.apple.WebKit.WebContent', 900),
      );
    }
    if (i === 15) procs.push(proc(40, 'com.apple.WebKit.Networking', 10));
    return sample(i * 10_000, procs);
  });
  const dropped = foreignPids(rows, 4);
  assert.deepEqual([...dropped].sort(), [30, 31, 40]);
  assert.equal(summarizeSamples(rows).peakLargestMb, 100);
});
