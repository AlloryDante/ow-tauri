/**
 * Paths as the window shows them. The window, its exports and its stills
 * never show the user's home folder: a path inside it is written with `~`
 * in its place (`~/Library/Application Support/<app>/exports`).
 *
 * @packageDocumentation
 */

/** Whether `path` looks like a Windows path (drive letter or backslashes). */
function isWindowsPath(path: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(path) || path.includes('\\');
}

/**
 * Writes a path inside the home folder with `~` in place of the home folder.
 * Other paths come back unchanged. Windows paths compare without regard to
 * case and with either separator.
 *
 * @param path - an absolute path
 * @param home - the home folder (`app.getPath('home')`)
 * @returns the display form
 *
 * @example
 * ```ts
 * homeRelative('/Users/me/Library/App/exports', '/Users/me'); // '~/Library/App/exports'
 * homeRelative('C:\\Users\\Me\\AppData\\App', 'c:\\users\\me'); // '~\\AppData\\App'
 * homeRelative('/opt/app/data', '/Users/me'); // '/opt/app/data'
 * ```
 */
export function homeRelative(path: string, home: string): string {
  const base = home.replace(/[\\/]+$/, '');
  if (base === '') return path;
  const windows = isWindowsPath(path) || isWindowsPath(base);
  const norm = (p: string): string => (windows ? p.replace(/\//g, '\\').toLowerCase() : p);
  const p = norm(path);
  const b = norm(base);
  if (p === b) return '~';
  const sep = windows ? '\\' : '/';
  if (!p.startsWith(b + sep)) return path;
  return `~${sep}${path.slice(base.length + 1)}`;
}

/**
 * The shell form of a display path for a copy-and-paste command: `~/...`
 * becomes `"$HOME/..."` (the tilde does not expand inside quotes); any other
 * path is quoted as it is.
 *
 * @param display - a path from {@link homeRelative}
 * @returns the quoted shell argument
 *
 * @example
 * ```ts
 * shellPath('~/Library/Application Support/App/parity-report.json');
 * // '"$HOME/Library/Application Support/App/parity-report.json"'
 * ```
 */
export function shellPath(display: string): string {
  if (display === '~') return '"$HOME"';
  if (display.startsWith('~/')) return `"$HOME/${display.slice(2)}"`;
  return `"${display}"`;
}

/** Escapes a string for use inside a regular expression. */
function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/**
 * Replaces every mention of the home folder in a text (a JSON document, a
 * report, an error message) with `~`: the plain path, its JSON-escaped form,
 * its forward-slash form and its URL-encoded form (`file:///Users/me/...`).
 * A longer folder name that only starts with the same letters is left alone.
 *
 * @param text - the text
 * @param home - the home folder (`app.getPath('home')`)
 * @returns the text without the home folder
 *
 * @example
 * ```ts
 * redactHome('{"url":"file:///Users/me/app/index.html"}', '/Users/me');
 * // '{"url":"file://~/app/index.html"}'
 * ```
 */
export function redactHome(text: string, home: string): string {
  const base = home.replace(/[\\/]+$/, '');
  if (base.length < 2) return text;
  const slash = base.replace(/\\/g, '/');
  const forms = [
    ...new Set([base, JSON.stringify(base).slice(1, -1), slash, encodeURI(slash)]),
  ].sort((a, b) => b.length - a.length);
  const flags = isWindowsPath(base) ? 'gi' : 'g';
  const pattern = new RegExp(`(?:${forms.map(escapeRegExp).join('|')})(?=[\\\\/"'\\s]|$)`, flags);
  return text.replace(pattern, '~');
}
