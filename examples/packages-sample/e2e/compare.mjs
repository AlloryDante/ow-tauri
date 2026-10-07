#!/usr/bin/env node
// Compares two end-to-end runs action by action (normally ow-tauri against
// the ow-electron baseline) and writes compare.md and compare.json into the
// first run's folder.
//
//   node e2e/compare.mjs e2e/out/<tauri run> e2e/out/<electron run>
//
// Each action's output (page console, errors, unhandled rejections, alerts,
// main-process log lines and console) is normalised: host names and
// versions, window ids, app URLs, absolute paths, numbers in timings and
// ad-content details are replaced by placeholders. Ad events are compared
// as the set of event names per slot. What remains is listed per page.

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';

const [aDir, bDir] = process.argv.slice(2);
if (!aDir || !bDir) {
  console.error('usage: compare.mjs <run A> <run B>');
  process.exit(2);
}

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
 * Differences that are the JavaScript engine's, not the app's or the host's:
 * V8 and JavaScriptCore word the same TypeError differently, and Chromium
 * prefixes an uncaught error with `Uncaught `. They are folded before the
 * comparison and counted in the report's header.
 */
const ENGINE = [
  [/^Uncaught /, ''],
  [/Cannot (?:read|set) properties of (?:undefined|null) \((?:reading|setting) '[^']*'\)/g, '<undefined access>'],
  [/(?:undefined|null) is not an object \(evaluating '[^']*'\)/g, '<undefined access>'],
  [/\b(?:TypeError|Error): <undefined access>/g, '<undefined access>'],
];
export const engineFolds = { count: 0 };

/** Volatile or host-labelled parts of a line. */
export function normalise(text) {
  let line = String(text);
  for (const [pattern, replacement] of ENGINE) {
    const next = line.replace(pattern, replacement);
    if (next !== line) engineFolds.count += 1;
    line = next;
  }
  return line
    .replace(/ow-(electron|tauri) v[\w.+-]+/g, 'ow-<host> v<ver>')
    .replace(/\b(electron|tauri)\b/gi, '<host>')
    .replace(/(file|tauri|https?):\/\/[^\s"')]*?(\/renderer\/|\/browser\/|\/osr\/|\/exclusive\/)/g, '<app>$2')
    .replace(/\/(Users|private|var)\/[^\s"',)]+/g, '<path>')
    .replace(/"?(id|windowId|webContentsId)"?\s*[:=]\s*\d+/g, '$1:<n>')
    .replace(/\b\d{10,13}\b/g, '<ts>')
    .replace(/\[\d{2}:\d{2}:\d{2}\]/g, '[<time>]')
    // React's development warnings end with a component stack, whose frames
    // the engines print differently.
    .replace(/^(Warning: [^\n]*?)(?:%s|\n)[\s\S]*$/, '$1')
    .replace(/\b\d+(\.\d+)?\s?ms\b/g, '<n>ms')
    .replace(/\s+/g, ' ')
    .trim();
}

const AD_EVENT = /owadview - (?:video )?([\w-]+)|performance ad - ([\w-]+)|(High impact ad (?:loaded|removed))|house_ad clicked/;

function lines(step) {
  const out = [];
  for (const p of step.events ?? []) {
    if (p.kind === 'console' && AD_EVENT.test(p.text)) continue; // compared as events
    out.push(`page ${p.kind}${p.level ? `:${p.level}` : ''}: ${normalise(p.text)}`);
  }
  for (const m of step.main ?? []) {
    out.push(`main ${m.kind}${m.level ? `:${m.level}` : ''}: ${normalise(m.text)}`);
  }
  if (step.error) out.push(`driver error: ${normalise(step.error)}`);
  return out;
}

function adEvents(step) {
  const names = new Set();
  for (const p of step.events ?? []) {
    if (p.kind !== 'console') continue;
    const m = AD_EVENT.exec(p.text);
    if (!m) continue;
    const slot = /- (ad\d) (\d+x\d+)/.exec(p.text);
    names.add(`${slot ? slot[1] : 'perf'}:${m[1] ?? m[2] ?? m[3] ?? 'house_ad'}`);
  }
  return [...names].sort();
}

function load(dir) {
  const records = readJsonl(join(dir, 'e2e.jsonl'));
  const steps = new Map();
  for (const r of records) {
    if (r.kind !== 'step') continue;
    const key = `${r.page} | ${r.action} | ${r.target}`;
    const prev = steps.get(key);
    if (prev) {
      prev.lines.push(...lines(r));
      for (const e of adEvents(r)) if (!prev.ads.includes(e)) prev.ads.push(e);
    } else {
      steps.set(key, { page: r.page, key, lines: lines(r), ads: adEvents(r), result: r.result });
    }
  }
  const states = records.filter((r) => r.kind === 'state' || r.kind === 'boot');
  const summary = existsSync(join(dir, 'summary.json')) ? JSON.parse(readFileSync(join(dir, 'summary.json'), 'utf8')) : {};
  return { dir, name: basename(dir), records, steps, states, summary };
}

const A = load(aDir);
const B = load(bDir);
const keys = [...new Set([...A.steps.keys(), ...B.steps.keys()])];
const pages = [...new Set(keys.map((k) => k.split(' | ')[0]))];
const result = { a: A.name, b: B.name, pages: [] };
for (const page of pages) {
  const rows = [];
  for (const key of keys.filter((k) => k.startsWith(`${page} |`))) {
    const a = A.steps.get(key);
    const b = B.steps.get(key);
    const la = new Set(a?.lines ?? []);
    const lb = new Set(b?.lines ?? []);
    const onlyA = [...la].filter((l) => !lb.has(l));
    const onlyB = [...lb].filter((l) => !la.has(l));
    const adsA = a?.ads ?? [];
    const adsB = b?.ads ?? [];
    const adsOnlyA = adsA.filter((e) => !adsB.includes(e));
    const adsOnlyB = adsB.filter((e) => !adsA.includes(e));
    rows.push({
      key,
      in: a && b ? 'both' : a ? A.name : B.name,
      same: Boolean(a && b) && !onlyA.length && !onlyB.length && !adsOnlyA.length && !adsOnlyB.length,
      onlyA,
      onlyB,
      adsOnlyA,
      adsOnlyB,
      adsA,
      adsB,
    });
  }
  result.pages.push({ page, rows });
}

const md = [];
md.push(`# ${A.name} vs ${B.name}`, '');
md.push(`| run | verdict | everVisible | steps | steps with errors | blocked OS surfaces |`, '|---|---|---|---|---|---|');
for (const r of [A, B]) {
  const s = r.summary;
  md.push(`| ${r.name} | ${s.verdict} | ${s.everVisible} | ${s.steps} | ${s.stepsWithErrors} | ${s.blocked} |`);
}
md.push('', `Engine wording folded in ${engineFolds.count} lines (V8 vs JavaScriptCore TypeError text, \`Uncaught \` prefix).`);
md.push('', '| page | actions | same | differ | only in one run |', '|---|---|---|---|---|');
for (const p of result.pages) {
  const same = p.rows.filter((r) => r.same).length;
  const one = p.rows.filter((r) => r.in !== 'both').length;
  md.push(`| ${p.page} | ${p.rows.length} | ${same} | ${p.rows.length - same - one} | ${one} |`);
}
for (const p of result.pages) {
  const diff = p.rows.filter((r) => !r.same);
  if (!diff.length) continue;
  md.push('', `## ${p.page}`, '');
  for (const r of diff) {
    md.push(`- \`${r.key}\`${r.in !== 'both' ? ` (only in ${r.in})` : ''}`);
    for (const l of r.onlyA) md.push(`  - ${A.name}: ${l.slice(0, 300)}`);
    for (const l of r.onlyB) md.push(`  - ${B.name}: ${l.slice(0, 300)}`);
    if (r.adsOnlyA.length || r.adsOnlyB.length) {
      md.push(`  - ad events ${A.name}: ${r.adsA.join(', ') || '-'}; ${B.name}: ${r.adsB.join(', ') || '-'}`);
    }
  }
}
writeFileSync(join(aDir, 'compare.json'), JSON.stringify(result, null, 2));
writeFileSync(join(aDir, 'compare.md'), md.join('\n') + '\n');
console.log(md.slice(0, 20).join('\n'));
