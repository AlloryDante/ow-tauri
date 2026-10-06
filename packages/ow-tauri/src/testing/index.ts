/**
 * `ow-tauri/testing`: helpers for unit tests of code that uses ow-tauri.
 *
 * Built on `@tauri-apps/api/mocks`: {@link mockHost} installs a fake plugin
 * that answers the commands of `docs/CONTRACT.md` with sensible defaults,
 * records every call, and lets the test push host messages into the
 * webview's channel. {@link setHostContext} replaces context detection
 * (CONTRACT B, "Context check"). Import this module from tests only.
 *
 * @example
 * ```ts
 * import { mockHost, settle } from 'ow-tauri/testing';
 * import { ipcMain } from 'ow-tauri/electron';
 *
 * const host = mockHost({ label: 'ow-main' });
 * ipcMain.handle('ping', () => 'pong');
 * host.push({ type: 'ipc', kind: 'invoke', id: 1, channel: 'ping', args: [],
 *             sender: { windowId: 1, label: 'bw-1', url: 'tauri://localhost/', frameId: 0 } });
 * await settle();
 * host.callsOf('ipc_reply'); // [{ id: 1, ok: true, value: 'pong', seq: 1 }]
 * ```
 *
 * @packageDocumentation
 */
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks';

import { attachRuntime } from '../bootstrap/install.js';
import { BOOTSTRAP_GLOBAL } from '../bootstrap/kernel.js';
import {
  PACKAGE_VERSION,
  PLUGIN,
  type HostContext,
  type HostMessage,
  type HostSnapshot,
} from '../shared/protocol.js';

export type { HostContext, HostMessage, HostSnapshot } from '../shared/protocol.js';

/**
 * Overrides the detected context of the current document's runtime.
 *
 * @param context - `'main'`, `'ui'`, `'none'`, or `null` to detect it from
 *   the webview label again
 */
export function setHostContext(context: HostContext | null): void {
  attachRuntime().setContextOverride(context);
}

/** A fake command implementation: receives the arguments, returns the response or throws. */
export type MockCommand = (args: Record<string, unknown>) => unknown;

/** One recorded command call. */
export interface MockCall {
  /** `ipc_send` for plugin commands, the full name (`plugin:window|show`) otherwise. */
  command: string;
  /** The arguments as the runtime passed them. */
  args: Record<string, unknown>;
}

/** Options for {@link mockHost}. */
export interface MockHostOptions {
  /** The webview label; decides the context. Default `ow-main`. */
  label?: string;
  /** Snapshot overrides for `__OW_TAURI_BOOTSTRAP__`; `null` removes the global. */
  snapshot?: Partial<HostSnapshot> | null;
  /** Command implementations, by name (`ipc_invoke`, `plugin:window|show`, ...). */
  commands?: Record<string, MockCommand>;
}

/** A fake plugin bound to the current document. */
export interface MockHost {
  /** Every command call, in order. */
  readonly calls: readonly MockCall[];
  /**
   * The arguments of every call of one command.
   *
   * @param command - the command name, as in {@link MockCall.command}
   * @returns the argument objects, in call order
   */
  callsOf(command: string): Record<string, unknown>[];
  /**
   * Delivers host messages as one channel payload (`HostMessage[]`).
   *
   * @param messages - the messages
   */
  push(...messages: HostMessage[]): void;
  /**
   * Replaces (or adds) a command implementation.
   *
   * @param command - the command name
   * @param implementation - the fake
   */
  setCommand(command: string, implementation: MockCommand): void;
  /** The epoch the last `ipc_subscribe` returned, or `undefined`. */
  readonly epoch: string | undefined;
  /** The snapshot the runtime was given. */
  readonly snapshot: HostSnapshot;
  /** Forgets recorded calls. */
  clearCalls(): void;
  /** Removes the Tauri mocks. */
  dispose(): void;
}

/**
 * A complete `HostSnapshot` with neutral values, for tests.
 *
 * @param overrides - fields to replace
 * @returns the snapshot
 */
export function defaultSnapshot(overrides: Partial<HostSnapshot> = {}): HostSnapshot {
  const display = {
    id: 1,
    label: 'Display 1',
    bounds: { x: 0, y: 0, width: 1920, height: 1080 },
    workArea: { x: 0, y: 0, width: 1920, height: 1040 },
    scaleFactor: 1,
  };
  return {
    seq: 0,
    versions: {
      owTauri: PACKAGE_VERSION,
      tauri: '2.12.1',
      app: '1.0.0',
      webview: 'test',
      os: 'test',
    },
    manifest: {
      name: 'test-app',
      productName: 'Test App',
      version: '1.0.0',
      author: 'ow-tauri contributors',
      overwolf: { packages: [] },
      buildOverwolf: {
        disableAdOptimization: false,
        enablePackageBundling: false,
        requireSigning: false,
        enableOWCertSigning: false,
      },
      raw: {},
    },
    identity: {
      uid: 'testuid',
      cuid: 'testuid',
      muid: 'testmuid',
      muidV2: 'testmuid',
      phasePercent: 50,
    },
    utmParams: null,
    switches: { argv: ['test-app'], testAd: true },
    paths: {
      appPath: '/app',
      appData: '/data',
      userData: '/data/Test App',
      sessionData: '/data/Test App',
      temp: '/tmp',
      home: '/home/user',
      desktop: '/home/user/Desktop',
      documents: '/home/user/Documents',
      downloads: '/home/user/Downloads',
      music: '/home/user/Music',
      pictures: '/home/user/Pictures',
      videos: '/home/user/Videos',
      logs: '/data/Test App/logs',
      exe: '/app/test-app',
      crashDumps: '/data/Test App/Crashpad',
    },
    isPackaged: false,
    locale: 'en-US',
    displays: [display],
    primaryDisplayId: 1,
    packages: null,
    flags: {
      anonymousAnalyticsDisabled: false,
      adsOptimizationDisabled: false,
      adsFpdDisabled: false,
    },
    platform: 'win32',
    arch: 'x64',
    ...overrides,
  };
}

interface ChannelLike {
  id: number;
}

/**
 * Installs a fake plugin for the current document and restarts the runtime
 * against it: registries are cleared, the snapshot is reloaded and the
 * channel is subscribed again.
 *
 * Defaults: `ipc_subscribe` returns a fresh epoch; `ipc_invoke` returns
 * increasing request ids from 1; `window_create` returns increasing window
 * ids from 1 with label `bw-<id>`; `bootstrap` returns the snapshot; every
 * other command resolves `null`.
 *
 * @param options - label, snapshot and command fakes
 * @returns the fake host
 */
export function mockHost(options: MockHostOptions = {}): MockHost {
  clearMocks();
  const label = options.label ?? 'ow-main';
  mockWindows(label);
  const snapshot = defaultSnapshot(options.snapshot ?? {});
  const globals = globalThis as Record<string, unknown>;
  if (options.snapshot === null) Reflect.deleteProperty(globals, BOOTSTRAP_GLOBAL);
  else globals[BOOTSTRAP_GLOBAL] = snapshot;

  const calls: MockCall[] = [];
  const commands = new Map<string, MockCommand>(Object.entries(options.commands ?? {}));
  let channel: ChannelLike | undefined;
  let channelIndex = 0;
  let epoch: string | undefined;
  let epochs = 0;
  let nextInvokeId = 1;
  let nextWindowId = 1;
  const prefix = `plugin:${PLUGIN}|`;

  const defaults: Record<string, MockCommand> = {
    ipc_subscribe: (args) => {
      channel = args['onMessage'] as ChannelLike;
      channelIndex = 0;
      epoch = `epoch-${String(++epochs)}`;
      return { epoch };
    },
    ipc_invoke: () => ({ id: nextInvokeId++ }),
    bootstrap: (): HostSnapshot => JSON.parse(JSON.stringify(snapshot)) as HostSnapshot,
    window_create: () => {
      const id = nextWindowId++;
      return { id, label: `bw-${String(id)}` };
    },
  };

  mockIPC((cmd, payload) => {
    const command = cmd.startsWith(prefix) ? cmd.slice(prefix.length) : cmd;
    const args = (payload ?? {}) as Record<string, unknown>;
    calls.push({ command, args });
    const implementation = commands.get(command) ?? defaults[command];
    return implementation ? implementation(args) : null;
  });

  const kernel = attachRuntime();
  kernel.setContextOverride(null);
  kernel.reset();
  void kernel.start();

  return {
    calls,
    callsOf: (command) => calls.filter((c) => c.command === command).map((c) => c.args),
    push: (...messages) => {
      if (!channel)
        throw new Error('mockHost.push: the runtime has not subscribed a channel (context none?)');
      const internals = (
        globalThis as {
          __TAURI_INTERNALS__?: { runCallback?: (id: number, data: unknown) => void };
        }
      ).__TAURI_INTERNALS__;
      internals?.runCallback?.(channel.id, { index: channelIndex++, message: messages });
    },
    setCommand: (command, implementation) => {
      commands.set(command, implementation);
    },
    get epoch() {
      return epoch;
    },
    snapshot,
    clearCalls: () => {
      calls.length = 0;
    },
    dispose: () => {
      clearMocks();
    },
  };
}

/**
 * Waits until queued promise callbacks and zero-delay timers have run, so
 * fire-and-forget commands and readiness steps complete.
 *
 * @param rounds - macrotask rounds to wait (default 5)
 */
export async function settle(rounds = 5): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
  }
}
