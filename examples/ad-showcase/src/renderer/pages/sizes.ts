/**
 * Page 1, Sizes: all seven documented containers with unique `cid`s, plus a
 * 300x250 below the fold of a scroll box (test ads load only in view).
 *
 * The 970x90 needs about 1000 px of width, so the page folds the timeline
 * rail while it is open (and unfolds it on leave if it folded it). In live
 * mode only one video-capable container may be on a page (ad policy), so the
 * 400x300 is left out there; page 2 is the live proof.
 *
 * @packageDocumentation
 */
import { h, note, pageHeader } from '../dom.js';
import { createSlot, type Slot } from '../slots.js';
import type { MountPage } from './page.js';

/** Grid area, size and cid of each slot. */
const SLOTS: readonly { area: string; size: [number, number]; cid: string }[] = [
  { area: 'w970', size: [970, 90], cid: 'sz-970x90' },
  { area: 'w728', size: [728, 90], cid: 'sz-728x90' },
  { area: 't160', size: [160, 600], cid: 'sz-160x600' },
  { area: 'v600', size: [400, 600], cid: 'sz-400x600' },
  { area: 'b60', size: [400, 60], cid: 'sz-400x60' },
  { area: 'm300', size: [400, 300], cid: 'sz-400x300' },
  { area: 'r250', size: [300, 250], cid: 'sz-300x250' },
];

export const mountSizes: MountPage = (root, ctx) => {
  const foldedRail = !ctx.railCollapsed();
  if (foldedRail) ctx.setRailCollapsed(true);
  const slots: Slot[] = [];
  const grid = h('div', { class: 'sizes-grid' });
  for (const spec of SLOTS) {
    if (ctx.mode === 'live' && spec.cid === 'sz-400x300') {
      grid.append(
        h(
          'div',
          { class: 'slot slot-omitted', style: `grid-area:${spec.area}` },
          note(
            'info',
            '400x300 not mounted in LIVE: at most one video container per page (Overwolf ad policy). Page 2 shows it live.',
          ),
        ),
      );
      continue;
    }
    const slot = createSlot(ctx, { size: spec.size, cid: spec.cid });
    slot.card.style.gridArea = spec.area;
    slots.push(slot);
    grid.append(slot.card);
  }

  const fold = createSlot(ctx, { size: [300, 250], cid: 'sz-fold-300x250' });
  slots.push(fold);
  const scroller = h(
    'div',
    {
      class: 'fold-scroller',
      attrs: { tabindex: '0', 'aria-label': 'Below-the-fold scroll box' },
      data: { action: 'sizes-fold-scroller' },
    },
    h('div', { class: 'fold-spacer' }, h('p', { class: 'muted', text: 'Scroll to load ↓' })),
    fold.card,
  );

  root.append(
    pageHeader(
      'Sizes',
      'The seven container sizes Overwolf documents, each with its own cid. Test ads fill all seven in view.',
    ),
    grid,
    h(
      'section',
      { class: 'fold-section' },
      h('h2', { class: 'label', text: 'Below the fold' }),
      h('p', {
        class: 'muted',
        text: 'This 300x250 starts out of view and waits; it loads once scrolled in.',
      }),
      scroller,
    ),
  );
  return () => {
    for (const slot of slots) slot.dispose();
    if (foldedRail) ctx.setRailCollapsed(false);
  };
};
