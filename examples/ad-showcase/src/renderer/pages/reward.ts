/**
 * Page 5, Reward: a small in-game shop. The 400x300 `rewarded-ad;` slot
 * preloads in view, is hidden once `video_ad_ready` arrives and is shown
 * again when the player presses "Watch ad"; the reward is granted on
 * `complete` after a `play` (see `reward.ts`). A second, 300x250 reward slot
 * is too small for the reward player and reports "unavailable" after 10 s
 * without `video_ad_ready`.
 *
 * @packageDocumentation
 */
import { button, h, note, pageHeader } from '../dom.js';
import { REWARD_STEPS, RewardFlow, UNAVAILABLE_AFTER_S, type RewardEffect } from '../reward.js';
import { createSlot } from '../slots.js';
import type { MountPage } from './page.js';

/** Coins per completed ad. */
const REWARD = 100;

function readCoins(key: string): number {
  try {
    const n = Number(localStorage.getItem(key));
    return Number.isFinite(n) && n > 0 ? n : 0;
  } catch {
    return 0;
  }
}

function writeCoins(key: string, coins: number): void {
  try {
    localStorage.setItem(key, String(coins));
  } catch {
    // Storage unavailable: the balance lasts for this session only.
  }
}

export const mountReward: MountPage = (root, ctx) => {
  const coinKey = `ad-showcase.coins.${ctx.info.host}`;
  let coins = readCoins(coinKey);
  const flow = new RewardFlow();

  const slot = createSlot(ctx, {
    size: [400, 300],
    cid: 'rw-400x300',
    kind: 'reward',
    adstyle: 'rewarded-ad;',
  });
  const small = createSlot(ctx, {
    size: [300, 250],
    cid: 'rw-300x250',
    kind: 'reward',
    adstyle: 'rewarded-ad;',
  });

  const balance = h('span', { class: 'coins mono', text: String(coins) });
  const watch = button(
    `Watch ad · +${String(REWARD)} coins`,
    'reward-watch',
    () => {
      apply(flow.watch());
      ctx.control('reward-watch', slot.cid);
      render();
    },
    'btn-primary',
  );
  const hideDuringPlay = button('Hide 2 s during play', 'reward-hide-2s', () => {
    ctx.control('reward-hide-2s', slot.cid);
    slot.card.style.display = 'none';
    setTimeout(() => {
      slot.card.style.display = '';
    }, 2000);
  });
  const placeholder = h(
    'div',
    { class: 'slot-placeholder', style: 'width:400px;height:324px' },
    h('span', { class: 'muted', text: 'Reward slot hidden (display: none) until "Watch ad"' }),
  );
  placeholder.hidden = true;
  const setSlotShown = (shown: boolean): void => {
    slot.card.style.display = shown ? '' : 'none';
    placeholder.hidden = shown;
  };
  const stepper = h('ol', { class: 'stepper', attrs: { 'aria-label': 'Reward progress' } });
  const status = h('p', { class: 'muted', attrs: { 'aria-live': 'polite' } });

  const apply = (effect: RewardEffect): void => {
    if (effect.grant) {
      coins += REWARD;
      writeCoins(coinKey, coins);
      ctx.control('reward-granted', slot.cid, { coins, grants: flow.grants });
    }
    if (effect.hide) setSlotShown(false);
    if (effect.show) setSlotShown(true);
  };
  const render = (): void => {
    balance.textContent = String(coins);
    watch.disabled = !flow.canWatch;
    hideDuringPlay.disabled = flow.step !== 'playing';
    const at = REWARD_STEPS.indexOf(flow.step);
    stepper.replaceChildren(
      ...REWARD_STEPS.map((step, i) =>
        h('li', {
          class: i < at ? 'done' : i === at ? 'current' : '',
          text: step === 'granted' ? `Granted +${String(REWARD)}` : step,
          attrs: i === at ? { 'aria-current': 'step' } : {},
        }),
      ),
    );
    status.textContent =
      flow.step === 'preloading'
        ? 'Preloading the reward video in view…'
        : flow.step === 'ready'
          ? 'Ready: the slot is hidden until the player asks for the video.'
          : flow.step === 'playing'
            ? 'Playing: hiding and showing the slot does not restart it.'
            : flow.step === 'granted'
              ? 'Reward granted once for this play; the next video preloads hidden.'
              : 'Completed without a play in this cycle: no reward.';
  };
  for (const name of ['video_ad_ready', 'play', 'complete']) {
    slot.on(name, () => {
      apply(flow.onEvent(name));
      render();
    });
  }

  let smallReady = false;
  small.on('video_ad_ready', () => {
    smallReady = true;
  });
  const unavailable = setTimeout(() => {
    if (!smallReady) {
      small.setStatus('unavailable');
      ctx.control('reward-unavailable', small.cid, {
        reason: `no video_ad_ready within ${String(UNAVAILABLE_AFTER_S)} s (slot < 400x300)`,
      });
    }
  }, UNAVAILABLE_AFTER_S * 1000);

  render();
  ctx.inspect(() => ({
    step: flow.step,
    grants: flow.grants,
    coins,
    canWatch: flow.canWatch,
    slotDisplay: getComputedStyle(slot.card).display,
    small: small.status,
  }));
  root.append(
    pageHeader(
      'Reward',
      'adstyle "rewarded-ad;" on a 400x300 slot. Watch the ad to earn coins; the grant happens on complete after a play.',
    ),
    h(
      'div',
      { class: 'reward-page' },
      h(
        'section',
        { class: 'card shop', attrs: { 'aria-label': 'Shop' } },
        h('h2', { class: 'label', text: 'Shop' }),
        h('p', { class: 'balance' }, h('span', { text: 'Coins ' }), balance),
        watch,
        stepper,
        status,
        hideDuringPlay,
      ),
      h('div', { class: 'reward-slots' }, slot.card, placeholder, small.card),
    ),
    note(
      'warn',
      'Client-side grant: Overwolf documents no server-side verification or postback for reward ads, so this cannot stop fraud. A slot smaller than 400x300 gets no events at all; the app treats "no video_ad_ready in 10 s" as unavailable.',
    ),
  );
  return () => {
    clearTimeout(unavailable);
    slot.dispose();
    small.dispose();
  };
};
