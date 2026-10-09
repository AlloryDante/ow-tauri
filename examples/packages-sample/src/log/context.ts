/**
 * React access to the log store: the provider value and the hooks that read
 * it.
 *
 * @packageDocumentation
 */
import { createContext, useContext, useSyncExternalStore } from 'react';

import type { LogEntry, LogStore } from './store';

/** The app's log store (set by `<LogContext value={store}>` in `main.tsx`). */
export const LogContext = createContext<LogStore | null>(null);

/**
 * The log store of the nearest provider.
 *
 * @returns the store
 */
export function useLog(): LogStore {
  const store = useContext(LogContext);
  if (!store) throw new Error('useLog() needs a <LogContext> provider');
  return store;
}

/**
 * The log entries; the component renders again on every change.
 *
 * @returns the current entries, oldest first
 */
export function useLogEntries(): readonly LogEntry[] {
  const store = useLog();
  return useSyncExternalStore(store.subscribe, store.entries);
}
