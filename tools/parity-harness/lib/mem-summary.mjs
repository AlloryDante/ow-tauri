#!/usr/bin/env node
// Summarises a run's proc-samples.jsonl (run.mjs --mem-sample-ms) into
// per-bucket medians and evaluates the DESIGN §7.6 macOS guest footprint
// gate: the largest web content process's last-bucket median may be at most
// its first-bucket median + 25 % (no growth).
//
//   node lib/mem-summary.mjs captures/<run> [--bucket-s 120] [--min-samples 4]

import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { fileURLToPath } from 'node:url';

/** Growth the gate allows between the first and the last bucket. */
export const GROWTH_LIMIT = 0.25;

const median = (values) => {
  const s = [...values].sort((a, b) => a - b);
  if (!s.length) return 0;
  const mid = Math.floor(s.length / 2);
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
};

const isContent = (p) => /WebContent|Renderer/.test(p.name);

/**
 * Pids to drop: processes seen in fewer than `minSamples` samples, and every
 * process of a foreign WebKit app group (another app of the same responsible
 * process: its own GPU process appeared after the first sample, and the
 * group shares its lifetime within one sample).
 */
export function foreignPids(rows, minSamples) {
  const seen = new Map();
  const span = new Map();
  rows.forEach((r, i) => {
    for (const p of r.procs) {
      seen.set(p.pid, (seen.get(p.pid) ?? 0) + 1);
      const s = span.get(p.pid) ?? { name: p.name, a: i, b: i };
      s.b = i;
      span.set(p.pid, s);
    }
  });
  const first = new Set((rows[0]?.procs ?? []).map((p) => p.pid));
  const gpus = [...span]
    .filter(([pid, s]) => s.name.endsWith('WebKit.GPU') && !first.has(pid))
    .map(([, s]) => s);
  const out = new Set();
  for (const [pid, s] of span) {
    if ((seen.get(pid) ?? 0) < minSamples) out.add(pid);
    else if (gpus.some((g) => Math.abs(s.a - g.a) <= 1 && Math.abs(s.b - g.b) <= 1)) out.add(pid);
  }
  return out;
}

/**
 * Bucketed medians (MB) of the total footprint, the web content sum and the
 * largest web content process, plus the gate verdict.
 * @param {Array<{t: number, procs: Array<{pid: number, name: string, footprintKb: number | null}>}>} rows
 */
export function summarizeSamples(rows, { bucketS = 120, minSamples = 4 } = {}) {
  if (!rows.length) return null;
  const dropped = foreignPids(rows, minSamples);
  const t0 = rows[0].t;
  const buckets = new Map();
  let peakContent = 0;
  let peakTotal = 0;
  for (const r of rows) {
    const ps = r.procs.filter((p) => !dropped.has(p.pid));
    const content = ps.filter(isContent).map((p) => p.footprintKb ?? 0);
    const total = ps.reduce((n, p) => n + (p.footprintKb ?? 0), 0);
    const row = {
      total,
      content: content.reduce((n, v) => n + v, 0),
      largest: Math.max(0, ...content),
      procs: ps.length,
    };
    peakContent = Math.max(peakContent, row.largest);
    peakTotal = Math.max(peakTotal, total);
    const k = Math.floor((r.t - t0) / 1000 / bucketS);
    if (!buckets.has(k)) buckets.set(k, []);
    buckets.get(k).push(row);
  }
  const mb = (kb) => Math.round(kb / 1024);
  const keys = [...buckets.keys()].sort((a, b) => a - b);
  const series = (key) => keys.map((k) => mb(median(buckets.get(k).map((x) => x[key]))));
  const largest = series('largest');
  const firstLargest = largest[0];
  const lastLargest = largest[largest.length - 1];
  const limit = Math.round(firstLargest * (1 + GROWTH_LIMIT));
  return {
    minutes: Math.round(((rows[rows.length - 1].t - t0) / 60000) * 10) / 10,
    bucketS,
    droppedPids: dropped.size,
    total: series('total'),
    content: series('content'),
    largest,
    procs: keys.map((k) => median(buckets.get(k).map((x) => x.procs))),
    peakLargestMb: mb(peakContent),
    peakTotalMb: mb(peakTotal),
    gate: {
      rule: `largest web content process: last ${bucketS / 60}-min median <= first + ${GROWTH_LIMIT * 100} %`,
      firstMb: firstLargest,
      lastMb: lastLargest,
      limitMb: limit,
      pass: lastLargest <= limit,
    },
  };
}

function main() {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      'bucket-s': { type: 'string', default: '120' },
      'min-samples': { type: 'string', default: '4' },
    },
  });
  const dir = positionals[0];
  if (!dir) {
    console.error('usage: node lib/mem-summary.mjs <run dir> [--bucket-s 120] [--min-samples 4]');
    process.exit(2);
  }
  const rows = readFileSync(join(dir, 'proc-samples.jsonl'), 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((l) => JSON.parse(l));
  const summary = summarizeSamples(rows, {
    bucketS: Number(values['bucket-s']),
    minSamples: Number(values['min-samples']),
  });
  writeFileSync(join(dir, 'mem-summary.json'), JSON.stringify(summary, null, 2) + '\n');
  console.log(JSON.stringify(summary, null, 2));
  process.exit(summary?.gate.pass ? 0 : 1);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
