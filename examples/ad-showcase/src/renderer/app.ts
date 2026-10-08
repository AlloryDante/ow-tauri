/**
 * The showcase window: top bar (mode, host, uid, consent, theme, restart),
 * numbered sidebar (keys 1 to 9), the page area and the event timeline rail.
 * Plain DOM; the same file runs on ow-electron and on ow-tauri.
 *
 * @packageDocumentation
 */
import { maskId } from '../shared/identity.js';
import type { AdMode, HostInfo, ShowcaseApi } from '../shared/ipc.js';
import { formatRoute, parseRoute } from '../shared/route.js';
import { consentChip, type ConsentState } from './consent-chip.js';
import { button, h } from './dom.js';
import { mountConsent } from './pages/consent.js';
import { mountControls } from './pages/controls.js';
import { mountHighImpact } from './pages/high-impact.js';
import { mountHouse } from './pages/house.js';
import { mountInterstitial } from './pages/interstitial.js';
import { mountLayouts } from './pages/layouts.js';
import type { PageContext, PageDef } from './pages/page.js';
import { mountParity } from './pages/parity.js';
import { mountReward } from './pages/reward.js';
import { mountSizes } from './pages/sizes.js';
import { TimelineStore } from './timeline-store.js';
import { TimelineView } from './timeline-view.js';

/** The nine pages, in sidebar order. */
export const PAGES: readonly PageDef[] = [
  { n: 1, id: 'sizes', label: 'Sizes', mount: mountSizes },
  { n: 2, id: 'layouts', label: 'Layouts', mount: mountLayouts },
  { n: 3, id: 'high-impact', label: 'High impact', mount: mountHighImpact },
  { n: 4, id: 'interstitial', label: 'Interstitial', mount: mountInterstitial },
  { n: 5, id: 'reward', label: 'Reward', mount: mountReward },
  { n: 6, id: 'house', label: 'House', mount: mountHouse },
  { n: 7, id: 'controls', label: 'Controls', mount: mountControls },
  { n: 8, id: 'consent', label: 'Consent & identity', mount: mountConsent },
  { n: 9, id: 'parity', label: 'Parity', mount: mountParity },
];

const THEME_KEY = 'ad-showcase.theme';

/** What the lab driver and devtools can read (`window.__showcase`). */
export interface ShowcaseInspection {
  /** The current page id. */
  page(): string;
  /** A JSON-safe snapshot of the window state. */
  snapshot(): Record<string, unknown>;
  /** Timeline rows from index `from` on (JSON-safe). */
  entries(from?: number): unknown[];
}

declare global {
  interface Window {
    /** The preload API. */
    showcase?: ShowcaseApi;
    /** Read-only inspection for the lab driver. */
    __showcase?: ShowcaseInspection;
  }
}

/**
 * A slot card for the inspection snapshot: its `cid`, status, `display`,
 * box rectangle and the share of the box inside the viewport.
 */
function slotState(card: HTMLElement): Record<string, unknown> {
  const box = card.querySelector<HTMLElement>('.slot-box');
  const r = box?.getBoundingClientRect();
  let inView = 0;
  if (r && r.width > 0 && r.height > 0) {
    const w = Math.max(0, Math.min(r.right, innerWidth) - Math.max(r.left, 0));
    const hgt = Math.max(0, Math.min(r.bottom, innerHeight) - Math.max(r.top, 0));
    inView = Math.round(((w * hgt) / (r.width * r.height)) * 100) / 100;
  }
  return {
    cid: box?.dataset['cid'] ?? '',
    status: card.dataset['status'] ?? '',
    display: getComputedStyle(card).display,
    rect: r ? [r.left, r.top, r.width, r.height].map(Math.round) : null,
    inView,
  };
}

function storedTheme(): 'light' | 'dark' | null {
  try {
    const v = localStorage.getItem(THEME_KEY);
    return v === 'light' || v === 'dark' ? v : null;
  } catch {
    return null;
  }
}

function applyTheme(theme: 'light' | 'dark' | null): void {
  if (theme) document.documentElement.dataset['theme'] = theme;
  else delete document.documentElement.dataset['theme'];
}

function effectiveTheme(): 'light' | 'dark' {
  const set = document.documentElement.dataset['theme'];
  if (set === 'light' || set === 'dark') return set;
  return matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
}

/**
 * Builds the window into `root` and opens page 1.
 *
 * @param root - the element to render into
 * @param api - the preload API
 * @param info - host, mode and identity
 */
export function startApp(root: HTMLElement, api: ShowcaseApi, info: HostInfo): void {
  const t0 = performance.now();
  const store = new TimelineStore();
  const now = (): number => performance.now() - t0;
  const control = (action: string, cid = 'app', details: Record<string, unknown> = {}): void => {
    store.add({
      t: now(),
      cid,
      name: `control:${action}`,
      family: 'control',
      payload: details,
      sinceMount: null,
    });
  };

  applyTheme(storedTheme());

  // ---------------------------------------------------------------- top bar
  const modeBadge = h('span', {
    class: `badge badge-${info.mode}`,
    text: info.mode.toUpperCase(),
    attrs: { 'aria-label': `Ad mode ${info.mode}` },
  });
  const hostChip = h('span', {
    class: 'mono muted top-host',
    text: `host: ${info.host} ${info.hostVersion} · ${info.engine} · ${info.platform}`,
  });
  let uidShown = false;
  const uidButton = h('button', {
    class: 'reveal mono',
    text: `uid ${maskId(info.uid)}`,
    attrs: { type: 'button', 'aria-pressed': 'false', title: 'Click to reveal' },
    data: { action: 'top-reveal-uid' },
    onClick: () => {
      uidShown = !uidShown;
      uidButton.textContent = `uid ${uidShown ? info.uid : maskId(info.uid)}`;
      uidButton.setAttribute('aria-pressed', String(uidShown));
    },
  });
  const consentEl = h('span', { class: 'chip', attrs: { role: 'status' } });
  const showConsent = (state: ConsentState): void => {
    const chip = consentChip(state);
    consentEl.textContent = chip.text;
    consentEl.className = `chip tone-${chip.tone}`;
    consentEl.title = chip.title;
    consentEl.dataset['state'] = state;
  };
  showConsent('checking');
  void api.cmpRequired().then(
    (required) => {
      showConsent(required ? 'required' : 'not-required');
    },
    () => {
      showConsent('failed');
    },
  );
  const themeButton = button(
    'Theme',
    'top-theme',
    () => {
      const next = effectiveTheme() === 'dark' ? 'light' : 'dark';
      applyTheme(next);
      try {
        localStorage.setItem(THEME_KEY, next);
      } catch {
        // The choice lasts for this session only.
      }
    },
    'btn-ghost',
  );
  themeButton.setAttribute('aria-label', 'Switch light or dark theme');

  const other: AdMode = info.mode === 'test' ? 'live' : 'test';
  const banner = h('div', { class: 'banner', attrs: { role: 'alert' } });
  banner.hidden = true;
  const restart = (mode: AdMode): void => {
    control('restart', 'app', { mode, route: currentRoute() });
    void api.restart(mode, currentRoute());
  };
  const restartButton = button(`Restart in ${other.toUpperCase()}`, 'top-restart', () => {
    if (other === 'test') {
      restart('test');
      return;
    }
    banner.replaceChildren(
      h('span', {
        text: 'Restart with LIVE ads? Real ad requests go out for this app uid. Never click an ad you are not allowed to.',
      }),
      button(
        'Restart LIVE',
        'top-restart-confirm',
        () => {
          restart('live');
        },
        'btn-primary',
      ),
      button(
        'Cancel',
        'top-restart-cancel',
        () => {
          banner.hidden = true;
        },
        'btn-ghost',
      ),
    );
    banner.hidden = false;
  });

  const topBar = h(
    'header',
    { class: 'topbar' },
    h(
      'div',
      { class: 'brand' },
      h('span', { class: 'brand-dot', attrs: { 'aria-hidden': 'true' } }),
      h('span', { class: 'brand-name', text: 'ow-tauri Ad Showcase' }),
      modeBadge,
    ),
    hostChip,
    h('div', { class: 'top-right' }, uidButton, consentEl, themeButton, restartButton),
  );

  // ----------------------------------------------------------------- layout
  const main = h('main', { class: 'content', attrs: { tabindex: '-1' } });
  const shell = h('div', { class: 'shell' });
  const view = new TimelineView({
    store,
    exportJson: async () => {
      const json = JSON.stringify(
        store.toExport(
          {
            host: info.host,
            hostVersion: info.hostVersion,
            mode: info.mode,
            uidMasked: maskId(info.uid),
            platform: info.platform,
          },
          new Date(),
        ),
        null,
        2,
      );
      const result = await api.exportTimeline({ json });
      control('export', 'app', { file: result.path.split(/[\\/]/).pop() ?? '' });
      return result.path;
    },
    onToggle: (collapsed) => {
      shell.classList.toggle('rail-collapsed', collapsed);
    },
  });

  const navButtons = new Map<string, HTMLButtonElement>();
  const nav = h('nav', { class: 'sidebar', attrs: { 'aria-label': 'Pages' } });
  for (const page of PAGES) {
    const b = h(
      'button',
      {
        class: 'nav-item',
        attrs: { type: 'button', 'aria-keyshortcuts': String(page.n) },
        data: { page: page.id, action: `nav-${page.id}` },
        onClick: () => {
          open(page);
        },
      },
      h('span', { class: 'nav-n mono', text: String(page.n) }),
      h('span', { text: page.label }),
    );
    navButtons.set(page.id, b);
    nav.append(b);
  }
  shell.append(nav, main, view.element);
  root.replaceChildren(topBar, banner, shell);

  // ------------------------------------------------------------------ pages
  let current: PageDef | null = null;
  let currentArg: string | null = null;
  let cleanup: (() => void) | null = null;
  let inspectPage: (() => Record<string, unknown>) | null = null;
  const currentRoute = (): string =>
    current ? formatRoute({ page: current.id, arg: currentArg }) : '';

  const pageListeners = new Set<(event: { name: string }) => void>();
  api.onWindowEvent((event) => {
    store.add({
      t: now(),
      cid: 'app',
      name: `window:${event.name}`,
      family: 'lifecycle',
      payload: event.detail ?? {},
      sinceMount: null,
    });
    for (const listener of pageListeners) listener(event);
  });

  function open(page: PageDef, arg: string | null = null): void {
    if (current?.id === page.id) return;
    cleanup?.();
    cleanup = null;
    inspectPage = null;
    main.replaceChildren();
    pageListeners.clear();
    current = page;
    currentArg = arg;
    for (const [id, b] of navButtons) {
      if (id === page.id) b.setAttribute('aria-current', 'page');
      else b.removeAttribute('aria-current');
    }
    // The rail's default scope is this visit: the page's own elements plus
    // app and control rows from now on.
    store.beginVisit();
    control('page', 'app', { page: page.id });
    const pageRoot = h('div', { class: `page page-${page.id}`, data: { page: page.id } });
    main.append(pageRoot);
    main.scrollTop = 0;
    const ctx: PageContext = {
      store,
      now,
      mode: info.mode,
      control,
      info,
      api,
      setRailCollapsed: (collapsed) => {
        view.setCollapsed(collapsed);
      },
      railCollapsed: () => view.collapsed,
      onWindowEvent: (listener) => {
        pageListeners.add(listener);
        return () => pageListeners.delete(listener);
      },
      arg,
      setArg: (next) => {
        if (current?.id !== page.id) return;
        // Kept in memory only: an in-page URL change (history.replaceState)
        // counts as a navigation for ow-electron's ad elements, which then
        // stop loading [OBS, showcase lab]. Restart passes the route instead.
        currentArg = next;
      },
      inspect: (fn) => {
        inspectPage = fn;
      },
    };
    cleanup = page.mount(pageRoot, ctx);
  }

  document.addEventListener('keydown', (event) => {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const target = event.target as HTMLElement | null;
    if (target && /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName)) return;
    const page = PAGES.find((p) => String(p.n) === event.key);
    if (page) {
      event.preventDefault();
      open(page);
      navButtons.get(page.id)?.focus();
    }
  });

  window.__showcase = Object.freeze({
    page: () => current?.id ?? '',
    snapshot: () => ({
      page: current?.id ?? '',
      route: currentRoute(),
      host: info.host,
      mode: info.mode,
      theme: effectiveTheme(),
      railCollapsed: view.collapsed,
      viewport: [innerWidth, innerHeight],
      owadviews: document.querySelectorAll('owadview').length,
      slots: [...document.querySelectorAll<HTMLElement>('.slot[data-status]')].map(slotState),
      state: inspectPage?.() ?? null,
      counts: Object.fromEntries(store.counts()),
      entries: store.entries.length,
    }),
    entries: (from = 0) => store.entries.slice(from),
  });

  // The hash (`#page/arg`, set by `--showcase-page` at start) picks the first
  // page; page 1 otherwise.
  const start = parseRoute(location.hash);
  const startPage = PAGES.find((p) => p.id === start?.page);
  const first = startPage ?? PAGES[0];
  if (first) open(first, startPage ? (start?.arg ?? null) : null);
}
