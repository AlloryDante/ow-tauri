/**
 * Page 2, Layouts: Overwolf's eight recommended layouts at true size. Each
 * switch removes the old containers and creates new ones (never recycled).
 * Every layout has at most one video-capable container, so this is the page
 * for live mode.
 *
 * @packageDocumentation
 */
import { button, h, pageHeader } from '../dom.js';
import { createSlot, type Slot } from '../slots.js';
import type { MountPage } from './page.js';

/** A recommended layout: its containers, shortest first. */
export interface LayoutDef {
  /** Stable id. */
  id: string;
  /** Overwolf's name. */
  name: string;
  /** Container sizes. */
  sizes: readonly (readonly [number, number])[];
  /** Shown in a framed pop-up card (PopUp Studio Plus). */
  popup?: boolean;
}

/** The eight recommended layouts (Overwolf ads documentation). */
export const LAYOUTS: readonly LayoutDef[] = [
  {
    id: 'combo-classic',
    name: 'Combo Classic',
    sizes: [
      [300, 250],
      [400, 600],
    ],
  },
  {
    id: 'tall-duo',
    name: 'Tall Duo',
    sizes: [
      [160, 600],
      [400, 600],
    ],
  },
  {
    id: 'tower-plus',
    name: 'Tower Plus',
    sizes: [
      [400, 60],
      [400, 600],
    ],
  },
  {
    id: 'studio-tower',
    name: 'Studio Tower',
    sizes: [
      [400, 300],
      [160, 600],
    ],
  },
  {
    id: 'tower',
    name: 'Tower',
    sizes: [
      [728, 90],
      [400, 600],
    ],
  },
  {
    id: 'studio',
    name: 'Studio',
    sizes: [
      [728, 90],
      [400, 300],
    ],
  },
  {
    id: 'studio-plus',
    name: 'Studio Plus',
    sizes: [
      [400, 60],
      [400, 300],
    ],
  },
  {
    id: 'popup-studio-plus',
    name: 'PopUp Studio Plus',
    sizes: [
      [400, 60],
      [400, 300],
    ],
    popup: true,
  },
];

let visit = 0;

export const mountLayouts: MountPage = (root, ctx) => {
  const stage = h('div', { class: 'layout-stage' });
  let slots: Slot[] = [];
  const select = h('select', {
    class: 'select',
    attrs: { id: 'layout-select', 'aria-label': 'Layout' },
    data: { action: 'layout-select' },
  });
  for (const l of LAYOUTS) select.append(h('option', { text: l.name, attrs: { value: l.id } }));

  const render = (id: string): void => {
    for (const slot of slots) slot.dispose();
    slots = [];
    stage.replaceChildren();
    const layout = LAYOUTS.find((l) => l.id === id) ?? LAYOUTS[0];
    if (!layout) return;
    visit += 1;
    ctx.control('layout', 'app', { layout: layout.id });
    const zone = h('div', { class: layout.popup ? 'layout-zone popup' : 'layout-zone' });
    if (layout.popup) {
      zone.append(h('div', { class: 'popup-bar label', text: 'Pop-up window' }));
    }
    layout.sizes.forEach(([w, hgt], i) => {
      // Unique per creation: a cid may repeat only across windows.
      const cid = `ly${String(visit % 100)}-${String(i)}-${String(w)}x${String(hgt)}`.slice(0, 20);
      const slot = createSlot(ctx, { size: [w, hgt], cid });
      slots.push(slot);
      zone.append(slot.card);
    });
    stage.append(zone);
  };
  select.addEventListener('change', () => {
    render(select.value);
  });

  root.append(
    pageHeader(
      'Layouts',
      'The eight recommended layouts at true size. Switching removes the old containers and creates new ones.',
    ),
    h(
      'div',
      { class: 'toolbar' },
      h('label', { class: 'label', text: 'Layout', attrs: { for: 'layout-select' } }),
      select,
      button('Recreate', 'layout-recreate', () => {
        render(select.value);
      }),
    ),
    stage,
  );
  render(select.value);
  return () => {
    for (const slot of slots) slot.dispose();
  };
};
