#!/usr/bin/env node
// Keeps the one release version in step across every manifest (DESIGN §8.2:
// the crates and the npm packages share one version).
//
//   node scripts/release/version-sync.mjs check [--expect <version>]
//   node scripts/release/version-sync.mjs set <version>
//   node scripts/release/version-sync.mjs dist-tag <version>
//
// `check` lists every place that carries the version and fails when they
// differ (or differ from `--expect`). `set` rewrites them all, lockfiles
// included, with plain text edits: no cargo or npm run is needed. `dist-tag`
// prints the npm dist-tag for a version: `next` for a prerelease, else
// `latest`.
//
// Node built-ins only. `--root <dir>` points at another checkout (tests).

import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

/** The repository root this script lives in. */
export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

/** The published npm packages and the private one that ships inside the crate. */
export const NPM_PACKAGES = ['packages/api', 'packages/cli', 'packages/guest-shims'];

/** npm names whose dependency ranges inside this repository follow the version. */
export const NPM_INTERNAL = [
  'tauri-plugin-overwolf-api',
  'tauri-plugin-overwolf-cli',
  'tauri-plugin-overwolf-guest-shims',
];

/** Cargo packages of this repository (path packages in every Cargo.lock). */
export const CARGO_INTERNAL = [
  'tauri-plugin-overwolf',
  'tauri-plugin-overwolf-unstable',
  'ow-tauri-acl-tests',
];

/** The plugin crate's manifest, which pins the `unstable` helper crate. */
const PLUGIN_MANIFEST = 'crates/tauri-plugin-overwolf/Cargo.toml';

/** The JS runtime version the API package sends with every `adview_mount`. */
const RUNTIME_VERSION_FILE = 'packages/api/src/internal.ts';

const DEP_TABLES = ['dependencies', 'devDependencies', 'peerDependencies', 'optionalDependencies'];

// SemVer 2.0.0 (https://semver.org/#is-there-a-suggested-regular-expression-regex-to-check-a-semver-string).
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;

/**
 * Whether `version` is a SemVer 2.0.0 version without a leading `v`.
 * Build metadata (`+…`) is refused: crates.io and npm ignore it, so two
 * releases could not be told apart.
 */
export function isValidVersion(version) {
  const m = SEMVER.exec(version);
  return m !== null && m[5] === undefined;
}

/** `next` for a prerelease (`1.0.0-rc.1`), else `latest`. */
export function distTag(version) {
  if (!isValidVersion(version))
    throw new Error(`not a release version: ${JSON.stringify(version)}`);
  return version.includes('-') ? 'next' : 'latest';
}

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/**
 * A place that carries the version: `read` returns it (or throws), `write`
 * returns the file text with it replaced.
 */
function site(file, label, read, write) {
  return { file, label, read, write };
}

/** `version = "…"` inside the `[workspace.package]` table. */
function workspaceVersion() {
  const table = (text) => {
    const start = text.search(/^\[workspace\.package\][ \t]*$/m);
    if (start < 0) throw new Error('no [workspace.package] table');
    const rest = text.slice(start + 1);
    const next = rest.search(/^\[/m);
    const end = next < 0 ? text.length : start + 1 + next;
    return [start, end];
  };
  const re = /^version[ \t]*=[ \t]*"([^"]*)"/m;
  return site(
    'Cargo.toml',
    '[workspace.package] version',
    (text) => {
      const [a, b] = table(text);
      const m = re.exec(text.slice(a, b));
      if (!m) throw new Error('no version in [workspace.package]');
      return m[1];
    },
    (text, v) => {
      const [a, b] = table(text);
      return text.slice(0, a) + text.slice(a, b).replace(re, `version = "${v}"`) + text.slice(b);
    },
  );
}

/** The plugin's requirement on `tauri-plugin-overwolf-unstable`. */
function unstablePin() {
  const re =
    /^(tauri-plugin-overwolf-unstable[ \t]*=[ \t]*\{[^}\n]*\bversion[ \t]*=[ \t]*")([^"]*)(")/m;
  return site(
    PLUGIN_MANIFEST,
    'tauri-plugin-overwolf-unstable requirement',
    (text) => {
      const m = re.exec(text);
      if (!m) throw new Error('no versioned tauri-plugin-overwolf-unstable dependency');
      return stripRange(m[2]);
    },
    (text, v) => text.replace(re, (_, a, old, c) => `${a}${rangePrefix(old)}${v}${c}`),
  );
}

/** `export const RUNTIME_VERSION = '…';` */
function runtimeVersion() {
  const re = /^(export const RUNTIME_VERSION = ')([^']*)(';)$/m;
  return site(
    RUNTIME_VERSION_FILE,
    'RUNTIME_VERSION',
    (text) => {
      const m = re.exec(text);
      if (!m) throw new Error('no RUNTIME_VERSION constant');
      return m[2];
    },
    (text, v) => text.replace(re, `$1${v}$3`),
  );
}

/** The top-level `"version"` of a package.json (two-space or tab indent). */
function packageVersion(file) {
  const re = /^( {2}|\t)"version"(\s*:\s*)"([^"]*)"/m;
  return site(
    file,
    'version',
    (text) => {
      const json = JSON.parse(text);
      const m = re.exec(text);
      if (!m || m[3] !== json.version) throw new Error('no top-level "version"');
      return json.version;
    },
    (text, v) => text.replace(re, `$1"version"$2"${v}"`),
  );
}

/** A dependency range on one of this repository's npm packages. */
function npmPin(file, name) {
  const re = new RegExp(`("${escape(name)}"\\s*:\\s*")([^"]*)(")`, 'g');
  return site(
    file,
    `${name} range`,
    (text) => {
      const ranges = [...text.matchAll(re)].map((m) => m[2]);
      const versions = new Set(ranges.map(stripRange));
      if (versions.size !== 1) throw new Error(`ranges differ: ${ranges.join(', ')}`);
      return [...versions][0];
    },
    (text, v) => text.replace(re, (_, a, old, c) => `${a}${rangePrefix(old)}${v}${c}`),
  );
}

/** `^1.2.3` → `1.2.3`; anything that is not one version stays as written. */
function stripRange(range) {
  return range.replace(/^[\^~=]/, '');
}

function rangePrefix(range) {
  return /^[\^~=]/.test(range) ? range[0] : '';
}

/** Entries of the root package-lock.json: the workspaces' versions and ranges. */
function packageLock(file) {
  const edit = (text, fn) => {
    const json = JSON.parse(text);
    const indent = /^\{\r?\n([ \t]+)"/.exec(text)?.[1] ?? '  ';
    const eol = text.includes('\r\n') ? '\r\n' : '\n';
    fn(json);
    return JSON.stringify(json, null, indent).replace(/\n/g, eol) + eol;
  };
  const entries = (json) => {
    const out = [];
    for (const [key, entry] of Object.entries(json.packages ?? {})) {
      if (NPM_PACKAGES.includes(key) && entry.version !== undefined) {
        out.push({ get: () => entry.version, set: (v) => (entry.version = v) });
      }
      for (const table of DEP_TABLES) {
        for (const name of NPM_INTERNAL) {
          const deps = entry[table];
          if (deps && typeof deps[name] === 'string') {
            out.push({
              get: () => stripRange(deps[name]),
              set: (v) => (deps[name] = rangePrefix(deps[name]) + v),
            });
          }
        }
      }
    }
    return out;
  };
  return site(
    file,
    'workspace versions and ranges',
    (text) => {
      const versions = new Set(entries(JSON.parse(text)).map((e) => e.get()));
      if (versions.size === 0) throw new Error('no workspace entries');
      if (versions.size !== 1) throw new Error(`entries differ: ${[...versions].join(', ')}`);
      return [...versions][0];
    },
    (text, v) => edit(text, (json) => entries(json).forEach((e) => e.set(v))),
  );
}

/**
 * The `[[package]]` blocks of this repository's path packages in a Cargo.lock
 * (no `source` line: registry and git packages always have one).
 */
function cargoLock(file) {
  const blocks = (text) => {
    const out = [];
    const re =
      /^\[\[package\]\]\r?\nname = "([^"]+)"\r?\nversion = "([^"]*)"\r?\n((?:(?!\[\[package\]\])[\s\S])*)/gm;
    for (const m of text.matchAll(re)) {
      if (CARGO_INTERNAL.includes(m[1]) && !/^source = /m.test(m[3])) {
        out.push({ name: m[1], version: m[2], index: m.index, header: m[0].length - m[3].length });
      }
    }
    return out;
  };
  return site(
    file,
    'path packages',
    (text) => {
      const found = blocks(text);
      if (found.length === 0) throw new Error('no path package entries');
      const versions = new Set(found.map((b) => b.version));
      if (versions.size !== 1) {
        throw new Error(`entries differ: ${found.map((b) => `${b.name} ${b.version}`).join(', ')}`);
      }
      return [...versions][0];
    },
    (text, v) => {
      let out = text;
      for (const b of blocks(text).reverse()) {
        const head = out.slice(b.index, b.index + b.header);
        const next = head.replace(/^version = "[^"]*"/m, `version = "${v}"`);
        out = out.slice(0, b.index) + next + out.slice(b.index + b.header);
      }
      return out;
    },
  );
}

/** Workspace folders listed in the root package.json (`dir/*` globs expanded). */
function npmWorkspaces(root) {
  const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  const out = [];
  for (const pattern of pkg.workspaces ?? []) {
    if (pattern.endsWith('/*')) {
      const dir = pattern.slice(0, -2);
      if (!existsSync(join(root, dir))) continue;
      for (const name of readdirSync(join(root, dir)).sort()) {
        if (existsSync(join(root, dir, name, 'package.json'))) out.push(`${dir}/${name}`);
      }
    } else if (existsSync(join(root, pattern, 'package.json'))) {
      out.push(pattern);
    }
  }
  return out;
}

/** Cargo.lock files of the workspace and of the standalone Tauri apps that use the plugin by path. */
function cargoLocks(root) {
  const out = ['Cargo.lock'];
  const add = (dir) => {
    if (!existsSync(join(root, dir))) return;
    for (const name of readdirSync(join(root, dir)).sort()) {
      const lock = `${dir}/${name}/src-tauri/Cargo.lock`;
      if (existsSync(join(root, lock))) out.push(lock);
    }
  };
  add('examples');
  add('tools/parity-harness');
  return out.filter((f) => existsSync(join(root, f)));
}

/** Every place that carries the release version, for the checkout at `root`. */
export function sites(root = REPO_ROOT) {
  const out = [workspaceVersion(), unstablePin(), runtimeVersion()];
  for (const dir of NPM_PACKAGES) out.push(packageVersion(`${dir}/package.json`));
  for (const dir of npmWorkspaces(root)) {
    const file = `${dir}/package.json`;
    const text = readFileSync(join(root, file), 'utf8');
    const json = JSON.parse(text);
    for (const name of NPM_INTERNAL) {
      if (DEP_TABLES.some((t) => typeof json[t]?.[name] === 'string')) out.push(npmPin(file, name));
    }
  }
  if (existsSync(join(root, 'package-lock.json'))) out.push(packageLock('package-lock.json'));
  for (const lock of cargoLocks(root)) out.push(cargoLock(lock));
  return out;
}

/** Reads every site: `[{ file, label, version | error }]`. */
export function readVersions(root = REPO_ROOT) {
  return sites(root).map((s) => {
    try {
      return {
        file: s.file,
        label: s.label,
        version: s.read(readFileSync(join(root, s.file), 'utf8')),
      };
    } catch (err) {
      return { file: s.file, label: s.label, error: err.message };
    }
  });
}

/**
 * Checks that every site carries one version (and that it is `expect`, when
 * given). Returns `{ ok, version, rows, problems }`.
 */
export function check(root = REPO_ROOT, expect) {
  const rows = readVersions(root);
  const problems = [];
  for (const r of rows) if (r.error) problems.push(`${r.file} (${r.label}): ${r.error}`);
  const versions = new Set(rows.filter((r) => !r.error).map((r) => r.version));
  const version = rows.find((r) => !r.error)?.version;
  if (versions.size > 1) {
    for (const r of rows) {
      if (!r.error && r.version !== version) {
        problems.push(
          `${r.file} (${r.label}): ${r.version}, expected ${version} (as ${rows[0].file})`,
        );
      }
    }
  }
  if (version !== undefined && !isValidVersion(version)) {
    problems.push(`${version} is not a SemVer release version`);
  }
  if (expect !== undefined) {
    for (const r of rows) {
      if (!r.error && r.version !== expect && versions.size === 1) {
        problems.push(`${r.file} (${r.label}): ${r.version}, expected ${expect}`);
      }
    }
  }
  return { ok: problems.length === 0, version, rows, problems };
}

/** Writes `version` to every site. Returns the files it changed. */
export function set(root, version) {
  if (!isValidVersion(version)) {
    throw new Error(
      `not a release version: ${JSON.stringify(version)} (SemVer, no leading v, no +build)`,
    );
  }
  const byFile = new Map();
  for (const s of sites(root)) {
    if (!byFile.has(s.file)) byFile.set(s.file, []);
    byFile.get(s.file).push(s);
  }
  const changed = [];
  for (const [file, list] of byFile) {
    const path = join(root, file);
    const before = readFileSync(path, 'utf8');
    let text = before;
    for (const s of list) {
      s.read(text);
      text = s.write(text, version);
      const now = s.read(text);
      if (now !== version) throw new Error(`${file} (${s.label}): wrote ${now}, wanted ${version}`);
    }
    if (text !== before) {
      writeFileSync(path, text);
      changed.push(file);
    }
  }
  return changed;
}

function parseArgs(argv) {
  const out = { positional: [], root: REPO_ROOT, expect: undefined };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--root') out.root = resolve(argv[++i] ?? '');
    else if (a === '--expect') out.expect = argv[++i];
    else out.positional.push(a);
  }
  return out;
}

function main(argv) {
  const { positional, root, expect } = parseArgs(argv);
  const [command, version] = positional;
  if (command === 'check') {
    const result = check(root, expect);
    for (const r of result.rows) {
      console.log(`${(r.error ? 'ERROR' : r.version).padEnd(14)} ${r.file} (${r.label})`);
    }
    if (!result.ok) {
      console.error(`\nversion-sync: ${result.problems.length} problem(s):`);
      for (const p of result.problems) console.error(`  - ${p}`);
      console.error('\nFix with: node scripts/release/version-sync.mjs set <version>');
      return 1;
    }
    console.log(`\nversion-sync: ${result.rows.length} places carry ${result.version}.`);
    return 0;
  }
  if (command === 'set' && version !== undefined) {
    const changed = set(root, version);
    for (const f of changed) console.log(`updated ${f}`);
    const result = check(root, version);
    if (!result.ok) {
      for (const p of result.problems) console.error(`  - ${p}`);
      return 1;
    }
    console.log(`version-sync: ${result.rows.length} places carry ${version}.`);
    return 0;
  }
  if (command === 'dist-tag' && version !== undefined) {
    console.log(distTag(version));
    return 0;
  }
  console.error(
    'Usage: node scripts/release/version-sync.mjs check [--expect <version>] | set <version> | dist-tag <version>',
  );
  return 2;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (err) {
    console.error(`version-sync: ${err.message}`);
    process.exitCode = 1;
  }
}
