/**
 * Page 9, Parity: renders the parity harness's report (`parity-diff.json`,
 * copied to `<userData>/parity-report.json`) when there is one, else shows
 * how to make it.
 *
 * @packageDocumentation
 */
import type { ParityDiff, ParityLookup } from '../../shared/ipc.js';
import { shellPath } from '../../shared/paths.js';
import { button, h, note, pageHeader } from '../dom.js';
import type { MountPage } from './page.js';

function short(value: unknown): string {
  if (value === undefined) return '';
  const text = typeof value === 'string' ? value : JSON.stringify(value);
  return text.length > 140 ? `${text.slice(0, 137)}...` : text;
}

function tone(cls: string): string {
  if (cls === 'BUG') return 'err';
  if (cls.startsWith('intended')) return 'info';
  if (cls === 'variance') return 'muted';
  return 'warn';
}

function renderReport(body: HTMLElement, lookup: ParityLookup): void {
  const report = lookup.report;
  if (!report) {
    body.replaceChildren(
      note(
        lookup.error ? 'warn' : 'info',
        lookup.error
          ? `Could not read ${lookup.path}: ${lookup.error}`
          : `No parity report at ${lookup.path}.`,
      ),
      h('h2', { class: 'label', text: 'Make one' }),
      h(
        'pre',
        { class: 'mono payload' },
        [
          '# In the ow-tauri repository, macOS, test ads, invisible windows:',
          'cd tools/parity-harness',
          'node run.mjs --mode test --present transparent --window-monitor --run-id E1',
          'node run.mjs --host tauri --mode test --present transparent --window-monitor --run-id T1',
          'node parity-diff.mjs captures/E1 captures/T1',
          `cp captures/T1/parity-diff.json ${shellPath(lookup.path)}`,
        ].join('\n'),
      ),
      h('p', {
        class: 'muted',
        text: 'Each host reads its own data folder: ow-electron the userData folder named after the product name, ow-tauri the app data folder named after the bundle identifier. Copy the report to the path above for the host you run.',
      }),
    );
    return;
  }
  const diffs: ParityDiff[] = report.diffs ?? [];
  const counts = Object.entries(report.counts ?? {});
  body.replaceChildren(
    h(
      'div',
      { class: 'parity-summary' },
      h('span', {
        class: `chip chip-big tone-${(report.bugs ?? 0) === 0 ? 'ok' : 'err'}`,
        text: `${String(report.bugs ?? 0)} BUG`,
      }),
      h('span', {
        class: 'mono muted',
        text: `${report.electron?.runId ?? '?'} (ow-electron) vs ${report.tauri?.runId ?? '?'} (ow-tauri)`,
      }),
      ...counts.map(([k, n]) =>
        h('span', { class: `chip tone-${tone(k)}`, text: `${k} ${String(n)}` }),
      ),
    ),
    diffs.length === 0
      ? note('info', 'No observable differences.')
      : h(
          'div',
          { class: 'table-wrap' },
          h(
            'table',
            { class: 'parity-table' },
            h(
              'thead',
              {},
              h(
                'tr',
                {},
                ...['Section', 'Key', 'ow-electron', 'ow-tauri', 'Verdict'].map((t) =>
                  h('th', { text: t, attrs: { scope: 'col' } }),
                ),
              ),
            ),
            h(
              'tbody',
              {},
              ...diffs.map((d) =>
                h(
                  'tr',
                  {},
                  h('td', { text: d.section }),
                  h('td', { class: 'mono', text: `${short(d.key)} ${short(d.field)}`.trim() }),
                  h('td', { class: 'mono', text: short(d.electron) }),
                  h('td', { class: 'mono', text: short(d.tauri) }),
                  h(
                    'td',
                    { attrs: d.why ? { title: d.why } : {} },
                    h('span', { class: `chip tone-${tone(d.class)}`, text: d.class }),
                  ),
                ),
              ),
            ),
          ),
        ),
    h('p', { class: 'muted mono', text: `Source: ${lookup.path}` }),
  );
}

export const mountParity: MountPage = (root, ctx) => {
  const body = h('div', { class: 'parity-body' }, h('p', { class: 'muted', text: 'Loading…' }));
  let live = true;
  const load = (): void => {
    void ctx.api.parity().then((lookup) => {
      if (live) renderReport(body, lookup);
    });
  };
  root.append(
    pageHeader(
      'Parity',
      'The parity harness compares what ow-electron and ow-tauri send and do for the same scenario.',
    ),
    h('div', { class: 'toolbar' }, button('Reload report', 'parity-reload', load)),
    body,
  );
  load();
  return () => {
    live = false;
  };
};
