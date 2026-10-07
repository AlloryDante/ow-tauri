/**
 * Page 7, Controls: one 400x300 video slot and every documented control on
 * it, each logged as a `control:` row in the timeline: a live
 * `customTracking` update, `setAudioMuted`, pausing with `display: none`,
 * scrolling it out of view and back, hiding the window and minimizing it.
 *
 * @packageDocumentation
 */
import { button, h, note, pageHeader } from '../dom.js';
import { createSlot } from '../slots.js';
import type { MountPage } from './page.js';

/** The `<owadview>` members this page calls (present on both hosts once attached). */
interface AdviewMembers {
  setAudioMuted?: (muted: boolean) => void;
}

const DEFAULT_TRACKING = '{ "placement": "controls-page", "variant": "a" }';

export const mountControls: MountPage = (root, ctx) => {
  const slot = createSlot(ctx, { size: [400, 300], cid: 'ctl-400x300' });
  const el = slot.el as HTMLElement & AdviewMembers;

  const tracking = h('textarea', {
    class: 'input mono',
    attrs: { rows: '4', spellcheck: 'false', 'aria-label': 'customTracking JSON' },
    data: { action: 'controls-tracking' },
  });
  tracking.value = DEFAULT_TRACKING;
  const trackingStatus = h('p', { class: 'muted mono', attrs: { 'aria-live': 'polite' } });
  const applyTracking = button('Apply customTracking', 'controls-tracking-apply', () => {
    const text = tracking.value.trim();
    try {
      JSON.parse(text);
    } catch (error) {
      trackingStatus.textContent = `Not JSON: ${String(error)}`;
      return;
    }
    // The attribute is the documented equivalent of the property, and works
    // before the element is attached on both hosts.
    el.setAttribute('customTracking', text);
    trackingStatus.textContent = 'Sent to the running ad page.';
    ctx.control('customTracking', slot.cid, { value: text });
  });

  let muted = true;
  const mute = button('Unmute', 'controls-mute', () => {
    if (typeof el.setAudioMuted !== 'function') {
      ctx.control('setAudioMuted', slot.cid, { skipped: 'element not attached yet' });
      return;
    }
    muted = !muted;
    el.setAudioMuted(muted);
    mute.textContent = muted ? 'Unmute' : 'Mute';
    mute.setAttribute('aria-pressed', String(!muted));
    ctx.control('setAudioMuted', slot.cid, { muted });
  });
  mute.setAttribute('aria-pressed', 'false');

  let hidden = false;
  const displayToggle = button('display: none', 'controls-display', () => {
    hidden = !hidden;
    slot.card.style.display = hidden ? 'none' : '';
    displayToggle.textContent = hidden ? 'display: block' : 'display: none';
    if (hidden) slot.setStatus('hidden');
    ctx.control('display', slot.cid, { display: hidden ? 'none' : 'block' });
  });

  const scroller = h(
    'div',
    { class: 'ctl-scroller', attrs: { tabindex: '0', 'aria-label': 'Slot scroll box' } },
    slot.card,
    h('div', { class: 'ctl-spacer' }),
  );
  const outOfView = button('Scroll out of view', 'controls-scroll-out', () => {
    scroller.scrollTop = scroller.scrollHeight;
    ctx.control('scroll', slot.cid, { to: 'out of view' });
  });
  const backInView = button('Scroll back', 'controls-scroll-back', () => {
    scroller.scrollTop = 0;
    ctx.control('scroll', slot.cid, { to: 'in view' });
  });

  const hideWindow = button('Hide window 3 s', 'controls-hide-window', () => {
    ctx.control('window', 'app', { action: 'hide-3s' });
    void ctx.api.windowAction('hide-3s');
  });
  const minimize = button('Minimize 3 s', 'controls-minimize', () => {
    ctx.control('window', 'app', { action: 'minimize-3s' });
    void ctx.api.windowAction('minimize-3s');
  });

  root.append(
    pageHeader(
      'Controls',
      'Every documented control on one 400x300 video slot. Each action is a control row in the timeline.',
    ),
    h(
      'div',
      { class: 'two-col' },
      scroller,
      h(
        'section',
        { class: 'card controls', attrs: { 'aria-label': 'Controls' } },
        h('h2', { class: 'label', text: 'customTracking' }),
        tracking,
        h('div', { class: 'toolbar' }, applyTracking),
        trackingStatus,
        h('h2', { class: 'label', text: 'Sound and visibility' }),
        h('div', { class: 'toolbar' }, mute, displayToggle, outOfView, backInView),
        h('h2', { class: 'label', text: 'Window' }),
        h('div', { class: 'toolbar' }, hideWindow, minimize),
        note(
          'info',
          'Guests start muted. A hidden slot (display: none, scrolled out, hidden or minimized window) pauses; the ad page reloads itself and waits until it is visible again.',
        ),
      ),
    ),
  );
  return () => {
    slot.dispose();
  };
};
