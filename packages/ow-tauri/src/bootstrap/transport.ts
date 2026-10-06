/**
 * The runtime's only dependency on Tauri: invoking commands and creating the
 * host-message channel. Tests and the IPC conformance suite replace it.
 *
 * @packageDocumentation
 */
import { Channel, invoke } from '@tauri-apps/api/core';

import type { TauriInternals } from '../shared/protocol.js';

/** How the runtime talks to the host. */
export interface Transport {
  /**
   * Invokes a Tauri command.
   *
   * @param command - the full command, e.g. `plugin:overwolf|ipc_send`
   * @param args - the command arguments
   * @returns the command's response
   */
  invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
  /**
   * Creates the object passed as `onMessage` to `ipc_subscribe`.
   *
   * @param onBatch - called with each `HostMessage[]` batch, in order
   * @returns a value Tauri serialises as a channel reference
   */
  channel(onBatch: (batch: unknown) => void): unknown;
  /**
   * The calling webview's label, or `undefined` outside Tauri.
   *
   * @returns the label
   */
  label(): string | undefined;
  /**
   * Whether the Tauri IPC is present in this document.
   *
   * @returns `true` when commands can be invoked
   */
  available(): boolean;
}

function internals(): TauriInternals | undefined {
  return (globalThis as { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__;
}

/**
 * The transport backed by `@tauri-apps/api/core`. It reads
 * `window.__TAURI_INTERNALS__` at call time, so `mockIPC()` installed after
 * the runtime still takes effect.
 */
export const tauriTransport: Transport = {
  invoke: (command, args) => invoke(command, args),
  channel: (onBatch) => {
    const channel = new Channel<unknown>();
    channel.onmessage = onBatch;
    return channel;
  },
  label: () => {
    const metadata = internals()?.metadata;
    return metadata?.currentWebview?.label ?? metadata?.currentWindow?.label;
  },
  available: () => typeof internals()?.invoke === 'function',
};
