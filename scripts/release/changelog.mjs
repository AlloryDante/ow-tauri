#!/usr/bin/env node
// Reads and stamps release sections of CHANGELOG.md (Keep a Changelog 1.1).
//
//   node scripts/release/changelog.mjs section <version>          prints the section body
//   node scripts/release/changelog.mjs check <version> [--dated]  fails if it is missing or empty
//   node scripts/release/changelog.mjs stamp <version> [--date YYYY-MM-DD]
//
// A section starts at `## [<version>] - <date>` (or `- Unreleased` before the
// release) and ends at the next `## ` heading or the link references at the
// bottom. `stamp` turns `- Unreleased` into today's date (UTC).
//
// Node built-ins only. `--file <path>` reads another changelog (tests).

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const DEFAULT_FILE = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'CHANGELOG.md');

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

function headingRe(version) {
  return new RegExp(`^## \\[${escape(version)}\\](?: - (.+?))?[ \\t]*$`, 'm');
}

/**
 * Finds the section of `version`: `{ date, body, start, headingEnd }`, or
 * `undefined`. `date` is the text after ` - ` in the heading.
 */
export function findSection(text, version) {
  const m = headingRe(version).exec(text);
  if (!m) return undefined;
  const headingEnd = m.index + m[0].length;
  const rest = text.slice(headingEnd);
  const next = rest.search(/^## |^\[[^\]]+\]: /m);
  const body = (next < 0 ? rest : rest.slice(0, next)).trim();
  return { date: m[1], body, start: m.index, headingEnd };
}

/** Whether a heading date is a real `YYYY-MM-DD` date. */
export function isDated(date) {
  if (!date || !/^\d{4}-\d{2}-\d{2}$/.test(date)) return false;
  const d = new Date(`${date}T00:00:00Z`);
  return !Number.isNaN(d.getTime()) && d.toISOString().startsWith(date);
}

/** Problems with the section of `version` (empty when it is fine). */
export function checkSection(text, version, { dated = false } = {}) {
  const s = findSection(text, version);
  if (!s) return [`CHANGELOG.md has no "## [${version}]" section`];
  const problems = [];
  if (s.body === '') problems.push(`the ${version} section is empty`);
  if (dated && !isDated(s.date)) {
    problems.push(
      `the ${version} heading has no release date (found ${JSON.stringify(s.date ?? '')}); ` +
        `run: node scripts/release/changelog.mjs stamp ${version}`,
    );
  }
  return problems;
}

/** Replaces `- Unreleased` (or a missing date) in the heading of `version` with `date`. */
export function stamp(text, version, date) {
  if (!isDated(date)) throw new Error(`not a YYYY-MM-DD date: ${date}`);
  const s = findSection(text, version);
  if (!s) throw new Error(`CHANGELOG.md has no "## [${version}]" section`);
  if (isDated(s.date)) return text;
  if (s.date !== undefined && s.date.toLowerCase() !== 'unreleased') {
    throw new Error(`unexpected heading date ${JSON.stringify(s.date)}`);
  }
  return `${text.slice(0, s.start)}## [${version}] - ${date}${text.slice(s.headingEnd)}`;
}

function main(argv) {
  let file = DEFAULT_FILE;
  let date = new Date().toISOString().slice(0, 10);
  let dated = false;
  const positional = [];
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--file') file = resolve(argv[++i] ?? '');
    else if (argv[i] === '--date') date = argv[++i];
    else if (argv[i] === '--dated') dated = true;
    else positional.push(argv[i]);
  }
  const [command, version] = positional;
  if (!version || !['section', 'check', 'stamp'].includes(command)) {
    console.error(
      'Usage: node scripts/release/changelog.mjs section <version> | check <version> [--dated] | stamp <version> [--date YYYY-MM-DD]',
    );
    return 2;
  }
  const text = readFileSync(file, 'utf8');
  if (command === 'stamp') {
    const out = stamp(text, version, date);
    if (out !== text) writeFileSync(file, out);
    console.log(out === text ? `${version} is already dated` : `stamped ${version} - ${date}`);
    return 0;
  }
  const problems = checkSection(text, version, { dated: command === 'check' && dated });
  if (problems.length > 0) {
    for (const p of problems) console.error(`changelog: ${p}`);
    return 1;
  }
  if (command === 'section') console.log(findSection(text, version).body);
  else console.log(`changelog: the ${version} section is present${dated ? ' and dated' : ''}.`);
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (err) {
    console.error(`changelog: ${err.message}`);
    process.exitCode = 1;
  }
}
