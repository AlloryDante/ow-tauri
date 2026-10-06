/// <reference types="node" />
/**
 * The `ow-tauri` command line: argument parsing and dispatch.
 *
 * @packageDocumentation
 */

import { resolve } from 'node:path';

import type { Logger } from './sign.js';
import { sign } from './sign.js';
import { signExe } from './sign-exe.js';

/** What {@link run} needs from the process. */
export interface CliIo {
  /** The environment. */
  readonly env: Record<string, string | undefined>;
  /** The working directory. */
  readonly cwd: string;
  /** `process.platform` of the build target. */
  readonly platform: string;
  /** Messages. */
  readonly log: Logger;
  /** Usage and dry-run output. */
  readonly out: (text: string) => void;
}

/** The usage text. */
export const USAGE = `Usage:
  ow-tauri sign [--package <package.json>] [--main <entry file>] [--project-dir <dir>]
                [--out <dir>] [--platform win32|darwin|linux] [--dry-run]
      Signs the app with Overwolf's signing service (OW_CLI_EMAIL, OW_CLI_API_KEY,
      OW_BUILD_KEY; OW_CLI_API_URL) and writes ow-tauri-signed/ next to package.json.
  ow-tauri sign-exe <file> [--app-exe <name.exe>] [--signed-dir <dir>] [--fallback "<cmd %1>"]
      For bundle.windows.signCommand: Overwolf certificate signing of the app exe.
`;

/** Parsed arguments: positionals and `--name value` / `--flag` options. */
interface Parsed {
  readonly positional: string[];
  readonly options: Map<string, string | true>;
}

const FLAGS = new Set(['dry-run', 'help']);

/**
 * Parses `argv` (without the node and script paths).
 *
 * @param argv - the arguments
 * @returns positionals and options
 */
export function parseArgs(argv: readonly string[]): Parsed {
  const positional: string[] = [];
  const options = new Map<string, string | true>();
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i] ?? '';
    if (!arg.startsWith('--')) {
      positional.push(arg);
      continue;
    }
    const eq = arg.indexOf('=');
    const name = eq === -1 ? arg.slice(2) : arg.slice(2, eq);
    if (eq !== -1) {
      options.set(name, arg.slice(eq + 1));
    } else if (FLAGS.has(name)) {
      options.set(name, true);
    } else {
      const value = argv[i + 1];
      if (value === undefined) throw new Error(`--${name} needs a value`);
      options.set(name, value);
      i++;
    }
  }
  return { positional, options };
}

function text(options: Map<string, string | true>, name: string): string | undefined {
  const value = options.get(name);
  return typeof value === 'string' ? value : undefined;
}

function check(options: Map<string, string | true>, allowed: readonly string[]): void {
  for (const name of options.keys()) {
    if (!allowed.includes(name)) throw new Error(`unknown option --${name}`);
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
    const { positional, options } = parseArgs(argv);
    const [command, ...rest] = positional;
    if (command === undefined || command === 'help' || options.has('help')) {
      io.out(USAGE);
      return command === undefined && !options.has('help') ? 1 : 0;
    }
    if (command === 'sign') {
      check(options, ['package', 'main', 'project-dir', 'out', 'platform', 'dry-run']);
      await sign({
        packageJson: resolve(io.cwd, text(options, 'package') ?? 'package.json'),
        main: text(options, 'main'),
        projectDir: text(options, 'project-dir'),
        outDir: text(options, 'out'),
        platform: text(options, 'platform') ?? io.platform,
        dryRun: options.has('dry-run'),
        env: io.env,
        log: io.log,
      });
      return 0;
    }
    if (command === 'sign-exe') {
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
    throw new Error(`unknown command "${command}"`);
  } catch (error) {
    io.log.warn((error as Error).message);
    return 1;
  }
}
