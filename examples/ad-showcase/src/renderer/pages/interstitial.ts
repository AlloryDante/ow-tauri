/**
 * Page 4, Interstitial: the performance `<owadview>` appended to
 * `document.body`, as the official sample does, with the documented
 * `adstyle` variants and an optional `unit`.
 *
 * - The "Click me" counter shows input passing through the overlay while the
 *   ad loads, and the overlay turning modal after its first
 *   `display_ad_loaded`.
 * - The DOM panel shows the live `owadview` count and the performance
 *   element's `pointer-events`, so the host removing the element after
 *   `shutdown` (and removing a second one at once) is visible.
 * - "Shrink to 900x500" takes the window below the ad's minimum for the
 *   `performance_ad_error` path.
 *
 * @packageDocumentation
 */
import { button, h, note, pageHeader } from '../dom.js';
import { listenAll, watchRemoval } from '../slots.js';
import type { MountPage } from './page.js';

/** The interstitial variants: default, a red dim and a blur. */
const VARIANTS: readonly { id: string; label: string; adstyle?: string }[] = [
  { id: 'default', label: 'Default' },
  {
    id: 'red-dim',
    label: 'Red dim',
    adstyle: 'background-color: rgba(255, 0, 0, 0.5); background-blur: -1;',
  },
  { id: 'blur-3', label: 'Blur 3', adstyle: 'background-blur: 3;' },
];

let created = 0;

export const mountInterstitial: MountPage = (root, ctx) => {
  const ours = new Set<HTMLElement>();
  const stops: (() => void)[] = [];
  let shrunk = false;

  const open = (variant: string, adstyle?: string, unit?: string): void => {
    created += 1;
    const label = `perf-${String(created)}`;
    const el = document.createElement('owadview');
    el.setAttribute('performance', '');
    if (adstyle) el.setAttribute('adstyle', adstyle);
    if (unit) el.setAttribute('unit', unit);
    const at = ctx.now();
    listenAll(ctx, el, label, 'performance', at);
    ctx.control('interstitial', label, {
      variant,
      adstyle: adstyle ?? null,
      unit: unit ?? null,
      alreadyUp: document.querySelector('owadview[performance]') !== null,
    });
    ours.add(el);
    document.body.appendChild(el);
    stops.push(watchRemoval(ctx, el, label, at));
  };

  const unitInput = h('input', {
    class: 'input mono',
    attrs: {
      type: 'text',
      id: 'perf-unit',
      placeholder: 'unit (optional)',
      'aria-label': 'Ad unit',
      spellcheck: 'false',
    },
    data: { action: 'interstitial-unit' },
  });

  let clicks = 0;
  const counter = h('span', { class: 'counter-value mono', text: '0' });
  const target = h(
    'button',
    {
      class: 'click-target',
      attrs: { type: 'button' },
      data: { action: 'interstitial-click-me' },
      onClick: () => {
        clicks += 1;
        counter.textContent = String(clicks);
      },
    },
    h('span', { text: 'Click me' }),
    counter,
  );

  const domCount = h('dd', { class: 'mono' });
  const domPerf = h('dd', { class: 'mono' });
  const domInline = h('dd', { class: 'mono' });
  const domComputed = h('dd', { class: 'mono' });
  const domDiv = h('dd', { class: 'mono' });
  const refresh = (): void => {
    domCount.textContent = String(document.querySelectorAll('owadview').length);
    const perf = document.querySelector<HTMLElement>('owadview[performance]');
    domPerf.textContent = perf ? 'in the document' : 'none';
    domInline.textContent = perf ? (perf.getAttribute('style') ?? '(no style attribute)') : '–';
    domComputed.textContent = perf ? getComputedStyle(perf).pointerEvents : '–';
    const div = perf?.querySelector(':scope > div');
    domDiv.textContent = div ? getComputedStyle(div).pointerEvents : '–';
  };
  refresh();
  const timer = setInterval(refresh, 200);

  root.append(
    pageHeader(
      'Interstitial',
      'A performance owadview on document.body. Input passes through while it loads; after its first display_ad_loaded it is modal until dismissed.',
    ),
    h(
      'div',
      { class: 'toolbar' },
      ...VARIANTS.map((v) =>
        button(v.label, `interstitial-${v.id}`, () => {
          open(v.id, v.adstyle);
        }),
      ),
      unitInput,
      button('With unit', 'interstitial-unit-open', () => {
        open('unit', undefined, unitInput.value.trim() || undefined);
      }),
    ),
    h(
      'div',
      { class: 'perf-page' },
      h('div', { class: 'click-zone' }, target),
      h(
        'aside',
        { class: 'card dom-panel', attrs: { 'aria-label': 'DOM panel' } },
        h('h2', { class: 'label', text: 'DOM' }),
        h(
          'dl',
          { class: 'kv' },
          h('dt', { text: 'owadview elements' }),
          domCount,
          h('dt', { text: 'performance element' }),
          domPerf,
          h('dt', { text: 'its style attribute' }),
          domInline,
          h('dt', { text: 'computed pointer-events' }),
          domComputed,
          h('dt', { text: 'overlay div pointer-events' }),
          domDiv,
        ),
        h(
          'div',
          { class: 'toolbar' },
          button('Shrink to 900x500', 'interstitial-shrink', () => {
            shrunk = true;
            ctx.control('window', 'app', { action: 'shrink-900x500' });
            void ctx.api.windowAction('shrink-900x500');
          }),
          button('Restore size', 'interstitial-restore-size', () => {
            shrunk = false;
            ctx.control('window', 'app', { action: 'restore-size' });
            void ctx.api.windowAction('restore-size');
          }),
        ),
      ),
    ),
    note(
      'info',
      'One interstitial per window: a second press while one is up is removed by the host at once (watch the count). No fill and errors end with shutdown, after which the host removes the element. Overwolf asks for a window of at least 1000x600 and never during gameplay.',
    ),
  );
  return () => {
    clearInterval(timer);
    for (const stop of stops) stop();
    for (const el of ours) el.remove();
    if (shrunk) void ctx.api.windowAction('restore-size');
  };
};
