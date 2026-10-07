#!/usr/bin/env node
// Compares two showcase lab runs step by step (normally ow-tauri against the
// ow-electron baseline, the same scenario on both) and writes compare.md and
// compare.json into the first run's folder.
//
//   node e2e/compare.mjs e2e/out/<tauri run> e2e/out/<electron run>
//
// Per step (the steps have the same names on both hosts):
// - the events each ad element raised since the previous step: event names
//   with their counts, per slot. Slot ids lose their per-creation counter
//   (`sz3-…`, `ly12-…`, `perf-4`), so the same slot compares across hosts;
// - the slot statuses, the page's own state (`snapshot().state`) and the
//   step's checks (`filled`, `ready`, `loaded`, … with `ok` and the time);
// - the interstitial hit probes (page hit test, pointer-events, clicks).
// Verdicts: `same`, `count` (the same event names, other counts: refresh
// and reload timing), `lifecycle` (they differ only in guest page lifecycle
// events) and `differs` (an ad event, a status, a state field or a check
// that only one host has). The report also lists each format's
// event order per slot over the whole run.

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';

const readJsonl = (file) =>
  existsSync(file)
    ? readFileSync(file, 'utf8')
        .split('\n')
        .filter(Boolean)
        .flatMap((l) => {
          try {
            return [JSON.parse(l)];
          } catch {
            return [];
          }
        })
    : [];

/**
 * Drops the per-creation counter from a slot id (`sz3-300x250` ->
 * `sz-300x250`, `ly12-0-728x90` -> `ly-0-728x90`, `perf-4` -> `perf`), so
 * the same slot compares across hosts whatever order the hosts created or
 * reported their slots in. A step creates each of these at most once.
 */
export function cidNamer() {
  return (cid) => {
    const m = /^(sz|ly)\d+(-.*)$/.exec(cid);
    if (m) return `${m[1]}${m[2]}`;
    return /^perf-\d+$/.test(cid) ? 'perf' : cid;
  };
}

/**
 * Guest page lifecycle events. ow-electron reports a `did-fail-load` for
 * the first, aborted navigation of every guest; ow-tauri has no such
 * navigation [OBS]. Rows that differ only in these are `lifecycle`.
 */
export const LIFECYCLE = new Set(['did-attach', 'dom-ready', 'did-finish-load', 'did-fail-load']);

/** Step fields that are timings or geometry, not outcomes. */
const VOLATILE_STATE = new Set(['rect', 'zone', 'bigBox', 'target', 'scrollTop', 'coins']);

/** A run: its steps in order, plus the probes and the event order per slot. */
export function loadRun(dir) {
  const records = readJsonl(join(dir, 'e2e.jsonl'));
  const summary = existsSync(join(dir, 'summary.json'))
    ? JSON.parse(readFileSync(join(dir, 'summary.json'), 'utf8'))
    : {};
  const name = cidNamer();
  const steps = [];
  const order = {};
  const totals = {};
  for (const r of records) {
    if (r.kind !== 'step') continue;
    const events = {};
    for (const e of r.events ?? []) {
      const cid = name(e.cid);
      if (cid === 'app' || e.name.startsWith('control:')) continue;
      events[cid] ??= {};
      events[cid][e.name] = (events[cid][e.name] ?? 0) + 1;
      totals[cid] ??= {};
      totals[cid][e.name] = (totals[cid][e.name] ?? 0) + 1;
      const seq = (order[cid] ??= []);
      if (seq.at(-1) !== e.name) seq.push(e.name);
    }
    const checks = {};
    for (const [k, v] of Object.entries(r)) {
      if (v && typeof v === 'object' && 'ok' in v && 'ms' in v) checks[k] = v;
    }
    const snap = r.snapshot ?? {};
    const slots = Object.fromEntries((snap.slots ?? []).map((s) => [name(s.cid), s]));
    const state = Object.fromEntries(
      Object.entries(snap.state ?? {}).filter(([k]) => !VOLATILE_STATE.has(k)),
    );
    steps.push({ name: r.name, events, checks, slots, state, owadviews: snap.owadviews });
  }
  const probes = records.filter((r) => r.kind === 'probe');
  return { dir, summary, steps, order, totals, probes };
}

const fmt = (counts) =>
  Object.entries(counts ?? {})
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([k, n]) => (n > 1 ? `${k}×${String(n)}` : k))
    .join(', ');

/** The per-step comparison rows. */
export function compareRuns(a, b) {
  const rows = [];
  const indexB = new Map();
  b.steps.forEach((s) => indexB.set(s.name, [...(indexB.get(s.name) ?? []), s]));
  const used = new Map();
  for (const sa of a.steps) {
    const k = used.get(sa.name) ?? 0;
    used.set(sa.name, k + 1);
    const sb = (indexB.get(sa.name) ?? [])[k];
    if (!sb) {
      rows.push({ step: sa.name, what: 'step', a: 'present', b: 'missing', verdict: 'differs' });
      continue;
    }
    const cids = new Set([...Object.keys(sa.events), ...Object.keys(sb.events)]);
    for (const cid of [...cids].sort()) {
      const ea = sa.events[cid] ?? {};
      const eb = sb.events[cid] ?? {};
      const names = (e, all) =>
        Object.keys(e)
          .filter((n) => all || !LIFECYCLE.has(n))
          .sort()
          .join();
      const verdict =
        names(ea, false) !== names(eb, false)
          ? 'differs'
          : names(ea, true) !== names(eb, true)
            ? 'lifecycle'
            : fmt(ea) === fmt(eb)
              ? 'same'
              : 'count';
      rows.push({ step: sa.name, what: `events ${cid}`, a: fmt(ea), b: fmt(eb), verdict });
    }
    const slots = new Set([...Object.keys(sa.slots), ...Object.keys(sb.slots)]);
    for (const cid of [...slots].sort()) {
      const xa = sa.slots[cid];
      const xb = sb.slots[cid];
      const va = xa ? `${xa.status}${xa.display === 'none' ? ' (display:none)' : ''}` : '-';
      const vb = xb ? `${xb.status}${xb.display === 'none' ? ' (display:none)' : ''}` : '-';
      rows.push({
        step: sa.name,
        what: `status ${cid}`,
        a: va,
        b: vb,
        verdict: va === vb ? 'same' : 'differs',
      });
    }
    const keys = new Set([...Object.keys(sa.state), ...Object.keys(sb.state)]);
    for (const key of [...keys].sort()) {
      const va = JSON.stringify(sa.state[key] ?? null);
      const vb = JSON.stringify(sb.state[key] ?? null);
      rows.push({
        step: sa.name,
        what: `state ${key}`,
        a: va,
        b: vb,
        verdict: va === vb ? 'same' : 'differs',
      });
    }
    const checks = new Set([...Object.keys(sa.checks), ...Object.keys(sb.checks)]);
    for (const key of [...checks].sort()) {
      const ca = sa.checks[key];
      const cb = sb.checks[key];
      rows.push({
        step: sa.name,
        what: `check ${key}`,
        a: ca ? `${ca.ok ? 'ok' : 'no'} ${String(ca.ms)} ms` : '-',
        b: cb ? `${cb.ok ? 'ok' : 'no'} ${String(cb.ms)} ms` : '-',
        verdict: ca?.ok === cb?.ok ? 'same' : 'differs',
      });
    }
    if (sa.owadviews !== sb.owadviews) {
      rows.push({
        step: sa.name,
        what: 'owadview count',
        a: String(sa.owadviews),
        b: String(sb.owadviews),
        verdict: 'differs',
      });
    }
  }
  for (const pa of a.probes) {
    const pb = b.probes.find((p) => p.name === pa.name);
    const show = (p) =>
      p
        ? `hit=${p.page?.action ?? p.page?.tag ?? '-'}${p.page?.inOwadview ? ' (owadview)' : ''} pe=${String(p.pointerEvents)}/${String(p.overlayPointerEvents)} clicks ${String(p.clicksBefore)}->${String(p.clicksAfter)}`
        : '-';
    const key = (p) =>
      p
        ? `${p.page?.action}|${p.page?.inOwadview}|${p.pointerEvents}|${p.overlayPointerEvents}`
        : '';
    rows.push({
      step: pa.name,
      what: 'probe',
      a:
        show(pa) +
        (pa.native ? ` native=${JSON.stringify(pa.native.hits?.[0]?.target ?? pa.native)}` : ''),
      b: show(pb),
      verdict: key(pa) === key(pb) ? 'same' : 'differs',
    });
  }
  return rows;
}

/**
 * Per slot over the whole run: the ad event names each host raised (guest
 * lifecycle left out). Step rows are sensitive to timing (a video's `play`
 * lands one step later on the slower host); these rows are not.
 */
export function compareTotals(a, b) {
  const adNames = (t) =>
    Object.keys(t ?? {})
      .filter((n) => !LIFECYCLE.has(n) && !n.startsWith('dom:'))
      .sort()
      .join(', ');
  return [...new Set([...Object.keys(a.totals), ...Object.keys(b.totals)])].sort().map((cid) => {
    const na = adNames(a.totals[cid]);
    const nb = adNames(b.totals[cid]);
    return { cid, a: na, b: nb, verdict: na === nb ? 'same' : 'differs' };
  });
}

function main() {
  const [aDir, bDir] = process.argv.slice(2);
  if (!aDir || !bDir) {
    console.error('usage: compare.mjs <run A> <run B>');
    process.exit(2);
  }
  const a = loadRun(aDir);
  const b = loadRun(bDir);
  const rows = compareRuns(a, b);
  const totals = compareTotals(a, b);
  const tally = rows.reduce((t, r) => ({ ...t, [r.verdict]: (t[r.verdict] ?? 0) + 1 }), {});
  const nameA = `${basename(aDir)} (${a.summary.host ?? '?'})`;
  const nameB = `${basename(bDir)} (${b.summary.host ?? '?'})`;
  const cell = (t) => String(t).replace(/\|/g, '\\|');
  const md = [
    `# Showcase lab: ${nameA} vs ${nameB}`,
    '',
    `Steps: ${String(a.steps.length)} vs ${String(b.steps.length)}. Rows: ${Object.entries(tally)
      .map(([k, n]) => `${k} ${String(n)}`)
      .join(', ')}.`,
    '',
    '## Differences',
    '',
    `| Step | What | ${nameA} | ${nameB} | Verdict |`,
    '|---|---|---|---|---|',
    ...rows
      .filter((r) => r.verdict !== 'same')
      .map(
        (r) => `| ${cell(r.step)} | ${cell(r.what)} | ${cell(r.a)} | ${cell(r.b)} | ${r.verdict} |`,
      ),
    '',
    '## Ad events per slot (whole run)',
    '',
    `| Slot | ${nameA} | ${nameB} | Same |`,
    '|---|---|---|---|',
    ...totals.map(
      (t) => `| ${t.cid} | ${cell(t.a)} | ${cell(t.b)} | ${t.verdict === 'same' ? 'yes' : 'no'} |`,
    ),
    '',
    '## Event order per slot (whole run, repeats folded)',
    '',
    `| Slot | ${nameA} | ${nameB} | Same |`,
    '|---|---|---|---|',
    ...[...new Set([...Object.keys(a.order), ...Object.keys(b.order)])].sort().map((cid) => {
      const oa = (a.order[cid] ?? []).join(' → ');
      const ob = (b.order[cid] ?? []).join(' → ');
      return `| ${cid} | ${cell(oa)} | ${cell(ob)} | ${oa === ob ? 'yes' : 'no'} |`;
    }),
    '',
  ].join('\n');
  writeFileSync(join(aDir, 'compare.md'), md);
  writeFileSync(
    join(aDir, 'compare.json'),
    JSON.stringify({ tally, rows, totals, orderA: a.order, orderB: b.order }, null, 2),
  );
  const slotsSame = totals.filter((t) => t.verdict === 'same').length;
  console.log(
    `compare: ${JSON.stringify(tally)}; ad events per slot same ${String(slotsSame)}/${String(totals.length)} -> ${join(aDir, 'compare.md')}`,
  );
}

if (import.meta.url === `file://${process.argv[1]}`) main();
