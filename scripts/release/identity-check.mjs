#!/usr/bin/env node
// Identity check (gate G9): no tracked file may name a person, a private
// project or lab identity, and nothing may describe how ow-electron's
// behaviour was learned other than by observation.
//
//   node scripts/release/identity-check.mjs            scans `git ls-files`
//   node scripts/release/identity-check.mjs <files…>   scans these files
//   node scripts/release/identity-check.mjs --hash <term>
//
// The denied terms are stored as SHA-256 hashes, so this file does not
// spell them out. Text is lower-cased and split into runs of letters and
// digits; every run and every pair or triple of neighbouring runs is hashed
// and looked up. A hit prints the file, the line and the entry number, never
// the term. To add a term, hash it with `--hash "<term>"` (lower case, words
// separated by one space) and append the hash to DENIED below.
//
// The repository's `<owner>/<name>` (in its URL, in package metadata and in
// the release docs) is the one allowed mention of its owner's account; it is
// removed before the scan.
//
// Node built-ins only. Binary files (a NUL byte) are skipped.

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

/** The repository (`<owner>/<name>` on GitHub), as written in package metadata. */
const REPOSITORY = 'AlloryDante/ow-tauri';

/** Text removed before the scan: the repository's `<owner>/<name>`, in any case. */
export const ALLOWED = [new RegExp(REPOSITORY.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi')];

/** SHA-256 of each denied term (see the header). Order is the entry number. */
export const DENIED = [
  '652ccbed27d3820bc260d313fa6580e8433ae652d11e31d8572c57157888f6cd',
  '6661198816aab1fe0b9b1e99c93bef8910b87a269ea99ad13b1ab60f6a733ef6',
  'a4225063e633790a042513deb76cd70741706f437690f5c8811cc9c7054e63a8',
  '0cb660db47562e2b3a660d61386d382e4ff585b2807c927d47556cf33fec083e',
  '54034d566d37185c1bbee462bb3014c119a6a8e8849c96c16a187ea2e07f181a',
  '1c5e35abd829553dbf3fe08354124fcf360f225927c78d1ba36f7511cfbfcbae',
  'fd20543f6dcb63ba0702d2502447a7bace6653fffed01b9c57b7261114ad4e3f',
  '2d0d38e68843216f183110ca3f4212682e1ccf86371e15fbdc0a3f24e697cfe0',
  '94805aea13420685a2929f6859071e2e12d650c6609c0d91ed602a9bf227af39',
  '3230d71babb5be67640f6571fe9dc20c38deebd66e2c5c85db1c4fba60fbeb9f',
  '6e3fd0b70aa5c52721d996ac6485608b4b1d6a9066541778c2b8bb920742a54f',
  'f6ebf749f9339f44e8a5ff914f8d9f169549d4f65b5c82da2af55f73834f8782',
  '807d5e8a70465f7acf0d1afe228a6a7a44e4b1590b70192a1f24cafb019d12ce',
  '1229eb92f937d8371a64432b16e93ea14a558d4822a0ef497db06cc9f83d2f23',
  '5c4cefffb8c5a3d52de634c813b8cfdcad8a7ae7c6e9a547cd4d517f65a79d4b',
  'a34966253ddd9a4b15111a32867c4112ba37b383547801b7bb2cf1772b0aceb0',
  '11108fa8a883da893ae6e502ffc0736ab7992607627c29fc941a8a97e4abfc21',
  'a1d01545e97c960af656f7cc158385be1df89e54b05b0d2423ac54ec02a2ee2d',
  '60eca26de0e424144a66468c03b331d28397ec0572a1293217da19b0d99961fa',
  '40334240fc165e12afd36998d2ee7c4cc3db6857d4d5fe15ec3a658effc3a19d',
  '1518a5450c15cdde26ba9cd488368225b3c95f88e55a185b34b9b54e76ce0204',
];

/** The SHA-256 hex of a term as the scan hashes it. */
export function hashTerm(term) {
  const normal = (term.toLowerCase().match(/[a-z0-9]+/g) ?? []).join(' ');
  return createHash('sha256').update(normal).digest('hex');
}

/**
 * Scans one text. Returns `[{ line, entry }]` (1-based line, 1-based entry
 * number in `denied`).
 */
export function scanText(text, denied = DENIED) {
  const index = new Map(denied.map((h, i) => [h, i + 1]));
  let clean = text;
  for (const re of ALLOWED) clean = clean.replace(re, (m) => ' '.repeat(m.length));
  const hits = [];
  const lines = clean.split('\n');
  for (let n = 0; n < lines.length; n++) {
    // Pairs and triples may span a line break ("first\nlast"): include the
    // first runs of the next line.
    const here = lines[n].toLowerCase().match(/[a-z0-9]+/g) ?? [];
    const next = (lines[n + 1]?.toLowerCase().match(/[a-z0-9]+/g) ?? []).slice(0, 2);
    const runs = [...here, ...next];
    const seen = new Set();
    for (let i = 0; i < here.length; i++) {
      for (let k = 1; k <= 3 && i + k <= runs.length; k++) {
        const h = createHash('sha256')
          .update(runs.slice(i, i + k).join(' '))
          .digest('hex');
        const entry = index.get(h);
        if (entry !== undefined && !seen.has(entry)) {
          seen.add(entry);
          hits.push({ line: n + 1, entry });
        }
      }
    }
  }
  return hits;
}

/** Scans files under `root`; the path itself is scanned as line 0. */
export function scanFiles(root, files, denied = DENIED) {
  const out = [];
  for (const file of files) {
    for (const h of scanText(file, denied)) out.push({ file, line: 0, entry: h.entry });
    let buf;
    try {
      buf = readFileSync(join(root, file));
    } catch {
      continue; // deleted in the working tree, or a submodule
    }
    if (buf.includes(0)) continue;
    for (const h of scanText(buf.toString('utf8'), denied)) out.push({ file, ...h });
  }
  return out;
}

function trackedFiles(root) {
  return execFileSync('git', ['ls-files', '-z'], { cwd: root, encoding: 'utf8' })
    .split('\0')
    .filter(Boolean);
}

function main(argv) {
  if (argv[0] === '--hash') {
    if (!argv[1]) {
      console.error('Usage: node scripts/release/identity-check.mjs --hash "<term>"');
      return 2;
    }
    console.log(hashTerm(argv[1]));
    return 0;
  }
  const files = argv.length > 0 ? argv : trackedFiles(REPO_ROOT);
  const hits = scanFiles(REPO_ROOT, files);
  for (const h of hits) console.error(`${h.file}:${h.line}: denied term #${h.entry}`);
  if (hits.length > 0) {
    console.error(
      `\nidentity-check: ${hits.length} hit(s). Remove the term (see the header of this script).`,
    );
    return 1;
  }
  console.log(`identity-check: ${files.length} files, no denied terms.`);
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = main(process.argv.slice(2));
}
