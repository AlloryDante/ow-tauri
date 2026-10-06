#!/usr/bin/env node
// UID matrix: launches ow-electron once per package.json variant (author
// shape x name fields), reads app.overwolf.uid, and explains each observed uid
// with the (author, name) inputs that reproduce it. Also cross-checks every
// explained pair with Overwolf's CLI (`ow client calc-electron-uid`).
//
// Launches are offline (--proxy-server pointing at a closed port), so the
// probes send no analytics for the throwaway app identities.
//
//   node uid-matrix.mjs            # full matrix
//   node uid-matrix.mjs --quick    # a few variants

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { launch, makeAppDir, owElectronVersion } from './lib/launch.mjs';
import { waitForQuietMachine } from './lib/load-guard.mjs';
import { isolationEnv } from './lib/paths.mjs';
import { uidCandidates } from './lib/uid.mjs';

const require = createRequire(import.meta.url);
const harnessDir = dirname(fileURLToPath(import.meta.url));

const AUTHORS = {
  object: { name: 'Example Studio' },
  'object+email': { name: 'Example Studio', email: 'dev@example.com', url: 'https://example.com' },
  string: 'Example Studio',
  'string-ltd': 'Overwolf Ltd.',
  'npm-string': 'Example Studio <dev@example.com> (https://example.com)',
  'npm-email-only': 'Example Studio <dev@example.com>',
  'empty-object': {},
  'empty-string': '',
  missing: undefined,
};

const NAMES = {
  'name-only': { name: 'parity-harness' },
  'name+productName': { name: 'parity-harness', productName: 'Parity Harness' },
  'name+build.productName': { name: 'parity-harness', build: { productName: 'Parity Build Name' } },
  'all-three': {
    name: 'parity-harness',
    productName: 'Parity Harness',
    build: { productName: 'Parity Build Name' },
  },
};

const EXTRAS = {
  'overwolf.uid-set': {
    name: 'parity-harness',
    productName: 'Parity Harness',
    author: { name: 'Example Studio' },
    overwolf: { uid: 'aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj' },
  },
  'non-ascii-name': {
    name: 'parity-unicode',
    productName: 'Pârity Ünicode',
    author: { name: 'Exämple' },
  },
  'quote-in-name': { name: 'quote-test', productName: "O'Brien Tools", author: { name: "D'Arcy" } },
  'spaces-around': {
    name: 'spaces-test',
    productName: ' Parity Harness ',
    author: { name: ' Example Studio ' },
  },
};

function variants(quick) {
  const out = [];
  for (const [authorKey, author] of Object.entries(AUTHORS)) {
    for (const [nameKey, names] of Object.entries(NAMES)) {
      if (quick && !['object', 'string', 'missing'].includes(authorKey)) continue;
      out.push({
        id: `${authorKey}__${nameKey}`,
        pkg: { ...names, version: '0.1.0', ...(author === undefined ? {} : { author }) },
      });
    }
  }
  for (const [id, pkg] of Object.entries(EXTRAS))
    out.push({ id, pkg: { version: '0.1.0', ...pkg } });
  return out;
}

function owCli(name, author) {
  const bin = join(dirname(require.resolve('@overwolf/ow-cli/package.json')), 'bin', 'ow.js');
  try {
    const out = execFileSync(
      process.execPath,
      [bin, 'client', 'calc-electron-uid', '-n', name, '-a', author],
      {
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'pipe'],
      },
    );
    return out.trim().split('\n').pop().trim();
  } catch (error) {
    return `error: ${
      String(error.stderr || error.message)
        .trim()
        .split('\n')[0]
    }`;
  }
}

async function main() {
  const { values } = parseArgs({
    options: {
      quick: { type: 'boolean', default: false },
      'no-wait': { type: 'boolean', default: false },
    },
  });
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  const outDir = join(harnessDir, 'captures', `uid-matrix-${stamp}`);
  const home = join(outDir, 'home');
  mkdirSync(home, { recursive: true });
  const homeEnv = isolationEnv(home) ?? {};
  if (!values['no-wait']) await waitForQuietMachine();

  const rows = [];
  for (const variant of variants(values.quick)) {
    const runDir = join(outDir, variant.id);
    const appDir = join(runDir, 'app');
    mkdirSync(runDir, { recursive: true });
    makeAppDir(appDir, variant.pkg);
    const configPath = join(runDir, 'config.json');
    writeFileSync(
      configPath,
      JSON.stringify({ runDir, probeOnly: true, probeDelayMs: 300, present: 'hidden' }),
    );
    const exit = await launch({
      appDir,
      switches: ['--use-mock-keychain', '--proxy-server=127.0.0.1:9', '--test-ad'],
      env: { ...homeEnv, PARITY_HARNESS_CONFIG: configPath },
      logDir: runDir,
      timeoutMs: 30_000,
    });
    const file = join(runDir, 'overwolf.json');
    const observed = existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : null;
    const first = observed?.snapshots?.[0];
    const uid = first?.members?.uid?.value ?? null;
    const appName = observed?.appName ?? null;
    const matches = uid ? uidCandidates(variant.pkg, appName).filter((c) => c.uid === uid) : [];
    const cli = matches[0] ? owCli(matches[0].name, matches[0].author) : null;
    const row = {
      id: variant.id,
      packageJson: variant.pkg,
      exit: exit.code,
      appGetName: appName,
      uid,
      envUid: first?.env?.OVERWOLF_APP_UID ?? null,
      matches: matches.map((m) => ({
        author: m.authorSource,
        name: m.nameSource,
        authorValue: m.author,
        nameValue: m.name,
      })),
      owCliForMatch: cli,
      owCliAgrees: cli === uid,
    };
    rows.push(row);
    console.error(
      `${variant.id}: ${uid} <- ${row.matches.map((m) => `${m.author} + ${m.name}`).join(' | ') || 'NO MATCH'}`,
    );
  }

  const result = {
    owElectron: owElectronVersion(),
    platform: process.platform,
    createdAt: new Date().toISOString(),
    rows,
  };
  writeFileSync(join(outDir, 'uid-matrix.json'), JSON.stringify(result, null, 2) + '\n');
  const md = [
    `# UID matrix (ow-electron ${result.owElectron}, ${process.platform})`,
    '',
    '| Variant | app.getName() | Observed uid | Explained by (author + name) | ow CLI agrees |',
    '|---|---|---|---|---|',
    ...rows.map(
      (r) =>
        `| ${r.id} | ${r.appGetName ?? ''} | \`${r.uid}\` | ${r.matches.map((m) => `${m.author} + ${m.name}`).join('<br>') || '**none**'} | ${r.owCliAgrees ? 'yes' : 'no'} |`,
    ),
    '',
  ].join('\n');
  writeFileSync(join(outDir, 'uid-matrix.md'), md);
  console.log(join(outDir, 'uid-matrix.md'));
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
