#!/usr/bin/env node
// MUID probe: tests candidate derivations of ow-electron's machine id (muid)
// against the value a harness run observed (app.overwolf.muid), using this
// machine's OS identifiers. Raw identifiers are never printed or saved; the
// output names only the derivation that matched.
//
//   node muid-probe.mjs                       # muid from the newest capture
//   node muid-probe.mjs --muid <uuid> --phase <n>
//   node muid-probe.mjs --experiment          # macOS: substitute identifiers
//
// --experiment launches ow-electron (offline, hidden) with a stand-in `ioreg`
// first on PATH that reports made-up IOPlatformUUID values. If the muid
// follows the stand-in, that both shows where ow-electron reads the machine id
// and pins the derivation down on inputs chosen to tell candidates apart (for
// example whether UUID version/variant bits are forced).

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { launch, makeAppDir } from './lib/launch.mjs';
import { isolationEnv } from './lib/paths.mjs';

const harnessDir = dirname(fileURLToPath(import.meta.url));

/** RFC 4122 namespaces, plus the nil UUID. */
const NAMESPACES = {
  DNS: '6ba7b810-9dad-11d1-80b4-00c04fd430c8',
  URL: '6ba7b811-9dad-11d1-80b4-00c04fd430c8',
  OID: '6ba7b812-9dad-11d1-80b4-00c04fd430c8',
  X500: '6ba7b814-9dad-11d1-80b4-00c04fd430c8',
  NIL: '00000000-0000-0000-0000-000000000000',
};

const hex = (algo, input) => createHash(algo).update(input).digest('hex');

/** Formats 32+ hex chars as a GUID (8-4-4-4-12). */
const asGuid = (h) =>
  `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20, 32)}`;

/** Name-based UUID (v3 md5 / v5 sha1) per RFC 4122 section 4.3. */
function nameUuid(version, namespace, name) {
  const ns = Buffer.from(namespace.replace(/-/g, ''), 'hex');
  const digest = createHash(version === 5 ? 'sha1' : 'md5')
    .update(Buffer.concat([ns, Buffer.from(name, 'utf8')]))
    .digest();
  digest[6] = (digest[6] & 0x0f) | (version << 4);
  digest[8] = (digest[8] & 0x3f) | 0x80;
  return asGuid(digest.subarray(0, 16).toString('hex'));
}

/** OS machine identifiers of this machine, keyed by a descriptive label. */
function machineIds() {
  const ids = {};
  const run = (cmd, args) => {
    try {
      return execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
    } catch {
      return '';
    }
  };
  if (process.platform === 'darwin') {
    const io = run('ioreg', ['-rd1', '-c', 'IOPlatformExpertDevice']);
    const pick = (key) => new RegExp(`"${key}" = "([^"]+)"`).exec(io)?.[1];
    if (pick('IOPlatformUUID')) ids.IOPlatformUUID = pick('IOPlatformUUID');
    if (pick('IOPlatformSerialNumber')) ids.IOPlatformSerialNumber = pick('IOPlatformSerialNumber');
    const kern = run('sysctl', ['-n', 'kern.uuid']).trim();
    if (kern) ids['kern.uuid'] = kern;
  } else if (process.platform === 'win32') {
    const reg = run('reg', [
      'query',
      'HKLM\\SOFTWARE\\Microsoft\\Cryptography',
      '/v',
      'MachineGuid',
    ]);
    const guid = /MachineGuid\s+REG_SZ\s+(\S+)/.exec(reg)?.[1];
    if (guid) ids.MachineGuid = guid;
  } else {
    for (const file of ['/etc/machine-id', '/var/lib/dbus/machine-id']) {
      if (existsSync(file)) ids[file] = readFileSync(file, 'utf8').trim();
    }
  }
  return ids;
}

/** Every candidate muid: label -> value. */
function candidates(ids) {
  const out = new Map();
  for (const [idLabel, raw] of Object.entries(ids)) {
    const forms = {
      [idLabel]: raw,
      [`lower(${idLabel})`]: raw.toLowerCase(),
      [`upper(${idLabel})`]: raw.toUpperCase(),
      [`nodash(${idLabel})`]: raw.replace(/-/g, ''),
      [`lower(nodash(${idLabel}))`]: raw.replace(/-/g, '').toLowerCase(),
    };
    // node-machine-id style: sha256 hex of the raw id.
    forms[`sha256hex(${idLabel})`] = hex('sha256', raw);
    forms[`sha256hex(lower(${idLabel}))`] = hex('sha256', raw.toLowerCase());
    for (const [formLabel, value] of Object.entries(forms)) {
      out.set(`raw:${formLabel}`, value);
      for (const algo of ['md5', 'sha1', 'sha256']) {
        out.set(`guid(${algo}(${formLabel}))`, asGuid(hex(algo, value)));
      }
      for (const [nsLabel, ns] of Object.entries(NAMESPACES)) {
        out.set(`uuidv5(${nsLabel}, ${formLabel})`, nameUuid(5, ns, value));
        out.set(`uuidv3(${nsLabel}, ${formLabel})`, nameUuid(3, ns, value));
      }
    }
  }
  return out;
}

/** phasePercent candidates computed from a muid. */
function phaseCandidates(muid) {
  const sumCodes = (s) => [...s].reduce((n, ch) => n + ch.charCodeAt(0), 0);
  const plain = muid.replace(/-/g, '');
  const out = {};
  for (const [label, input] of Object.entries({
    'muid without dashes': plain,
    'upper(muid without dashes)': plain.toUpperCase(),
    muid,
  })) {
    out[`sum(charCodes(md5hex(${label}))) % 100`] = sumCodes(hex('md5', input)) % 100;
    out[`sum(charCodes(${label})) % 100`] = sumCodes(input) % 100;
  }
  return out;
}

function newestObservedMuid() {
  const root = join(harnessDir, 'captures');
  if (!existsSync(root)) return null;
  const files = readdirSync(root)
    .map((d) => join(root, d, 'overwolf.json'))
    .filter((f) => existsSync(f))
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  for (const file of files) {
    const members = JSON.parse(readFileSync(file, 'utf8')).snapshots?.[0]?.members;
    if (members?.muid?.value) {
      return { muid: members.muid.value, phase: members.phasePercent?.value, source: file };
    }
  }
  return null;
}

/** Sets RFC 4122 version 5 and variant bits on a GUID string. */
function forceV5Bits(guid) {
  const b = Buffer.from(guid.replace(/-/g, ''), 'hex');
  b[6] = (b[6] & 0x0f) | 0x50;
  b[8] = (b[8] & 0x3f) | 0x80;
  return asGuid(b.toString('hex'));
}

/** Deterministic made-up platform UUIDs (upper case, like ioreg prints). */
function fakeUuids(count) {
  const out = [];
  for (let i = 0; out.length < count; i += 1) {
    out.push(asGuid(hex('sha1', `parity-harness-fake-machine-${i}`)).toUpperCase());
  }
  return out;
}

async function experiment() {
  if (process.platform !== 'darwin') {
    console.error('--experiment substitutes ioreg and runs on macOS only.');
    process.exit(2);
  }
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  const outDir = join(harnessDir, 'captures', `muid-experiment-${stamp}`);
  const binDir = join(outDir, 'bin');
  mkdirSync(binDir, { recursive: true });
  const home = join(outDir, 'home');
  mkdirSync(home, { recursive: true });
  const rows = [];
  for (const fake of fakeUuids(4)) {
    // A stand-in ioreg that prints the same shape as the real command.
    const script = join(binDir, 'ioreg');
    writeFileSync(
      script,
      `#!/bin/sh\ncat <<'IOREG'\n+-o Mac  <class IOPlatformExpertDevice, id 0x100000000, registered, matched, active, busy 0 (0 ms), retain 30>\n  {\n    "IOPlatformSerialNumber" = "PARITY000000"\n    "IOPlatformUUID" = "${fake}"\n  }\nIOREG\n`,
    );
    chmodSync(script, 0o755);
    const runDir = join(outDir, fake);
    mkdirSync(runDir, { recursive: true });
    const appDir = join(runDir, 'app');
    makeAppDir(appDir, {
      name: 'muid-experiment',
      productName: 'MUID Experiment',
      version: '0.1.0',
      author: { name: 'Example Studio' },
    });
    const configPath = join(runDir, 'config.json');
    writeFileSync(
      configPath,
      JSON.stringify({ runDir, probeOnly: true, probeDelayMs: 300, present: 'hidden' }),
    );
    await launch({
      appDir,
      switches: ['--use-mock-keychain', '--proxy-server=127.0.0.1:9', '--test-ad'],
      env: {
        ...isolationEnv(home),
        PATH: `${binDir}:${process.env.PATH}`,
        PARITY_HARNESS_CONFIG: configPath,
      },
      logDir: runDir,
      timeoutMs: 30_000,
    });
    const file = join(runDir, 'overwolf.json');
    const members = existsSync(file)
      ? JSON.parse(readFileSync(file, 'utf8')).snapshots?.[0]?.members
      : null;
    const observed = members?.muid?.value ?? null;
    const plain = asGuid(hex('sha256', fake.toLowerCase()));
    const plainUpper = asGuid(hex('sha256', fake));
    const row = {
      fakeIOPlatformUUID: fake,
      observedMuid: observed,
      observedPhasePercent: members?.phasePercent?.value ?? null,
      'guid(sha256(lower(id)))': plain,
      'guid(sha256(id))': plainUpper,
      'v5bits(guid(sha256(lower(id))))': forceV5Bits(plain),
      matches: [],
    };
    for (const key of [
      'guid(sha256(lower(id)))',
      'guid(sha256(id))',
      'v5bits(guid(sha256(lower(id))))',
    ]) {
      if (observed && row[key] === observed) row.matches.push(key);
    }
    row.phaseMatches = observed
      ? Object.entries(phaseCandidates(observed))
          .filter(([, v]) => v === row.observedPhasePercent)
          .map(([k]) => k)
      : [];
    rows.push(row);
    console.error(
      `${fake}: ${observed} -> ${row.matches.join(', ') || 'no match (machine id not read through ioreg?)'}`,
    );
  }
  writeFileSync(join(outDir, 'muid-experiment.json'), JSON.stringify(rows, null, 2) + '\n');
  console.log(JSON.stringify(rows, null, 2));
}

async function main() {
  const { values } = parseArgs({
    options: {
      muid: { type: 'string' },
      phase: { type: 'string' },
      experiment: { type: 'boolean', default: false },
    },
  });
  if (values.experiment) return experiment();
  const observed = values.muid
    ? {
        muid: values.muid,
        phase: values.phase === undefined ? undefined : Number(values.phase),
        source: 'cli',
      }
    : newestObservedMuid();
  if (!observed) {
    console.error('No observed muid: run `node run.mjs` first or pass --muid.');
    process.exit(2);
  }
  const ids = machineIds();
  const all = candidates(ids);
  const target = observed.muid.toLowerCase();
  const muidMatches = [...all]
    .filter(([, v]) => v.toLowerCase() === target)
    .map(([label]) => label);
  const phases = phaseCandidates(observed.muid);
  const phaseMatches = Object.entries(phases)
    .filter(([, v]) => v === observed.phase)
    .map(([label]) => label);
  const result = {
    platform: process.platform,
    observedFrom: observed.source,
    observedMuidShape: {
      version: observed.muid[14],
      variant: observed.muid[19],
      lowerCase: observed.muid === observed.muid.toLowerCase(),
    },
    identifiersTried: Object.keys(ids),
    candidatesTried: all.size,
    muidMatches,
    observedPhasePercent: observed.phase ?? null,
    phaseMatches,
  };
  const out = join(harnessDir, 'captures', 'muid-probe.json');
  writeFileSync(out, JSON.stringify(result, null, 2) + '\n');
  console.log(JSON.stringify(result, null, 2));
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
