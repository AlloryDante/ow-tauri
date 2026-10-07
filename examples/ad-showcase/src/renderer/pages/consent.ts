/**
 * Page 8, Consent and identity: `isCMPRequired()`, the ad privacy settings
 * window, `generateUserEmailHashes()` with a fixed example address, and the
 * identity the ad stack sees (uid and muid masked; click to reveal).
 *
 * @packageDocumentation
 */
import { maskId } from '../../shared/identity.js';
import { button, h, note, pageHeader } from '../dom.js';
import type { MountPage } from './page.js';

/** The example address the hash demo uses (never a real one). */
export const EXAMPLE_EMAIL = 'player@example.com';

function masked(id: string, action: string): HTMLElement {
  const value = h('button', {
    class: 'reveal mono',
    text: maskId(id),
    attrs: { type: 'button', 'aria-label': 'Reveal value', 'aria-pressed': 'false' },
    data: { action },
  });
  let shown = false;
  value.addEventListener('click', () => {
    shown = !shown;
    value.textContent = shown ? id : maskId(id);
    value.setAttribute('aria-pressed', String(shown));
  });
  return value;
}

export const mountConsent: MountPage = (root, ctx) => {
  const { info } = ctx;
  const cmpResult = h('span', { class: 'mono', text: '–' });
  const hashes = h('pre', { class: 'mono payload' });
  hashes.textContent = '–';

  const rows: [string, Node][] = [
    ['uid (app_id)', masked(info.uid, 'consent-reveal-uid')],
    ['app_cuid (formula)', masked(info.cuid, 'consent-reveal-cuid')],
    ['uid override', h('span', { text: info.uid === info.cuid ? 'no (formula uid)' : 'yes' })],
    ['muid', masked(info.muid, 'consent-reveal-muid')],
    ['phasePercent', h('span', { class: 'mono', text: String(info.phasePercent) })],
    ['product name', h('span', { text: info.productName })],
    ['app version', h('span', { class: 'mono', text: info.appVersion })],
    [
      'host',
      h('span', { class: 'mono', text: `${info.host} ${info.hostVersion} · ${info.engine}` }),
    ],
    ['platform', h('span', { class: 'mono', text: info.platform })],
    ['test ads', h('span', { class: 'mono', text: String(info.mode === 'test') })],
  ];

  root.append(
    pageHeader(
      'Consent and identity',
      'What the ad stack sees: consent, email hashes and the app identity.',
    ),
    h(
      'div',
      { class: 'two-col' },
      h(
        'section',
        { class: 'card', attrs: { 'aria-label': 'Consent' } },
        h('h2', { class: 'label', text: 'Consent (CMP)' }),
        h(
          'div',
          { class: 'toolbar' },
          button('isCMPRequired()', 'consent-cmp-required', () => {
            cmpResult.textContent = '…';
            void ctx.api.cmpRequired().then((required) => {
              cmpResult.textContent = String(required);
              ctx.control('isCMPRequired', 'app', { result: required });
            });
          }),
          cmpResult,
        ),
        h(
          'div',
          { class: 'toolbar' },
          button('Open ad privacy settings', 'consent-open-privacy', () => {
            ctx.control('openAdPrivacySettingsWindow', 'app');
            void ctx.api.openPrivacySettings();
          }),
        ),
        h('h2', { class: 'label', text: 'Email hashes' }),
        h(
          'div',
          { class: 'toolbar' },
          button(`generateUserEmailHashes("${EXAMPLE_EMAIL}")`, 'consent-email-hashes', () => {
            void ctx.api.emailHashes(EXAMPLE_EMAIL).then((result) => {
              hashes.textContent = JSON.stringify(result, null, 2);
              ctx.control('generateUserEmailHashes', 'app', { keys: Object.keys(result) });
            });
          }),
        ),
        hashes,
        note(
          'info',
          'The hashes also reach the running ad pages (eHashes), as on ow-electron. EU users see the consent window at first start; the settings window lets them change their choice.',
        ),
      ),
      h(
        'section',
        { class: 'card', attrs: { 'aria-label': 'Identity' } },
        h('h2', { class: 'label', text: 'Identity' }),
        h(
          'dl',
          { class: 'kv' },
          ...rows.flatMap(([k, v]) => [h('dt', { text: k }), h('dd', {}, v)]),
        ),
        note(
          'info',
          'Ids are masked; click one to reveal it on screen. The app never writes them to logs or exports.',
        ),
      ),
    ),
  );
  return () => undefined;
};
