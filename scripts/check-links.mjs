#!/usr/bin/env node
// Checks the relative links and anchors of the repository's Markdown files:
// every `[text](path)`, `[text](path#anchor)`, `[text](#anchor)`, reference
// definition and HTML `href` / `src` must name a file or folder that exists,
// and an anchor into a Markdown file must match one of its headings (GitHub
// slug rules) or an HTML `id` / `name`. External links (any URL scheme) are
// not fetched. Links inside code spans and fenced code blocks are ignored.
//
// Usage: node scripts/check-links.mjs [file.md ...]
// Without arguments it checks every Markdown file outside generated folders
// and outside the upstream sample's vendored docs.
// Node built-ins only, so CI runs it without `npm install`.

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, extname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));

/** Folders that hold generated, vendored or local-only files. */
const SKIP_DIRS = new Set([
  '.git',
  'node_modules',
  'target',
  'dist',
  'docs-out',
  'coverage',
  '.stage',
  'gen',
  'captures',
  'out',
]);

/**
 * Folders, relative to the repository root, whose Markdown is not ours:
 * Overwolf's package docs, imported verbatim with the upstream sample
 * (examples/packages-sample/CHANGES-FROM-UPSTREAM.md).
 */
const SKIP_PATHS = new Set(['examples/packages-sample/docs']);

/** @returns {string[]} every Markdown file under `dir`, outside SKIP_DIRS and SKIP_PATHS */
function markdownFiles(dir) {
  const found = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      const path = join(dir, entry.name);
      const rel = relative(root, path).split(sep).join('/');
      if (!SKIP_DIRS.has(entry.name) && !SKIP_PATHS.has(rel)) found.push(...markdownFiles(path));
    } else if (entry.isFile() && entry.name.toLowerCase().endsWith('.md')) {
      found.push(join(dir, entry.name));
    }
  }
  return found;
}

/**
 * GitHub's heading slug (github-slugger): lower case, drop every character
 * that is not a letter, mark, number, connector punctuation, space or hyphen,
 * then turn each space into a hyphen.
 *
 * @param {string} text the heading's rendered text
 */
export function slug(text) {
  return text
    .toLowerCase()
    .replace(/[^\p{L}\p{M}\p{N}\p{Pc} -]/gu, '')
    .replace(/ /g, '-');
}

/**
 * The text GitHub renders for a heading's Markdown source: links become their
 * text, images, HTML tags, emphasis markers and code backticks disappear.
 *
 * @param {string} source
 */
export function headingText(source) {
  return source
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\[([^\]]*)\]\[[^\]]*\]/g, '$1')
    .replace(/<[^>]+>/g, '')
    .replace(/`+/g, '')
    .replace(/(\*\*|__|\*)/g, '')
    .trim();
}

/**
 * Splits a Markdown file into lines, blanking fenced code blocks and code
 * spans so their contents are neither links nor headings.
 *
 * @param {string} text
 * @returns {string[]}
 */
export function proseLines(text) {
  const lines = text.split(/\r?\n/);
  /** @type {string | null} */
  let fence = null;
  return lines.map((line) => {
    const marker = /^ {0,3}(`{3,}|~{3,})/.exec(line);
    if (fence !== null) {
      if (marker && marker[1][0] === fence[0] && marker[1].length >= fence.length) fence = null;
      return '';
    }
    if (marker) {
      fence = marker[1];
      return '';
    }
    return line.replace(/(`+)(?:(?!\1).)+?\1/g, (span) => ' '.repeat(span.length));
  });
}

/**
 * Every anchor a Markdown file defines: heading slugs (with GitHub's `-1`,
 * `-2` suffixes for repeats) and HTML `id` / `name` attributes.
 *
 * @param {string} text
 * @returns {Set<string>}
 */
export function anchorsOf(text) {
  const anchors = new Set();
  /** @type {Map<string, number>} */
  const seen = new Map();
  const add = (/** @type {string} */ base) => {
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    anchors.add(count === 0 ? base : `${base}-${count}`);
  };
  const raw = text.split(/\r?\n/);
  const lines = proseLines(text);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const atx = /^ {0,3}#{1,6}(?:\s+(.*?))?\s*$/.exec(line);
    if (atx) {
      // Code spans were blanked in `line`; take the heading from the raw line.
      const source = /^ {0,3}#{1,6}(?:\s+(.*?))?\s*$/.exec(raw[i])?.[1] ?? '';
      add(slug(headingText(source.replace(/\s+#+\s*$/, ''))));
      continue;
    }
    const prev = i > 0 ? lines[i - 1] : '';
    if (
      /^ {0,3}(=+|-+)\s*$/.test(line) &&
      prev.trim() !== '' &&
      !/^ {0,3}(#|[-*+] |\d+[.)] |>|\|)/.test(prev) &&
      !/^ {0,3}(=+|-+)\s*$/.test(prev)
    ) {
      add(slug(headingText(raw[i - 1])));
    }
    for (const match of line.matchAll(/<[a-z][^>]*\s(?:id|name)\s*=\s*["']([^"']+)["']/gi)) {
      anchors.add(match[1]);
    }
  }
  return anchors;
}

/**
 * The link targets on each line: inline links and images, reference
 * definitions, and HTML `href` / `src` attributes.
 *
 * @param {string} text
 * @returns {{ line: number, target: string }[]}
 */
export function linksOf(text) {
  const links = [];
  const lines = proseLines(text);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    for (const match of line.matchAll(
      /\]\(\s*(<[^>]*>|[^)\s]+)(?:\s+(?:"[^"]*"|'[^']*'))?\s*\)/g,
    )) {
      links.push({ line: i + 1, target: match[1].replace(/^<|>$/g, '') });
    }
    const definition = /^ {0,3}\[[^\]]+\]:\s*(<[^>]*>|\S+)/.exec(line);
    if (definition) links.push({ line: i + 1, target: definition[1].replace(/^<|>$/g, '') });
    for (const match of line.matchAll(/<[a-z][^>]*\s(?:href|src)\s*=\s*["']([^"']+)["']/gi)) {
      links.push({ line: i + 1, target: match[1] });
    }
  }
  return links;
}

/** @type {Map<string, Set<string>>} */
const anchorCache = new Map();

/** @param {string} file */
function anchorsOfFile(file) {
  let anchors = anchorCache.get(file);
  if (!anchors) {
    anchors = anchorsOf(readFileSync(file, 'utf8'));
    anchorCache.set(file, anchors);
  }
  return anchors;
}

/**
 * Problems with the links of one Markdown file.
 *
 * @param {string} file absolute path
 * @returns {string[]}
 */
export function checkFile(file) {
  const problems = [];
  const rel = relative(root, file).split(sep).join('/');
  for (const { line, target } of linksOf(readFileSync(file, 'utf8'))) {
    // Any scheme (https:, mailto:, ...) or a protocol-relative URL: external.
    if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('//')) continue;
    const hash = target.indexOf('#');
    const pathPart = (hash === -1 ? target : target.slice(0, hash)).replace(/\?.*$/, '');
    const anchor = hash === -1 ? null : target.slice(hash + 1);
    let path;
    try {
      path = decodeURIComponent(pathPart);
    } catch {
      problems.push(`${rel}:${line}: malformed link ${target}`);
      continue;
    }
    // GitHub resolves a leading `/` from the repository root.
    const resolved =
      path === '' ? file : path.startsWith('/') ? join(root, path) : resolve(dirname(file), path);
    if (!existsSync(resolved)) {
      problems.push(`${rel}:${line}: ${target}: ${path} does not exist`);
      continue;
    }
    if (anchor === null || anchor === '') continue;
    if (statSync(resolved).isDirectory() || extname(resolved).toLowerCase() !== '.md') continue;
    let wanted;
    try {
      wanted = decodeURIComponent(anchor);
    } catch {
      wanted = anchor;
    }
    if (!anchorsOfFile(resolved).has(wanted)) {
      const where = path === '' ? 'this file' : path;
      problems.push(`${rel}:${line}: ${target}: no heading or id "${wanted}" in ${where}`);
    }
  }
  return problems;
}

function main() {
  const args = process.argv.slice(2);
  const files = args.length > 0 ? args.map((f) => resolve(f)) : markdownFiles(root);
  const problems = files.flatMap(checkFile);
  for (const problem of problems) console.error(problem);
  if (problems.length > 0) {
    console.error(`\n${problems.length} broken link(s) in ${files.length} Markdown file(s).`);
    process.exit(1);
  }
  console.log(`Links OK in ${files.length} Markdown file(s).`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
