/**
 * Page 6, House: one 400x300 slot and a log of its `house_ad_action` /
 * `house-ad-action` events (dispatched in both spellings). House ads are set
 * up in Overwolf's Dev Console and appear when no paid ad fills.
 *
 * @packageDocumentation
 */
import { h, note, pageHeader } from '../dom.js';
import { createSlot } from '../slots.js';
import { clock } from '../timeline-store.js';
import type { MountPage } from './page.js';

export const mountHouse: MountPage = (root, ctx) => {
  const slot = createSlot(ctx, { size: [400, 300], cid: 'house-400x300', kind: 'house' });
  const log = h('ol', { class: 'house-log mono', attrs: { 'aria-live': 'polite' } });
  const empty = h('p', { class: 'muted', text: 'No house ad action yet.' });
  for (const name of ['house_ad_action', 'house-ad-action']) {
    slot.on(name, (event) => {
      empty.hidden = true;
      const action = (event as Event & { action?: unknown }).action;
      log.append(
        h('li', {
          text: `${clock(ctx.now())}  ${name}  action=${JSON.stringify(action ?? null)}`,
        }),
      );
    });
  }
  root.append(
    pageHeader(
      'House ads',
      'Your own promotions, served by Overwolf when no paid ad fills the slot.',
    ),
    h(
      'div',
      { class: 'two-col' },
      slot.card,
      h(
        'section',
        { class: 'card', attrs: { 'aria-label': 'House ad actions' } },
        h('h2', { class: 'label', text: 'house_ad_action / house-ad-action' }),
        empty,
        log,
      ),
    ),
    note(
      'info',
      'House ads appear on no-fill once they are set up in the Overwolf Dev Console for this app uid (images, an optional link, an optional event name). The action events fire only when an event name is set. Test mode serves none unless one is set up.',
    ),
  );
  return () => {
    slot.dispose();
  };
};
