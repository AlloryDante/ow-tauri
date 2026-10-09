import { useState, type ReactElement } from 'react';

/** Entries shown in a collapsed object or array. */
const PREVIEW_MAX_ENTRIES = 4;
/** Characters shown of a string inside a collapsed preview. */
const PREVIEW_MAX_STR_LEN = 24;

/** Props of {@link JsonNode}. */
export interface JsonNodeProps {
  /** The value to show. */
  value: unknown;
}

/** A primitive as coloured text, or `null` for objects and arrays. */
function primitive(value: unknown, quote: string, maxLength?: number): ReactElement | null {
  if (value === null || value === undefined)
    return <span className="jv-null">{String(value)}</span>;
  if (typeof value === 'string') {
    const text =
      maxLength !== undefined && value.length > maxLength ? `${value.slice(0, maxLength)}…` : value;
    return (
      <span className="jv-string">
        {quote}
        {text}
        {quote}
      </span>
    );
  }
  if (typeof value === 'number' || typeof value === 'bigint')
    return <span className="jv-number">{String(value)}</span>;
  if (typeof value === 'boolean') return <span className="jv-boolean">{String(value)}</span>;
  // A function or a symbol (never in JSON).
  if (typeof value !== 'object') return <span className="jv-null">{typeof value}</span>;
  return null;
}

/** One value inside a collapsed preview. */
function PreviewValue({ value }: JsonNodeProps): ReactElement {
  return (
    primitive(value, "'", PREVIEW_MAX_STR_LEN) ?? (
      <span className="jv-brace">{Array.isArray(value) ? '[…]' : '{…}'}</span>
    )
  );
}

/**
 * A value as an expandable JSON tree (the upstream sample's log viewer):
 * objects and arrays start collapsed with a short preview.
 */
export function JsonNode({ value }: JsonNodeProps): ReactElement {
  const [expanded, setExpanded] = useState(false);
  const simple = primitive(value, '"');
  if (simple) return simple;

  const isArray = Array.isArray(value);
  const entries: [string, unknown][] = isArray
    ? (value as unknown[]).map((v, i) => [String(i), v])
    : Object.entries(value as Record<string, unknown>);
  const [open, close] = isArray ? ['[', ']'] : ['{', '}'];
  if (entries.length === 0)
    return (
      <span className="jv-brace">
        {open}
        {close}
      </span>
    );

  return (
    <span className="jv-node">
      <button
        type="button"
        className="jv-toggle"
        aria-expanded={expanded}
        aria-label={expanded ? 'Collapse' : 'Expand'}
        onClick={() => {
          setExpanded((e) => !e);
        }}
      >
        {expanded ? '▾' : '▸'}
      </button>
      <span className="jv-brace">{open}</span>
      {expanded ? (
        <>
          <span className="jv-block">
            {entries.map(([key, child]) => (
              <span key={key} className="jv-row">
                <span className="jv-key">{key}</span>
                <span className="jv-colon">: </span>
                <JsonNode value={child} />
                <span className="jv-comma">,</span>
              </span>
            ))}
          </span>
          <span className="jv-brace">{close}</span>
        </>
      ) : (
        <>
          {entries.slice(0, PREVIEW_MAX_ENTRIES).map(([key, child], i) => (
            <span key={key} className="jv-preview-entry">
              {i > 0 && <span className="jv-comma">, </span>}
              {!isArray && (
                <>
                  <span className="jv-key">{key}</span>
                  <span className="jv-colon">: </span>
                </>
              )}
              <PreviewValue value={child} />
            </span>
          ))}
          {entries.length > PREVIEW_MAX_ENTRIES && <span className="jv-preview-more"> …</span>}
          <span className="jv-brace">{close}</span>
        </>
      )}
    </span>
  );
}
