import { useEffect, useMemo, useRef, useState, type ReactElement } from 'react';

import { useLog, useLogEntries } from './context';
import { formatTime, matches } from './format';
import { JsonNode } from './JsonNode';
import type { LogEntry, LogSource } from './store';

/** Props of {@link LogView}. */
export interface LogViewProps {
  /** Only entries of these sources (default: all). */
  sources?: readonly LogSource[];
  /** Show the search box (default `true`). */
  search?: boolean;
  /** Accessible name of the log. */
  label: string;
}

/** One log line with its values. */
function Entry({ entry }: { entry: LogEntry }): ReactElement {
  return (
    <div className={`log-entry ${entry.level}`} data-source={entry.source}>
      <span className="log-time">{formatTime(entry.at)}</span> <span>{entry.message}</span>
      {entry.args.map((arg, i) => (
        <div key={i} className="log-entry-arg">
          <JsonNode value={arg} />
        </div>
      ))}
    </div>
  );
}

/**
 * The upstream sample's log view: search, auto scroll and clear over the
 * shared log store.
 */
export function LogView({ sources, search = true, label }: LogViewProps): ReactElement {
  const log = useLog();
  const entries = useLogEntries();
  const [autoScroll, setAutoScroll] = useState(true);
  const [query, setQuery] = useState('');
  const terminal = useRef<HTMLDivElement>(null);

  const shown = useMemo(
    () =>
      entries.filter(
        (entry) => (!sources || sources.includes(entry.source)) && matches(entry, query),
      ),
    [entries, sources, query],
  );

  useEffect(() => {
    const el = terminal.current;
    if (autoScroll && !query && el) el.scrollTop = el.scrollHeight;
  }, [shown, autoScroll, query]);

  return (
    <div className="log-panel">
      {search && (
        <div className="log-toolbar">
          <input
            className="log-search"
            type="search"
            placeholder="Search logs…"
            aria-label="Search logs"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
            }}
          />
          {query && (
            <span className="log-search-count">
              {shown.length} / {entries.length}
            </span>
          )}
        </div>
      )}
      <div className="log-terminal" ref={terminal} role="log" aria-label={label}>
        {shown.map((entry) => (
          <Entry key={entry.id} entry={entry} />
        ))}
      </div>
      <div className="page-actions">
        <label className="check">
          <input
            type="checkbox"
            checked={autoScroll}
            onChange={(e) => {
              setAutoScroll(e.target.checked);
            }}
          />
          Auto scroll
        </label>
        <button
          type="button"
          className="btn-secondary"
          onClick={() => {
            log.clear();
          }}
        >
          Clear
        </button>
      </div>
    </div>
  );
}
