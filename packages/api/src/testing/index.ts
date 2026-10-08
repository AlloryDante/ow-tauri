/**
 * `tauri-plugin-overwolf-api/testing`: a fake plugin for unit tests of app
 * code that uses this package (Vitest, Jest with a DOM environment).
 *
 * Built on `@tauri-apps/api/mocks`: {@link mockOverwolf} answers every
 * plugin command with sensible defaults, records the calls, keeps track of
 * mounted `<owadview>` elements and lets the test deliver ad events on their
 * channels. Import it from tests only.
 *
 * @example
 * ```ts
 * import { afterEach, expect, it } from 'vitest';
 * import { mockOverwolf, settle, type MockOverwolf } from 'tauri-plugin-overwolf-api/testing';
 *
 * let overwolf: MockOverwolf;
 * afterEach(() => overwolf.restore());
 *
 * it('shows the ad', async () => {
 *   overwolf = mockOverwolf({ info: { testAd: true } });
 *   await import('tauri-plugin-overwolf-api/adview');
 *   const ad = document.createElement('owadview');
 *   document.body.append(ad);
 *   await settle();
 *   const [mount] = overwolf.mounts();
 *   overwolf.emit(mount.elementId, 'display_ad_loaded', {});
 * });
 * ```
 *
 * @packageDocumentation
 */
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks';

import type { OverwolfErrorWire } from '../errors.js';
import { PLUGIN } from '../internal.js';
import type { EmailHashes, MachineIds, OverwolfInfo } from '../types.js';

/**
 * A fake command: receives the arguments, returns the result (or a promise
 * of it) or throws (a thrown {@link OverwolfErrorWire} rejects like the
 * plugin does). `original` is the default fake of the same command, so an
 * override can wrap it, e.g. to hold `adview_mount` open while a test
 * delivers events.
 */
export type MockCommand = (
  args: Record<string, unknown>,
  original: (args: Record<string, unknown>) => unknown,
) => unknown;

/** One recorded plugin call. */
export interface MockCall {
  /** The command name without the `plugin:overwolf|` prefix. */
  command: string;
  /** The arguments as passed to `invoke`. */
  args: Record<string, unknown>;
}

/** A mounted `<owadview>` as the fake plugin sees it. */
export interface MockMount {
  /** The runtime's element id. */
  readonly elementId: string;
  /** The guest label the fake assigned (`owad-<n>`). */
  readonly guestLabel: string;
  /** The `adview_mount` request. */
  readonly request: Record<string, unknown>;
}

/** Options of {@link mockOverwolf}. */
export interface MockOverwolfOptions {
  /** The label of the current webview (and window). Default `main`. */
  label?: string;
  /** Overrides of the default {@link OverwolfInfo}. */
  info?: Partial<OverwolfInfo>;
  /** What `getMachineIds()` returns. */
  machineIds?: MachineIds;
  /** What `isCMPRequired()` returns. Default `false`. */
  cmpRequired?: boolean;
  /** Command implementations by name (without the prefix); they replace the defaults. */
  commands?: Record<string, MockCommand>;
}

/** The fake plugin. */
export interface MockOverwolf {
  /** Every plugin call, in order. */
  readonly calls: readonly MockCall[];
  /**
   * The arguments of every call of one command.
   *
   * @param command - the command name, without the prefix
   * @returns the argument objects, in call order
   */
  callsOf(command: string): Record<string, unknown>[];
  /**
   * Replaces (or adds) a command implementation.
   *
   * @param command - the command name, without the prefix
   * @param implementation - the fake
   */
  setCommand(command: string, implementation: MockCommand): void;
  /**
   * The mounted `<owadview>` elements, in mount order.
   *
   * @returns the mounts
   */
  mounts(): MockMount[];
  /**
   * Delivers an event on a mounted element's channel, as the plugin does
   * for an ad page event (`source: 'guest'`) or a host lifecycle event.
   *
   * @param elementId - the element id of the mount
   * @param name - the event name
   * @param data - the payload
   * @param source - who produced it; default `guest`
   */
  emit(elementId: string, name: string, data?: unknown, source?: 'host' | 'guest'): void;
  /** Removes the mocks (`clearMocks()`). */
  restore(): void;
}

/** The {@link OverwolfInfo} the fake returns unless overridden. */
export const DEFAULT_INFO: OverwolfInfo = Object.freeze({
  uid: 'aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj',
  appCuid: 'aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj',
  phasePercent: 50,
  utmParams: null,
  testAd: true,
  adsSupported: true,
  name: 'Test App',
  version: '1.0.0',
  host: Object.freeze({ label: 'tauri', version: '2.12.1', owVersion: '42.11.4' }),
});

/** Fake hashes (not real digests of anything). */
const FAKE_HASHES: EmailHashes = Object.freeze({
  sha1: '0'.repeat(40),
  md5: '0'.repeat(32),
  sha256: '0'.repeat(64),
});

interface ChannelLike {
  id: number;
}

function isChannel(value: unknown): value is ChannelLike {
  return (
    typeof value === 'object' && value !== null && typeof (value as ChannelLike).id === 'number'
  );
}

/** The part of `window.__TAURI_INTERNALS__` the fake drives. */
interface Internals {
  runCallback(id: number, data: unknown): void;
}

/** Rejects the way the plugin does: with the wire object itself, not an `Error`. */
function rejectWith(code: OverwolfErrorWire['code'], message: string): never {
  const wire: OverwolfErrorWire = { code, message };
  // eslint-disable-next-line @typescript-eslint/only-throw-error -- the plugin rejects with plain wire objects
  throw wire;
}

/**
 * Installs the fake plugin for the current document.
 *
 * @param options - label, identity and command overrides
 * @returns the fake
 */
export function mockOverwolf(options: MockOverwolfOptions = {}): MockOverwolf {
  const label = options.label ?? 'main';
  mockWindows(label);
  const calls: MockCall[] = [];
  const live = new Map<string, MockMount & { channel: ChannelLike; next: number }>();
  let guests = 0;
  const info: OverwolfInfo = { ...DEFAULT_INFO, ...options.info };
  const defaults: Record<string, (args: Record<string, unknown>) => unknown> = {
    get_info: () => info,
    get_machine_ids: () => options.machineIds ?? { muid: 'mock-muid-v2', muidV2: 'mock-muid-v2' },
    is_cmp_required: () => options.cmpRequired ?? false,
    open_ad_privacy_settings_window: () => null,
    open_cmp_window: () => null,
    generate_user_email_hashes: () => FAKE_HASHES,
    set_user_email_hashes: () => null,
    clear_user_email_hashes: () => null,
    disable_anonymous_analytics: () => null,
    disable_ads_optimization: () => null,
    disable_ads_fpd: () => null,
    set_anonymous_analytics_preference: () => null,
    set_external_payment_user_id: () => null,
    set_analytics_user_enabled: () => null,
    set_window_name: () => null,
    adview_mount: (args) => {
      const request = (args['request'] ?? {}) as Record<string, unknown>;
      const elementId = String(request['elementId']);
      const channel = args['onEvent'];
      if (!isChannel(channel)) rejectWith('invalid-argument', 'onEvent is not a channel');
      const guestLabel = `owad-${String(++guests)}`;
      live.set(elementId, { elementId, guestLabel, request, channel, next: 0 });
      return { guestLabel };
    },
    adview_update: () => null,
    adview_unmount: (args) => {
      live.delete(String(args['elementId']));
      return null;
    },
    adview_command: () => null,
    updater_check: () => null,
  };
  const commands: Record<string, MockCommand> = { ...options.commands };
  const prefix = `plugin:${PLUGIN}|`;
  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    if (!cmd.startsWith(prefix)) {
      throw new Error(`${cmd} not allowed. Command not found`);
    }
    const command = cmd.slice(prefix.length);
    calls.push({ command, args });
    const original =
      defaults[command] ?? (() => rejectWith('unsupported', `${command} is not mocked`));
    const implementation = commands[command];
    return implementation ? implementation(args, original) : original(args);
  });
  return {
    calls,
    callsOf: (command) => calls.filter((c) => c.command === command).map((c) => c.args),
    setCommand: (command, implementation) => {
      commands[command] = implementation;
    },
    mounts: () =>
      [...live.values()].map(({ elementId, guestLabel, request }) => ({
        elementId,
        guestLabel,
        request,
      })),
    emit: (elementId, name, data, source = 'guest') => {
      const mount = live.get(elementId);
      if (!mount) throw new Error(`no mounted <owadview> with element id ${elementId}`);
      const internals = Reflect.get(globalThis, '__TAURI_INTERNALS__') as Internals;
      const message = data === undefined ? { name, source } : { name, data, source };
      internals.runCallback(mount.channel.id, { message, index: mount.next++ });
    },
    restore: () => {
      clearMocks();
    },
  };
}

/**
 * Waits until pending promise callbacks and zero-delay timers have run, so
 * the effects of a DOM change or an IPC reply are visible.
 *
 * @param rounds - how many timer turns to wait (default 4)
 * @returns resolves after them
 */
export async function settle(rounds = 4): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    await new Promise<void>((resolve) => {
      setTimeout(resolve, 0);
    });
  }
}
