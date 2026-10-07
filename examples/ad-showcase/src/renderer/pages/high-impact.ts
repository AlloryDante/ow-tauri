/**
 * Page 3, High impact: the Tower Plus layout in a 440 px wide, full-height ad
 * zone, with `adstyle="high-impact-ad;"` on the 400x600 slot. On
 * `high-impact-ad-loaded` the slot grows to the whole zone and its sibling is
 * hidden with `display: none`; on `high-impact-ad-removed` everything is
 * restored. Hiding the sibling, instead of removing and re-appending it as
 * the docs suggest, keeps it alive: a detached `<owadview>` is dead in
 * ow-electron [OBS].
 *
 * @packageDocumentation
 */
import { h, note, pageHeader } from '../dom.js';
import { createSlot } from '../slots.js';
import type { MountPage } from './page.js';

type HiState = 'waiting' | 'takeover' | 'removed';

export const mountHighImpact: MountPage = (root, ctx) => {
  const zone = h('div', { class: 'hi-zone', attrs: { 'aria-label': 'Ad zone 440 wide' } });
  const small = createSlot(ctx, { size: [400, 60], cid: 'hi-400x60' });
  const big = createSlot(ctx, {
    size: [400, 600],
    cid: 'hi-400x600',
    kind: 'high-impact',
    adstyle: 'high-impact-ad;',
  });
  zone.append(small.card, big.card);

  const stateChip = h('span', { class: 'chip chip-big', attrs: { role: 'status' } });
  const setState = (state: HiState): void => {
    stateChip.textContent =
      state === 'waiting' ? 'Waiting' : state === 'takeover' ? 'Takeover' : 'Removed · restored';
    stateChip.className = `chip chip-big tone-${state === 'takeover' ? 'hi' : state === 'removed' ? 'ok' : 'info'}`;
    zone.dataset['state'] = state;
  };
  setState('waiting');
  ctx.inspect(() => {
    const zr = zone.getBoundingClientRect();
    const br = big.box.getBoundingClientRect();
    return {
      state: zone.dataset['state'] ?? '',
      zone: [zr.width, zr.height].map(Math.round),
      bigBox: [br.width, br.height].map(Math.round),
      smallDisplay: getComputedStyle(small.card).display,
    };
  });

  const head = big.card.querySelector<HTMLElement>('.slot-head');
  // Takeover: the slot fills the zone (the sample's handler); the sibling is
  // hidden with display: none, never removed.
  big.on('high-impact-ad-loaded', () => {
    small.card.style.display = 'none';
    if (head) head.hidden = true;
    for (const node of [big.card, big.box]) {
      node.style.width = '100%';
      node.style.height = '100%';
    }
    setState('takeover');
  });
  big.on('high-impact-ad-removed', () => {
    small.card.style.display = '';
    if (head) head.hidden = false;
    big.card.style.width = '400px';
    big.card.style.height = '';
    big.box.style.width = '400px';
    big.box.style.height = '600px';
    setState('removed');
  });

  root.append(
    h(
      'div',
      { class: 'hi-page' },
      zone,
      h(
        'div',
        { class: 'hi-side' },
        pageHeader(
          'High impact',
          'One 400x600 slot with adstyle "high-impact-ad;" in a 440 px ad zone at full window height (at least 670 px).',
        ),
        h('div', { class: 'state-row' }, h('span', { class: 'label', text: 'State' }), stateChip),
        h(
          'ol',
          { class: 'steps-list' },
          h('li', { text: 'Waiting: both slots load as normal ads.' }),
          h('li', {
            text: 'Takeover: on high-impact-ad-loaded the slot fills the zone; its sibling gets display: none.',
          }),
          h('li', {
            text: 'Removed: on high-impact-ad-removed (about 15 s later in test mode) the sibling comes back.',
          }),
        ),
        note(
          'info',
          'Live high-impact demand comes from direct deals once Overwolf qualifies the app; until then it shows in test mode only.',
        ),
      ),
    ),
  );
  return () => {
    small.dispose();
    big.dispose();
  };
};
