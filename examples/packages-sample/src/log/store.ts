/**
 * The log every page writes to: app info, each API call and its result, and
 * every ad event. A small external store (React reads it with
 * `useSyncExternalStore`): each change replaces the entry list, so a
 * snapshot never changes under a render.
 *
 * @packageDocumentation
 */

/** How an entry is coloured in the log view (the upstream sample's types). */
export type LogLevel = 'info' | 'warn' | 'success' | 'error' | 'result' | 'dim';

/** Where an entry comes from. */
export type LogSource = 'app' | 'api' | 'ad' | 'updater';

/** One log line, with optional values shown as expandable JSON. */
export interface LogEntry {
  /** Increasing id (React key). */
  readonly id: number;
  /** When it was logged, `Date.now()` milliseconds. */
  readonly at: number;
  /** How it is coloured. */
  readonly level: LogLevel;
  /** Who logged it. */
  readonly source: LogSource;
  /** The text. */
  readonly message: string;
  /** Values shown under the text. */
  readonly args: readonly unknown[];
}

/** The log store. */
export interface LogStore {
  /**
   * The entries, oldest first. The same array until the next change.
   *
   * @returns the current snapshot
   */
  entries: () => readonly LogEntry[];
  /**
   * Adds an entry; the oldest is dropped beyond the store's limit.
   *
   * @param level - colour
   * @param source - who logs it
   * @param message - the text
   * @param args - values shown under the text
   * @returns the new entry
   */
  push: (level: LogLevel, source: LogSource, message: string, ...args: unknown[]) => LogEntry;
  /** Removes every entry. */
  clear: () => void;
  /**
   * Calls `listener` after every change.
   *
   * @param listener - the callback
   * @returns a function that removes it
   */
  subscribe: (listener: () => void) => () => void;
}

/** Options of {@link createLogStore}. */
export interface LogStoreOptions {
  /** The most entries kept (default {@link MAX_ENTRIES}). */
  max?: number;
  /** The clock (default `Date.now`). */
  now?: () => number;
}

/** The default number of entries kept. */
export const MAX_ENTRIES = 1000;

/**
 * Creates an empty log store.
 *
 * @param options - limit and clock
 * @returns the store
 */
export function createLogStore(options: LogStoreOptions = {}): LogStore {
  const max = Math.max(1, options.max ?? MAX_ENTRIES);
  const now = options.now ?? Date.now;
  const listeners = new Set<() => void>();
  let list: readonly LogEntry[] = [];
  let next = 1;
  const changed = (): void => {
    for (const listener of [...listeners]) listener();
  };
  return {
    entries: () => list,
    push: (level, source, message, ...args) => {
      const entry: LogEntry = { id: next++, at: now(), level, source, message, args };
      list = list.length >= max ? [...list.slice(list.length - max + 1), entry] : [...list, entry];
      changed();
      return entry;
    },
    clear: () => {
      if (list.length === 0) return;
      list = [];
      changed();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}
