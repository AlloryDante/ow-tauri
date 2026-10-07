/**
 * Page 1, Sizes: the seven documented containers with unique `cid`s, shown
 * as groups that each fit the window completely, plus a 300x250 below the
 * fold of a scroll box (test ads load only in view), and each size alone.
 *
 * All seven at once do not fit a 1280x860 window with a header row per slot
 * (they need about 1040x770 of the content area), and a slot that is not
 * fully in view waits instead of loading, so the page shows one group at a
 * time; switching removes the old containers and creates new ones. The
 * group or size is the page's route argument (`#sizes/banners`), so a
 * restart comes back to it.
 *
 * The groups need the width of the timeline rail, so the page folds the
 * rail while it is open (and unfolds it on leave if it folded it). In live
 * mode only one video-capable container may be on a page (ad policy), so the
 * groups leave the 400x300 out there; it can still be shown alone.
 *
 * @packageDocumentation
 */
import { h, note, pageHeader } from '../dom.js';
import { createSlot, type Slot } from '../slots.js';
import type { MountPage } from './page.js';

/** One container: grid area, size and `cid`. */
interface SizeSpec {
  area: string;
  size: [number, number];
  cid: string;
}

/** The seven sizes. */
export const SIZES: readonly SizeSpec[] = [
  { area: 't160', size: [160, 600], cid: 'sz-160x600' },
  { area: 'v600', size: [400, 600], cid: 'sz-400x600' },
  { area: 'm300', size: [400, 300], cid: 'sz-400x300' },
  { area: 'r250', size: [300, 250], cid: 'sz-300x250' },
  { area: 'w970', size: [970, 90], cid: 'sz-970x90' },
  { area: 'w728', size: [728, 90], cid: 'sz-728x90' },
  { area: 'b60', size: [400, 60], cid: 'sz-400x60' },
];

/** A choice of the "Show" select: a group, the fold demo or one size. */
interface View {
  id: string;
  label: string;
  /** Sizes shown, as `WxH`. */
  sizes: readonly string[];
  /** Grid class. */
  grid: 'group-towers' | 'group-banners' | 'group-single' | 'fold';
}

const sizeText = (s: SizeSpec): string => `${String(s.size[0])}x${String(s.size[1])}`;

/** The default view: the four tall and square containers. */
const TOWERS: View = {
  id: 'towers',
  label: 'Towers and rectangles',
  sizes: ['160x600', '400x600', '400x300', '300x250'],
  grid: 'group-towers',
};

/** The views of page 1, in select order; the first is the default. */
export const VIEWS: readonly View[] = [
  TOWERS,
  {
    id: 'banners',
    label: 'Banners',
    sizes: ['970x90', '728x90', '400x60'],
    grid: 'group-banners',
  },
  { id: 'fold', label: 'Below the fold', sizes: [], grid: 'fold' },
  ...SIZES.map((s) => ({
    id: sizeText(s),
    label: `${sizeText(s)} alone`,
    sizes: [sizeText(s)],
    grid: 'group-single' as const,
  })),
];

let visit = 0;

export const mountSizes: MountPage = (root, ctx) => {
  const foldedRail = !ctx.railCollapsed();
  if (foldedRail) ctx.setRailCollapsed(true);
  let slots: Slot[] = [];
  let view: View = VIEWS.find((v) => v.id === ctx.arg) ?? TOWERS;
  const stage = h('div', { class: 'sizes-stage' });

  const select = h('select', {
    class: 'select',
    attrs: { id: 'sizes-select', 'aria-label': 'Show' },
    data: { action: 'sizes-select' },
  });
  for (const v of VIEWS) select.append(h('option', { text: v.label, attrs: { value: v.id } }));
  select.value = view.id;

  const render = (): void => {
    for (const slot of slots) slot.dispose();
    slots = [];
    stage.replaceChildren();
    visit += 1;
    ctx.control('sizes', 'app', { show: view.id });
    // Unique per creation, like page 2: `sz<visit>-<size>`.
    const cidOf = (base: string): string =>
      base.replace(/^sz-/, `sz${String(visit % 100)}-`).slice(0, 20);
    if (view.grid === 'fold') {
      const fold = createSlot(ctx, { size: [300, 250], cid: cidOf('sz-fold-300x250') });
      slots.push(fold);
      stage.append(
        h(
          'section',
          { class: 'fold-section' },
          h('p', {
            class: 'muted',
            text: 'This 300x250 starts out of view and waits; it loads once scrolled in.',
          }),
          h(
            'div',
            {
              class: 'fold-scroller',
              attrs: { tabindex: '0', 'aria-label': 'Below-the-fold scroll box' },
              data: { action: 'sizes-fold-scroller' },
            },
            h(
              'div',
              { class: 'fold-spacer' },
              h('p', { class: 'muted', text: 'Scroll to load ↓' }),
            ),
            fold.card,
          ),
        ),
      );
      return;
    }
    const grid = h('div', { class: `sizes-grid ${view.grid}` });
    const single = view.grid === 'group-single';
    // Only the towers grid names its areas; an area name the grid does not
    // define adds implicit tracks after the explicit ones, which put all
    // three banners on one cell to the right of the window (out of view).
    const named = view.grid === 'group-towers';
    for (const spec of SIZES) {
      const text = sizeText(spec);
      if (!view.sizes.includes(text)) continue;
      if (!single && ctx.mode === 'live' && spec.cid === 'sz-400x300') {
        grid.append(
          h(
            'div',
            { class: 'slot slot-omitted', style: `grid-area:${spec.area}` },
            note(
              'info',
              '400x300 not mounted in LIVE: at most one video container per page (Overwolf ad policy). Show it alone, or use page 2.',
            ),
          ),
        );
        continue;
      }
      const slot = createSlot(ctx, { size: spec.size, cid: cidOf(spec.cid) });
      if (named) slot.card.style.gridArea = spec.area;
      slots.push(slot);
      grid.append(slot.card);
    }
    stage.append(grid);
  };

  select.addEventListener('change', () => {
    view = VIEWS.find((v) => v.id === select.value) ?? view;
    ctx.setArg(view.id);
    render();
  });

  ctx.inspect(() => ({ show: view.id }));
  root.append(
    pageHeader(
      'Sizes',
      'The seven container sizes Overwolf documents, each with its own cid. A slot loads once it is fully in view.',
      h(
        'div',
        { class: 'toolbar' },
        h('label', { class: 'label', text: 'Show', attrs: { for: 'sizes-select' } }),
        select,
      ),
    ),
    stage,
  );
  render();
  return () => {
    for (const slot of slots) slot.dispose();
    if (foldedRail) ctx.setRailCollapsed(false);
  };
};
