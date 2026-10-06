#!/usr/bin/env node
// Fails when any @overwolf/* npm dependency is not on that package's `latest`
// dist-tag (CONTRIBUTING.md, "Dependencies"). Checks every workspace manifest
// and the versions resolved in package-lock.json.
//
// Usage: node scripts/check-overwolf-dist-tags.mjs
// Needs network access to the npm registry; CI runs it on Linux only.

import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));

/** @param {string} path */
function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

/** Workspace manifests: the root plus every packages/* and examples/* package. */
function manifests() {
  const found = [join(root, 'package.json')];
  for (const dir of ['packages', 'examples']) {
    const base = join(root, dir);
    if (!existsSync(base)) continue;
    for (const entry of readdirSync(base, { withFileTypes: true })) {
      const manifest = join(base, entry.name, 'package.json');
      if (entry.isDirectory() && existsSync(manifest)) found.push(manifest);
    }
  }
  return found;
}

/** @type {Map<string, string>} */
const latestCache = new Map();

/** @param {string} name */
function latestOf(name) {
  const cached = latestCache.get(name);
  if (cached !== undefined) return cached;
  const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
  const latest = execFileSync(npm, ['view', name, 'dist-tags.latest'], {
    encoding: 'utf8',
  }).trim();
  latestCache.set(name, latest);
  return latest;
}

/** @type {string[]} */
const problems = [];

for (const manifest of manifests()) {
  const pkg = readJson(manifest);
  for (const field of [
    'dependencies',
    'devDependencies',
    'peerDependencies',
    'optionalDependencies',
  ]) {
    for (const [name, spec] of Object.entries(pkg[field] ?? {})) {
      if (!name.startsWith('@overwolf/')) continue;
      const latest = latestOf(name);
      const base = String(spec).replace(/^[\^~=]/, '');
      if (spec !== 'latest' && base !== latest) {
        problems.push(`${manifest}: ${field}.${name} is "${spec}", latest is ${latest}`);
      }
    }
  }
}

const lockPath = join(root, 'package-lock.json');
if (existsSync(lockPath)) {
  const lock = readJson(lockPath);
  for (const [path, entry] of Object.entries(lock.packages ?? {})) {
    const match = /node_modules\/(@overwolf\/[^/]+)$/.exec(path);
    if (match === null) continue;
    const name = match[1];
    const latest = latestOf(name);
    if (entry.version !== latest) {
      problems.push(`package-lock.json: ${path} resolves ${entry.version}, latest is ${latest}`);
    }
  }
}

if (problems.length > 0) {
  console.error('Overwolf packages must track the `latest` dist-tag:');
  for (const problem of problems) console.error(`  - ${problem}`);
  process.exit(1);
}
console.log(
  `Overwolf packages are on latest (${[...latestCache].map(([n, v]) => `${n}@${v}`).join(', ')}).`,
);
