/// <reference types="node" />
/**
 * The `ow-tauri` command line: argument parsing and dispatch.
 *
 * @packageDocumentation
 */

import { doctor, formatFindings } from './doctor.js';
import { init } from './init.js';
import { migrate } from './migrate.js';
import type { Logger } from './sign.js';
import { sign } from './sign.js';
import { signExe } from './sign-exe.js';
import { findTauriDir, loadTauriConfig, targetOf, type LoadedConfig } from './tauri-config.js';

/** What {@link run} needs from the process. */
export interface CliIo {
  /** The environment. */
  readonly env: Readonly<Record<string, string | undefined>>;
  /** The working directory. */
  readonly cwd: string;
  /** `process.platform`: the default build target. */
  readonly platform: string;
  /** Messages. */
  readonly log: Logger;
  /** Reports, usage and dry-run output. */
  readonly out: (text: string) => void;
}

/** The usage text. */
export const USAGE = `Usage: ow-tauri <command> [options]   (run it from the local install: npm exec --no -- ow-tauri)

  init [--author <name>] [--name <app name>]
      Adds plugins.overwolf, the overwolf:default capability, the Windows NSIS
      hooks overlay and the /gen/overwolf .gitignore line. Safe to run again.
  migrate --from <package.json> [--write <tauri.conf.json>]
      Prints (and with --write merges) the plugins.overwolf block that keeps an
      ow-electron app's uid, name and Overwolf flags.
  doctor
      Read-only checks: resolved uid, capabilities, Rust uses that miss ad
      windows, version alignment, test ads.
  sign [--main <entry file>] [--out <dir>] [--write-uid] [--dry-run]
      Overwolf signing after the frontend build (plugins.overwolf.signing.enabled;
      OW_CLI_EMAIL, OW_CLI_API_KEY, OW_BUILD_KEY; OW_CLI_API_URL). Writes signed/
      in the project folder.
  sign-exe <file> [--app-exe <name.exe>] [--signed-dir <dir>] [--fallback "<cmd %1>"]
      For bundle.windows.signCommand: Overwolf certificate signing of the app exe.

Options of init, migrate, doctor and sign:
  --tauri-dir <dir>    the folder of tauri.conf.json (default: . or ./src-tauri)
  --config <json|file> extra configuration, merged like tauri build --config
                       (repeatable; replaces TAURI_CONFIG)
  --platform <os>      the build target: win32, darwin or linux (default: this OS)
`;

/** Parsed arguments: positionals and `--name value` / `--flag` options. */
export interface Parsed {
  /** The positional arguments. */
  readonly positional: string[];
  /** The options; a repeated option keeps every value. */
  readonly options: Map<string, string[] | true>;
}

const FLAGS = new Set(['dry-run', 'help', 'write-uid']);

/**
 * Parses `argv` (without the node and script paths).
 *
 * @param argv - the arguments
 * @returns positionals and options
 */
export function parseArgs(argv: readonly string[]): Parsed {
  const positional: string[] = [];
  const options = new Map<string, string[] | true>();
  const add = (name: string, value: string): void => {
    const current = options.get(name);
    options.set(name, Array.isArray(current) ? [...current, value] : [value]);
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i] ?? '';
    if (!arg.startsWith('--')) {
      positional.push(arg);
      continue;
    }
    const eq = arg.indexOf('=');
    const name = eq === -1 ? arg.slice(2) : arg.slice(2, eq);
    if (eq !== -1) {
      add(name, arg.slice(eq + 1));
    } else if (FLAGS.has(name)) {
      options.set(name, true);
    } else {
      const value = argv[i + 1];
      if (value === undefined) throw new Error(`--${name} needs a value`);
      add(name, value);
      i++;
    }
  }
  return { positional, options };
}

/** The last value of an option. */
function text(options: Parsed['options'], name: string): string | undefined {
  const value = options.get(name);
  return Array.isArray(value) ? value.at(-1) : undefined;
}

/** Every value of a repeatable option. */
function all(options: Parsed['options'], name: string): string[] {
  const value = options.get(name);
  return Array.isArray(value) ? value : [];
}

function check(options: Parsed['options'], allowed: readonly string[]): void {
  for (const name of options.keys()) {
    if (!allowed.includes(name)) throw new Error(`unknown option --${name}`);
  }
}

/** Options every config-reading command accepts. */
const CONFIG_OPTIONS = ['tauri-dir', 'config', 'platform'];

async function loadConfig(options: Parsed['options'], io: CliIo): Promise<LoadedConfig> {
  return await loadTauriConfig({
    tauriDir: findTauriDir(io.cwd, text(options, 'tauri-dir')),
    target: targetOf(text(options, 'platform') ?? io.platform),
    env: io.env,
    configs: all(options, 'config'),
    cwd: io.cwd,
  });
}

async function dispatch(
  command: string,
  rest: string[],
  parsed: Parsed,
  io: CliIo,
): Promise<number> {
  const { options } = parsed;
  switch (command) {
    case 'sign': {
      check(options, [...CONFIG_OPTIONS, 'main', 'out', 'write-uid', 'dry-run']);
      await sign({
        loaded: await loadConfig(options, io),
        main: text(options, 'main'),
        cwd: io.cwd,
        outDir: text(options, 'out'),
        platform: text(options, 'platform') ?? io.platform,
        dryRun: options.has('dry-run'),
        writeUid: options.has('write-uid'),
        env: io.env,
        log: io.log,
      });
      return 0;
    }
    case 'sign-exe': {
      check(options, ['app-exe', 'signed-dir', 'fallback']);
      const [file] = rest;
      if (file === undefined) throw new Error('sign-exe needs the file to sign');
      await signExe({
        file,
        appExe: text(options, 'app-exe'),
        signedDir: text(options, 'signed-dir'),
        fallback: text(options, 'fallback'),
        cwd: io.cwd,
        env: io.env,
        log: io.log,
      });
      return 0;
    }
    case 'migrate': {
      check(options, ['from', 'write']);
      const from = text(options, 'from');
      if (from === undefined) throw new Error('migrate needs --from <package.json>');
      await migrate({ from, write: text(options, 'write'), cwd: io.cwd, log: io.log, out: io.out });
      return 0;
    }
    case 'init': {
      check(options, ['tauri-dir', 'author', 'name']);
      await init({
        tauriDir: findTauriDir(io.cwd, text(options, 'tauri-dir')),
        author: text(options, 'author'),
        name: text(options, 'name'),
        log: io.log,
      });
      return 0;
    }
    case 'doctor': {
      check(options, CONFIG_OPTIONS);
      const findings = await doctor(await loadConfig(options, io));
      io.out(formatFindings(findings));
      return findings.some((f) => f.level === 'error') ? 1 : 0;
    }
    default:
      throw new Error(`unknown command "${command}"`);
  }
}

/**
 * Runs the CLI.
 *
 * @param argv - the arguments
 * @param io - the process
 * @returns the exit code
 */
export async function run(argv: readonly string[], io: CliIo): Promise<number> {
  try {
    const parsed = parseArgs(argv);
    const [command, ...rest] = parsed.positional;
    if (command === undefined || command === 'help' || parsed.options.has('help')) {
      io.out(USAGE);
      return command === undefined && !parsed.options.has('help') ? 1 : 0;
    }
    return await dispatch(command, rest, parsed, io);
  } catch (error) {
    io.log.warn((error as Error).message);
    return 1;
  }
}
