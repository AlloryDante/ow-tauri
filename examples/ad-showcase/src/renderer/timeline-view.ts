/**
 * The event timeline rail: newest row at the bottom with autoscroll, a pause
 * toggle, filters by element and by family, a payload view per row, counts
 * per event name and the JSON export.
 *
 * @packageDocumentation
 */
import { button, h } from './dom.js';
import { FAMILIES, type Family } from './events.js';
import {
  clock,
  matches,
  spreadString,
  type TimelineEntry,
  type TimelineFilter,
  type TimelineStore,
} from './timeline-store.js';

/** Rows kept in the DOM; older ones stay in the store and in exports. */
const MAX_ROWS = 1500;

/** What the rail needs from the app shell. */
export interface TimelineViewOptions {
  /** The store to render. */
  store: TimelineStore;
  /** Writes the export; resolves with the written path. */
  exportJson: () => Promise<string>;
  /** Called after the rail collapses or expands. */
  onToggle?: (collapsed: boolean) => void;
}

/** The rail element and its controls. */
export class TimelineView {
  /** The `<aside>` to place in the layout. */
  readonly element: HTMLElement;
  readonly #store: TimelineStore;
  readonly #list: HTMLElement;
  readonly #counts: HTMLElement;
  readonly #cidSelect: HTMLSelectElement;
  readonly #familySelect: HTMLSelectElement;
  readonly #pauseButton: HTMLButtonElement;
  readonly #exportStatus: HTMLElement;
  readonly #total: HTMLElement;
  readonly #options: TimelineViewOptions;
  #filter: TimelineFilter = { cid: null, family: null };
  #paused = false;
  #pending = 0;
  #collapsed = false;

  constructor(options: TimelineViewOptions) {
    this.#options = options;
    this.#store = options.store;
    this.#cidSelect = h('select', {
      class: 'select',
      attrs: { 'aria-label': 'Filter by element' },
      data: { action: 'timeline-filter-cid' },
    });
    this.#familySelect = h('select', {
      class: 'select',
      attrs: { 'aria-label': 'Filter by family' },
      data: { action: 'timeline-filter-family' },
    });
    this.#familySelect.append(h('option', { text: 'all families', attrs: { value: '' } }));
    for (const f of FAMILIES)
      this.#familySelect.append(h('option', { text: f, attrs: { value: f } }));
    this.#cidSelect.addEventListener('change', () => {
      this.#filter = { ...this.#filter, cid: this.#cidSelect.value || null };
      this.#rerender();
    });
    this.#familySelect.addEventListener('change', () => {
      this.#filter = {
        ...this.#filter,
        family: (this.#familySelect.value || null) as Family | null,
      };
      this.#rerender();
    });
    this.#pauseButton = button('Pause', 'timeline-pause', () => {
      this.#setPaused(!this.#paused);
    });
    this.#pauseButton.setAttribute('aria-pressed', 'false');
    const exportButton = button('Export JSON', 'timeline-export', () => {
      void this.#export();
    });
    this.#exportStatus = h('p', { class: 'tl-export mono', attrs: { 'aria-live': 'polite' } });
    this.#list = h('div', {
      class: 'tl-list',
      attrs: { role: 'log', 'aria-label': 'Ad events', tabindex: '0' },
    });
    this.#counts = h('div', {
      class: 'tl-counts mono',
      attrs: { 'aria-label': 'Counts per event' },
    });
    this.#total = h('span', { class: 'tl-total mono', text: '0' });
    const collapse = button(
      'Hide',
      'timeline-collapse',
      () => {
        this.setCollapsed(!this.#collapsed);
      },
      'btn-ghost tl-collapse',
    );
    collapse.setAttribute('aria-expanded', 'true');
    this.element = h(
      'aside',
      { class: 'rail', attrs: { 'aria-label': 'Event timeline' } },
      h(
        'div',
        { class: 'rail-head' },
        h('h2', { class: 'label', text: 'Event timeline' }),
        this.#total,
        collapse,
      ),
      h(
        'div',
        { class: 'rail-tools' },
        this.#cidSelect,
        this.#familySelect,
        this.#pauseButton,
        exportButton,
      ),
      this.#exportStatus,
      this.#list,
      h('div', { class: 'rail-foot' }, h('h3', { class: 'label', text: 'Counts' }), this.#counts),
    );
    this.#store.subscribe((entry) => {
      this.#onEntry(entry);
    });
    this.#refreshCids();
    this.#renderCounts();
  }

  /**
   * Collapses or expands the rail.
   *
   * @param collapsed - `true` to collapse
   */
  setCollapsed(collapsed: boolean): void {
    this.#collapsed = collapsed;
    this.element.classList.toggle('collapsed', collapsed);
    const toggle = this.element.querySelector<HTMLButtonElement>('.tl-collapse');
    if (toggle) {
      toggle.textContent = collapsed ? 'Show' : 'Hide';
      toggle.setAttribute('aria-expanded', String(!collapsed));
    }
    this.#options.onToggle?.(collapsed);
  }

  /** Whether the rail is collapsed. */
  get collapsed(): boolean {
    return this.#collapsed;
  }

  #setPaused(paused: boolean): void {
    this.#paused = paused;
    this.#pauseButton.setAttribute('aria-pressed', String(paused));
    if (!paused) {
      this.#pending = 0;
      this.#rerender();
    }
    this.#pauseButton.textContent = paused ? 'Resume' : 'Pause';
  }

  async #export(): Promise<void> {
    this.#exportStatus.textContent = 'Writing…';
    try {
      const path = await this.#options.exportJson();
      this.#exportStatus.textContent = `Saved ${path}`;
    } catch (error) {
      this.#exportStatus.textContent = `Export failed: ${String(error)}`;
    }
  }

  #onEntry(entry: TimelineEntry): void {
    this.#total.textContent = String(this.#store.entries.length);
    if (!this.#cidSelect.querySelector(`option[value="${CSS.escape(entry.cid)}"]`)) {
      this.#refreshCids();
    }
    this.#renderCounts();
    if (!matches(entry, this.#filter)) return;
    if (this.#paused) {
      this.#pending += 1;
      this.#pauseButton.textContent = `Resume (${String(this.#pending)} new)`;
      return;
    }
    const atBottom = this.#list.scrollHeight - this.#list.scrollTop - this.#list.clientHeight < 24;
    this.#list.append(row(entry));
    while (this.#list.childElementCount > MAX_ROWS) this.#list.firstElementChild?.remove();
    if (atBottom) this.#list.scrollTop = this.#list.scrollHeight;
  }

  #rerender(): void {
    const shown = this.#store.entries.filter((e) => matches(e, this.#filter)).slice(-MAX_ROWS);
    this.#list.replaceChildren(...shown.map(row));
    this.#list.scrollTop = this.#list.scrollHeight;
  }

  #refreshCids(): void {
    const current = this.#cidSelect.value;
    this.#cidSelect.replaceChildren(
      h('option', { text: 'all elements', attrs: { value: '' } }),
      ...this.#store.cids().map((cid) => h('option', { text: cid, attrs: { value: cid } })),
    );
    this.#cidSelect.value = current;
  }

  #renderCounts(): void {
    const counts = this.#store.counts();
    this.#counts.replaceChildren(
      ...(counts.length === 0
        ? [h('span', { class: 'muted', text: 'No events yet.' })]
        : counts.map(([name, n]) =>
            h('span', { class: 'count' }, h('span', { text: name }), h('b', { text: String(n) })),
          )),
    );
  }
}

function row(entry: TimelineEntry): HTMLElement {
  const details = h('pre', { class: 'tl-payload mono' });
  details.hidden = true;
  const head = h(
    'button',
    {
      class: 'tl-row',
      attrs: { type: 'button', 'aria-expanded': 'false' },
      data: { family: entry.family, name: entry.name, cid: entry.cid },
      onClick: () => {
        const open = details.hidden;
        if (open && details.textContent === '') details.textContent = describe(entry);
        details.hidden = !open;
        head.setAttribute('aria-expanded', String(open));
      },
    },
    h('span', { class: 'tl-time mono', text: clock(entry.t) }),
    h('span', { class: `dot fam-${entry.family}`, attrs: { 'aria-hidden': 'true' } }),
    h('span', { class: 'tl-name', text: entry.name }),
    h('span', { class: 'tl-cid mono', text: entry.cid }),
  );
  return h('div', { class: 'tl-item' }, head, details);
}

function describe(entry: TimelineEntry): string {
  const lines = [
    `family ${entry.family}`,
    entry.sinceMount === null ? null : `+${String(Math.round(entry.sinceMount))} ms since created`,
  ].filter((l): l is string => l !== null);
  const spread = spreadString(entry.payload);
  if (spread !== null) lines.push(`payload: a string spread per character:\n"${spread}"`);
  else lines.push(JSON.stringify(entry.payload, null, 2));
  return lines.join('\n');
}
