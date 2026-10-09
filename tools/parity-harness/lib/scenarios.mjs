// Round-2 scenarios: option presets plus a timed action script that
// app/scenario.cjs runs inside the ow-electron app. Action times are in ms
// after the main window finished loading (or after ready with no window).
//
// Every scenario keeps the harness rules: no visible window, no input sent
// to any page, test ads unless --mode live --live-ok is given.

const OFFSCREEN = { x: -20000, y: -20000, center: false };
const TEST_EMAIL = 'test.email@overwolf.com';

/** The performance (interstitial) ad of the docs example, appended to <body>. */
const PERF_DOC = {
  parent: 'body',
  cid: null,
  customTracking: false,
  attrs: {
    performance: '',
    adstyle: 'background-color: rgba(255, 0, 0, 0.5); background-blur: -1;',
  },
};
/** The app "opts in" to a ready rewarded ad by showing its slot again (display:none for 1 s). */
const REWARD_OPT_IN = (times) => [
  { at: 12000, do: 'hook-guest-frames', label: 'oam' },
  ...times.flatMap((at) => [
    {
      at,
      do: 'page-eval',
      label: 'reward slot hidden',
      code: `document.querySelector('.slot').style.display = 'none'; 'ok'`,
    },
    {
      at: at + 1000,
      do: 'page-eval',
      label: 'reward slot shown (opt-in)',
      code: `document.querySelector('.slot').style.display = ''; 'ok'`,
    },
    { at: at + 3000, do: 'hook-guest-frames', label: 'oam-again' },
  ]),
  { at: 20000, do: 'probe-guests', label: 'reward+20s' },
];
const PERF_PROBES = [
  { at: 15000, do: 'probe-guests', label: 'perf+12s' },
  { at: 45000, do: 'probe-guests', label: 'perf+42s' },
  { at: 80000, do: 'probe-guests', label: 'perf+77s' },
];

/** The lab's interstitial (L1/L4): darker, with a positive blur. */
const PERF_LAB = {
  ...PERF_DOC,
  id: 'perf-lab',
  attrs: {
    performance: '',
    adstyle: 'background-color: rgba(0, 0, 0, 0.8); background-blur: 3;',
  },
};
/** Lab hit probe (page hit test, native hit test on Tauri, snapshots). */
const LAB_POINTS = [
  { name: 'control', selector: '#parity-control' },
  { name: 'reward-slot', selector: 'owadview[cid="parity_lab_reward"]' },
  { name: 'std-slot', selector: 'owadview[cid^="parity_lab_std"]' },
  { name: 'corner', x: 1150, y: 40 },
];
const hitProbe = (at, label, extra = {}) => ({
  at,
  do: 'hit-probe',
  label,
  points: LAB_POINTS,
  ...extra,
});
/** The app calls setAudioMuted(muted) on every element (L5). */
const muteAll = (at, muted) => ({
  at,
  do: 'page-eval',
  label: `setAudioMuted(${muted})`,
  code: `document.querySelectorAll('owadview').forEach((el) => el.setAudioMuted(${muted})); 'ok'`,
});

/**
 * The in-view sweep: moves the `inview-sweep` container's `side` (`top` or
 * `left`) so that `lo` % up to `hi` % of the slot is inside the viewport, in
 * steps of `pct` % every 1.5 s, then back down to `lo` %. `from` is the
 * offset in px with 0 % in view and `px` the offset of 1 %. Each step's label
 * is the planned share; the page reads the real share back after the move.
 */
const inviewSweep = (start, side, from, px, { lo = 0, hi = 100, pct = 5 } = {}) => {
  const up = [];
  for (let k = lo; k <= hi; k += pct) up.push(k);
  const shares = [...up, ...up.slice(0, -1).reverse()];
  return shares.map((k, i) => ({
    at: start + i * 1500,
    do: 'page-eval',
    label: `sweep ${side} ${k} %`,
    code: `(() => { const box = document.getElementById('inview-sweep'); box.style.${side} = '${from + k * px}px'; const r = box.getBoundingClientRect(); const w = Math.max(0, Math.min(r.right, innerWidth) - Math.max(r.left, 0)); const h = Math.max(0, Math.min(r.bottom, innerHeight) - Math.max(r.top, 0)); return { share: Math.round((w * h * 1000) / (r.width * r.height)) / 1000, at: performance.now() }; })()`,
  }));
};

/** Feature-flag stand-in responses for `--features <preset>` (cmp-required scenario). */
export const FEATURE_PRESETS = {
  empty: [{ status: 200, body: '{"params":[]}' }],
  'empty-object': [{ status: 200, body: '{}' }],
  'params-false': [{ status: 200, body: '{"params":[false]}' }],
  'params-true': [{ status: 200, body: '{"params":[true]}' }],
  'params-string-false': [{ status: 200, body: '{"params":["false"]}' }],
  'params-name-value': [
    { status: 200, body: '{"params":[{"name":"cmp-eu-only","value":"false"}]}' },
  ],
  'params-key-value': [{ status: 200, body: '{"params":[{"key":"enabled","value":false}]}' }],
  'enabled-false': [{ status: 200, body: '{"enabled":false,"params":[]}' }],
  status500: [{ status: 500, body: '' }],
  status404: [{ status: 404, body: '' }],
  invalid: [{ status: 200, body: 'not json' }],
  drop: [{ status: 'drop' }],
  hang: [{ status: 200, body: '{"params":[]}', delayMs: 45000 }],
  // §5.2 #13: past ow-tauri's 60 s consent timeout.
  'hang-long': [{ status: 200, body: '{"params":[]}', delayMs: 70000 }],
  // First answer empty, later answers non-empty: shows whether later calls refetch.
  'changes-later': [
    { status: 200, body: '{"params":[]}' },
    { status: 200, body: '{"params":[false]}' },
  ],
};

const w = (key, at, options, extra = {}) => ({
  at,
  do: 'open-window',
  key,
  options,
  ...extra,
});
const wx = (key, at, method, args) => ({ at, do: 'extra-window', key, method, args });

/**
 * setUserEmailHashes(...args) at `at`, the state file 1 s later and a guest
 * probe 2 s later (email-hashes-clear).
 */
const emailHashCall = (at, name, args) => [
  { at, do: 'ow-call', fn: 'setUserEmailHashes', args, label: `setUserEmailHashes(${name})` },
  { at: at + 1000, do: 'state-file', label: `after-${name}` },
  { at: at + 2000, do: 'probe-guests', label: `after-${name.replace(/\W+/g, '') || 'empty'}` },
];

/** @type {Record<string, {describe: string, defaults: Record<string, unknown>, config?: Record<string, unknown>}>} */
export const SCENARIOS = {
  messages: {
    describe:
      'R2-1/2/3/4/12/13: host->guest and guest->host IPC over an ad lifecycle; email hashes, external payment id, FPD and optimisation opt-outs; element display/scroll, window resize/hide/show/focus/blur/retitle; element API and pageurl/unit attributes.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250,400x600', duration: 100 },
    config: {
      elementAttrs: [{ pageurl: 'https://example.com/parity/page-one' }, { unit: 'parity-unit' }],
      actions: [
        { at: 3000, do: 'listeners', label: 'early' },
        { at: 12000, do: 'probe-guests', label: 'pre-actions' },
        { at: 12500, do: 'listeners', label: 'attached' },
        {
          at: 15000,
          do: 'page-eval',
          label: 'customTracking change',
          code: `document.querySelector('owadview').setAttribute('customTracking', JSON.stringify({parityHarness: 'changed'})); 'ok'`,
        },
        { at: 18000, do: 'ow-call', fn: 'generateUserEmailHashes', args: [TEST_EMAIL] },
        {
          at: 18300,
          do: 'ow-call',
          fn: 'generateUserEmailHashes',
          args: ['  Test.Email@Overwolf.COM  '],
          label: 'generateUserEmailHashes(mixed case, spaces)',
        },
        {
          at: 18600,
          do: 'ow-call',
          fn: 'generateUserEmailHashes',
          args: ['not-an-email'],
          label: 'generateUserEmailHashes(not an email)',
        },
        { at: 19000, do: 'ow-call', fn: 'setUserEmailHashes', generateFrom: TEST_EMAIL },
        { at: 22000, do: 'probe-guests', label: 'after-email' },
        {
          at: 24000,
          do: 'ow-call',
          fn: 'setExternalPaymentUserId',
          args: [{ providerName: 'tebex', userId: 'parity-test' }],
        },
        {
          at: 25500,
          do: 'ow-call',
          fn: 'setExternalPaymentUserId',
          args: [{ userId: 'parity-test-default-provider', paymentId: 'parity-payment' }],
          label: 'setExternalPaymentUserId(no provider, paymentId)',
        },
        {
          at: 26500,
          do: 'ow-call',
          fn: 'setExternalPaymentUserId',
          args: [{ providerName: 'tebex' }],
          sync: true,
          label: 'setExternalPaymentUserId(no userId)',
        },
        { at: 28000, do: 'ow-call', fn: 'disableAdsFPD' },
        { at: 30000, do: 'probe-guests', label: 'after-fpd' },
        {
          at: 32000,
          do: 'page-eval',
          label: 'slot display none',
          code: `document.querySelector('.slot').style.display = 'none'; 'ok'`,
        },
        {
          at: 37000,
          do: 'page-eval',
          label: 'slot display restore',
          code: `document.querySelector('.slot').style.display = ''; 'ok'`,
        },
        {
          at: 40000,
          do: 'page-eval',
          label: 'slots pushed below the viewport',
          code: `document.body.style.paddingTop = '4000px'; 'ok'`,
        },
        {
          at: 45000,
          do: 'page-eval',
          label: 'slots back in the viewport',
          code: `document.body.style.paddingTop = ''; 'ok'`,
        },
        { at: 48000, do: 'window', method: 'setSize', args: [1200, 1000] },
        { at: 52000, do: 'window', method: 'setSize', args: [1000, 700] },
        { at: 55000, do: 'window', method: 'hide' },
        { at: 56000, do: 'probe-guests', label: 'window-hidden' },
        { at: 62000, do: 'window', method: 'showInactive' },
        { at: 67000, do: 'window', method: 'emit', args: ['focus'] },
        { at: 68000, do: 'probe-guests', label: 'after-focus-event' },
        { at: 70000, do: 'window', method: 'emit', args: ['blur'] },
        { at: 73000, do: 'window', method: 'setTitle', args: ['Renamed Title'] },
        { at: 75000, do: 'probe-guests', label: 'after-retitle' },
        { at: 78000, do: 'ow-call', fn: 'disableAdsOptimization' },
        { at: 81000, do: 'probe-guests', label: 'after-disable-optimization' },
        { at: 85000, do: 'listeners', label: 'end' },
      ],
    },
  },

  introspect: {
    describe: 'Own members and IPC listener names on the ad guests (names only), 1 slot.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 35 },
    config: {
      emitterTrace: true,
      actions: [
        { at: 12000, do: 'introspect', label: 'attached' },
        { at: 14000, do: 'window', method: 'hide' },
        { at: 22000, do: 'window', method: 'showInactive' },
      ],
    },
  },

  crash: {
    describe:
      'R2-5: forcefullyCrashRenderer() on the ad guest, 3 spaced crashes then 3 within 4 s; reload timing, element events, analytics.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 160 },
    config: {
      actions: [
        { at: 15000, do: 'probe-guests', label: 'pre-crash' },
        { at: 20000, do: 'crash-guests' },
        { at: 50000, do: 'crash-guests' },
        { at: 80000, do: 'crash-guests' },
        { at: 110000, do: 'crash-guests' },
        { at: 112000, do: 'crash-guests' },
        { at: 114000, do: 'crash-guests' },
        { at: 150000, do: 'probe-guests', label: 'post-crash' },
      ],
    },
  },

  block: {
    describe:
      'R2-5: the first ad page navigation is pointed at a closed local port (connection refused); load-error retry timing and cap.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 120 },
    config: { failGuestLoads: 1, actions: [] },
  },

  cmp: {
    describe:
      'R2-6: openAdPrivacySettingsWindow() and openCMPWindow() with and without options, each closed before the next. Every window is pinned to opacity 0 before the constructor shows it (calibrated first), hidden on show, and watched by the window monitor. --screencapture adds main-display captures.',
    defaults: { mode: 'test', present: 'hidden', duration: 80 },
    config: {
      noWindow: true,
      calibrate: true,
      windowMonitorRequired: true,
      actions: [
        { at: 1500, do: 'screencapture', label: 'baseline' },
        {
          at: 3000,
          do: 'cmp-open',
          fn: 'openAdPrivacySettingsWindow',
          options: OFFSCREEN,
          label: 'privacy-offscreen',
        },
        { at: 6000, do: 'screencapture', label: 'privacy-offscreen' },
        {
          at: 8000,
          do: 'cmp-open',
          fn: 'openAdPrivacySettingsWindow',
          options: OFFSCREEN,
          label: 'privacy-while-open',
        },
        { at: 13000, do: 'cmp-state', label: 'privacy-offscreen' },
        { at: 14000, do: 'cmp-close', label: 'privacy-offscreen' },
        {
          at: 18000,
          do: 'cmp-open',
          fn: 'openCMPWindow',
          options: {
            ...OFFSCREEN,
            tab: 'vendors',
            language: 'de',
            width: 700,
            height: 500,
            backgroundColor: '#123456',
            preLoaderSpinnerColor: '#654321',
          },
          label: 'cmp-vendors-de',
        },
        { at: 21000, do: 'screencapture', label: 'cmp-vendors-de' },
        { at: 27000, do: 'cmp-close', label: 'cmp-vendors-de' },
        {
          at: 31000,
          do: 'cmp-open',
          fn: 'openAdPrivacySettingsWindow',
          options: { tab: 'features' },
          label: 'privacy-default-position',
        },
        { at: 34000, do: 'screencapture', label: 'privacy-default-position' },
        { at: 41000, do: 'cmp-close', label: 'privacy-default-position' },
        { at: 45000, do: 'cmp-open', fn: 'openCMPWindow', label: 'cmp-no-options' },
        { at: 55000, do: 'cmp-close', label: 'cmp-no-options' },
        { at: 60000, do: 'snapshot', label: 'after-cmp' },
        { at: 66000, do: 'screencapture', label: 'end' },
      ],
    },
  },

  'cmp-required': {
    describe:
      'R2-7: isCMPRequired() four times (two concurrent) with no other startup calls; use with --features <preset> (local stand-in) and --home profile:NAME for restarts.',
    defaults: { mode: 'test', present: 'hidden', duration: 12 },
    config: {
      noWindow: true,
      skipStartupCalls: true,
      actions: [
        { at: 0, do: 'ow-call', fn: 'isCMPRequired', label: 'call-1' },
        { at: 5, do: 'ow-call', fn: 'isCMPRequired', label: 'call-2-concurrent' },
        { at: 4000, do: 'ow-call', fn: 'isCMPRequired', label: 'call-3' },
        { at: 8000, do: 'ow-call', fn: 'isCMPRequired', label: 'call-4' },
      ],
    },
  },

  windows: {
    describe:
      'R2-8: window analytics names and window_closed rules: name option variants, file names, titles, retitle, hide without close, <1 s visible, duplicates, quit with windows open.',
    defaults: {
      mode: 'test',
      present: 'transparent',
      duration: 45,
      quitStyle: 'quit',
      offline: true,
    },
    config: {
      noWindow: true,
      actions: [
        w('A-file-settings', 1000, {}, { file: 'settings.html' }),
        w('B-name-hyphen', 1200, { name: 'my-window' }, { file: 'settings.html' }),
        w('C-name-long-spaces', 1400, { name: 'Window With Spaces And A Very Long Name 123' }),
        w('D-name-symbols', 1600, { name: 'wín#dow!' }),
        w('E-title-retitle', 1800, { title: 'Ctor Title' }, { file: 'page-two.htm' }),
        w('F-empty-title', 2000, { title: '' }, { file: 'My Page.html' }),
        w('G-query', 2200, {}, { file: 'deep.html', query: 'x=1&y=2' }),
        w('H-short', 2400, {}, { file: 'short.html' }),
        w('I-hide-then-close', 2600, {}, { file: 'hidden-later.html' }),
        w('J-hide-never-close', 2800, {}, { file: 'hidden-forever.html' }),
        w('K-never-shown', 3000, {}, { file: 'never.html', show: false }),
        w('L-dup-1', 3200, { name: 'dup' }),
        w('L-dup-2', 3400, { name: 'dup' }),
        w('M-open-at-quit', 3600, { name: 'still-open' }),
        w('N-index', 3800, {}, { file: 'index.html', query: 'layouts=none' }),
        wx('H-short', 2800, 'close'),
        wx('E-title-retitle', 5000, 'setTitle', ['Renamed']),
        wx('I-hide-then-close', 6600, 'hide'),
        wx('J-hide-never-close', 6800, 'hide'),
        wx('K-never-shown', 8000, 'close'),
        wx('A-file-settings', 10000, 'close'),
        wx('B-name-hyphen', 10200, 'close'),
        wx('C-name-long-spaces', 10400, 'close'),
        wx('D-name-symbols', 10600, 'close'),
        wx('E-title-retitle', 10800, 'close'),
        wx('F-empty-title', 11000, 'close'),
        wx('G-query', 11200, 'close'),
        wx('L-dup-1', 11400, 'close'),
        wx('L-dup-2', 11600, 'close'),
        wx('N-index', 11800, 'close'),
        wx('I-hide-then-close', 16600, 'close'),
        w('O-reopened', 18000, { name: 'reopen' }),
        wx('O-reopened', 21000, 'close'),
        w('O-reopened', 22000, { name: 'reopen' }),
        wx('O-reopened', 25000, 'close'),
        w('P-shown-twice', 26000, { name: 'shown-twice' }),
        wx('P-shown-twice', 28000, 'hide'),
        wx('P-shown-twice', 30000, 'showInactive'),
        wx('P-shown-twice', 33000, 'close'),
      ],
    },
  },

  'windows-urls': {
    describe:
      'R2-8, part 2: window analytics names for remote URLs, about:blank, data: URLs and unusual file names (offline).',
    defaults: {
      mode: 'test',
      present: 'transparent',
      duration: 30,
      quitStyle: 'quit',
      offline: true,
    },
    config: {
      noWindow: true,
      actions: [
        w('a-remote', 500, {}, { url: 'https://example.com/some/path/page.php?x=1#frag' }),
        w('b-remote-root', 700, {}, { url: 'https://example.com/' }),
        w('c-about-blank', 900, {}, { url: 'about:blank' }),
        w('d-data', 1100, {}, { url: 'data:text/html,<title>d</title>' }),
        w('e-upper', 1300, {}, { file: 'UPPER Case-Name_1.HTML' }),
        w('f-long', 1500, {}, { file: 'a-very-long-file-name-over-twenty-chars.html' }),
        w('g-unicode', 1700, {}, { file: 'pâgé ünï.html' }),
        w('h-noext', 1900, {}, { file: 'noext' }),
        w('i-double-ext', 2100, {}, { file: 'a.b.html' }),
        w('j-sub', 2300, {}, { file: 'sub.dir.html', query: 'q=1' }),
        w('k-name-only', 2500, { name: 'named' }, { url: 'about:blank' }),
        w('l-750ms', 2700, {}, { file: 'ms750.html' }),
        w('m-1500ms', 2900, {}, { file: 'ms1500.html' }),
        wx('l-750ms', 3450, 'close'),
        wx('m-1500ms', 4400, 'close'),
        w('n-navigates', 3100, {}, { file: 'first.html' }),
        {
          at: 5000,
          do: 'extra-window',
          key: 'n-navigates',
          method: 'loadURL',
          args: ['about:blank#second'],
        },
        ...[
          'a-remote',
          'b-remote-root',
          'c-about-blank',
          'd-data',
          'e-upper',
          'f-long',
          'g-unicode',
          'h-noext',
          'i-double-ext',
          'j-sub',
          'k-name-only',
          'n-navigates',
        ].map((key, i) => wx(key, 9000 + i * 300, 'close')),
      ],
    },
  },

  offscreen: {
    describe:
      'Live-fill question: the ad window shown at opacity 0 and moved off-screen (-20000,-20000). Combine with --mode live --live-ok --max-live-loads N for live demand.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 90 },
    config: { windowPosition: [-20000, -20000] },
  },

  // --- Round 3: ad formats (docs: monetization/advertising/*) ----------------
  perf: {
    describe:
      'R3: performance (interstitial) ad exactly as the docs example: <owadview performance adstyle="background-color: rgba(255, 0, 0, 0.5); background-blur: -1;"> appended to <body> at 3 s, no cid, in a 1200x800 window (docs minimum 1000x600). Element events, removal, layout samples, guest probes.',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 120 },
    config: {
      window: { width: 1200, height: 800 },
      elementSpec: [{ at: 3000, ...PERF_DOC }],
      actions: PERF_PROBES,
    },
  },

  'perf-sample': {
    describe:
      'R3: performance ad as the official sample creates it: only the empty `performance` attribute, appended to <body> at 3 s (1200x800 window).',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 120 },
    config: {
      window: { width: 1200, height: 800 },
      elementSpec: [
        { at: 3000, parent: 'body', cid: null, customTracking: false, attrs: { performance: '' } },
      ],
      actions: PERF_PROBES,
    },
  },

  'perf-unit': {
    describe:
      'R3: the sample\'s commented-out `unit` attribute: a performance ad with unit="parity_unit" at 3 s, and a 400x300 standard slot with the same unit; where does `unit` reach the ad library (forceAdUnit)?',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 60 },
    config: {
      window: { width: 1200, height: 800 },
      elementSpec: [
        { at: 0, layout: '400x300', attrs: { unit: 'parity_unit' }, cid: 'parity_unit_slot' },
        { at: 3000, ...PERF_DOC, attrs: { ...PERF_DOC.attrs, unit: 'parity_unit' } },
      ],
      actions: PERF_PROBES.slice(0, 2),
    },
  },

  'perf-small': {
    describe:
      'R3: performance ad (docs example) in a 900x480 window, below the documented 1000x600 minimum and, on every platform, below the 500x500 content area the ad itself requires. (ow-tauri sizes are content sizes, CONTRACT B.2.2; on Windows a 900x500 ow-electron window has only 884x435 of content, so 500 high passed on ow-tauri alone.)',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 90 },
    config: {
      window: { width: 900, height: 480 },
      elementSpec: [{ at: 3000, ...PERF_DOC }],
      actions: PERF_PROBES,
    },
  },

  'perf-with-standard': {
    describe:
      'R3: a 300x250 standard ad running, then a performance ad (docs example) added at 20 s; is the standard slot paused/reloaded while the overlay is up, and what follows. Then a second performance ad at 80 s.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 150 },
    config: {
      window: { width: 1200, height: 800 },
      elementSpec: [
        { at: 20000, ...PERF_DOC },
        { at: 80000, ...PERF_DOC, id: 'perf2' },
      ],
      actions: [
        { at: 15000, do: 'probe-guests', label: 'before-perf' },
        { at: 30000, do: 'probe-guests', label: 'perf+10s' },
        { at: 60000, do: 'probe-guests', label: 'perf+40s' },
        { at: 100000, do: 'probe-guests', label: 'perf2+20s' },
      ],
    },
  },

  'perf-twice': {
    describe:
      'R3: two performance ads (docs example) appended 1 s apart; the docs promise one per window ("other interstitial ad already initiated" in the native SDK).',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 90 },
    config: {
      window: { width: 1200, height: 800 },
      elementSpec: [
        { at: 3000, ...PERF_DOC },
        { at: 4000, ...PERF_DOC, id: 'perf2' },
      ],
      actions: PERF_PROBES,
    },
  },

  'perf-remove': {
    describe:
      'R3: performance ad (docs example) removed by the app 25 s after it was added; what the host sends and dispatches on removal.',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 60 },
    config: {
      window: { width: 1200, height: 800 },
      elementSpec: [{ at: 3000, ...PERF_DOC }],
      actions: [
        { at: 15000, do: 'probe-guests', label: 'perf+12s' },
        {
          at: 28000,
          do: 'page-eval',
          label: 'app removes the performance ad',
          code: `document.querySelectorAll('owadview[performance]').forEach((e) => e.remove()); 'ok'`,
        },
      ],
    },
  },

  'high-impact': {
    describe:
      'R3: the documented high-impact ad zone: #ads-parent 440 wide x window height (min 670), a 400x600 container with <owadview adstyle="high-impact-ad;"> and a 400x60 container; the documented listeners expand/restore it. Window 1000x760.',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 150 },
    config: {
      window: { width: 1000, height: 760 },
      elementSpec: [{ zone: 'high-impact' }],
      actions: [
        { at: 15000, do: 'probe-guests', label: 'hi+15s' },
        { at: 45000, do: 'probe-guests', label: 'hi+45s' },
        { at: 100000, do: 'probe-guests', label: 'hi+100s' },
      ],
    },
  },

  'high-impact-small-zone': {
    describe:
      'R3: high-impact zone in a 1000x560 window, so #ads-parent is below the documented 670 px minimum height.',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 90 },
    config: {
      window: { width: 1000, height: 560 },
      elementSpec: [{ zone: 'high-impact' }],
      actions: [{ at: 15000, do: 'probe-guests', label: 'hi+15s' }],
    },
  },

  reward: {
    describe:
      'R3: reward ad, full flow: adstyle="rewarded-ad;" on a 400x300 container (smaller slots are refused). The library preloads (video_ad_ready); the app opts the user in by showing the slot (here: display:none for 1 s at 15 s, as a collapsed "watch for reward" panel would), which posts userPlay and plays (play, impression ... complete = reward). A second opt-in at 75 s. App-side DOM changes only, no input events.',
    defaults: { mode: 'test', present: 'transparent', layout: '400x300', duration: 130 },
    config: {
      elementAttrs: [{ adstyle: 'rewarded-ad;' }],
      actions: REWARD_OPT_IN([15000, 75000]),
    },
  },

  'reward-two-slots': {
    describe:
      'R3: adstyle="rewarded-ad;" on a 400x300 and a 400x600 container at once, never re-shown: preload only (video_ad_ready, player_loaded), nothing plays.',
    defaults: { mode: 'test', present: 'transparent', layout: '400x300,400x600', duration: 60 },
    config: {
      elementAttrs: [{ adstyle: 'rewarded-ad;' }, { adstyle: 'rewarded-ad;' }],
      actions: [{ at: 20000, do: 'probe-guests', label: 'reward+20s' }],
    },
  },

  'send-command-probe': {
    describe:
      'R3: how <owadview>.sendCommand(...) and setPageUrl(...) travel (host -> guest message shape and guest reaction), on a rewarded 400x300 slot. App-side API calls only; no input events.',
    defaults: { mode: 'test', present: 'transparent', layout: '400x300', duration: 45 },
    config: {
      elementAttrs: [{ adstyle: 'rewarded-ad;' }],
      actions: [
        {
          at: 15000,
          do: 'page-eval',
          label: 'sendCommand(parity-probe)',
          code: `(() => { const el = document.querySelector('owadview'); try { return String(el.sendCommand('parity-probe', { a: 1 })); } catch (e) { return 'threw: ' + e; } })()`,
        },
        {
          at: 17000,
          do: 'page-eval',
          label: 'sendCommand() no args',
          code: `(() => { const el = document.querySelector('owadview'); try { return String(el.sendCommand()); } catch (e) { return 'threw: ' + e; } })()`,
        },
        {
          at: 19000,
          do: 'page-eval',
          label: 'sendCommand.length + toString',
          code: `(() => { const f = document.querySelector('owadview').sendCommand; return [f.length, String(f).slice(0, 400)]; })()`,
        },
        {
          at: 21000,
          do: 'page-eval',
          label: 'setPageUrl',
          code: `(() => { const el = document.querySelector('owadview'); try { return String(el.setPageUrl('https://example.com/parity/page-two')); } catch (e) { return 'threw: ' + e; } })()`,
        },
        { at: 25000, do: 'probe-guests', label: 'after-commands' },
      ],
    },
  },

  'reward-play-probe': {
    describe:
      'R3: what makes a ready rewarded ad play. Frame message hook at 12 s, the embedder window "focus" event at 15 s (GUEST_INSTANCE_FOCUS_CHANGE true), then sendCommand candidates 20 s apart; app-side calls only, no input events.',
    defaults: { mode: 'test', present: 'transparent', layout: '400x300', duration: 150 },
    config: {
      elementAttrs: [{ adstyle: 'rewarded-ad;' }],
      actions: [
        { at: 12000, do: 'hook-guest-frames', label: 'oam' },
        { at: 15000, do: 'window', method: 'emit', args: ['focus'] },
        ...['userPlay', 'play', 'playAd', 'showRewardedAd', 'reward'].map((name, i) => ({
          at: 30000 + i * 20000,
          do: 'page-eval',
          label: `sendCommand(${name})`,
          code: `document.querySelectorAll('owadview').forEach((el) => el.sendCommand('${name}')); 'sent'`,
        })),
      ],
    },
  },

  'reward-visibility-probe': {
    describe:
      'R3: a ready rewarded ad and slot visibility: the slot is hidden (display:none) for 1 s at 15 s, then for 8 s at 45 s, then pushed out of the viewport for 1 s at 80 s; does becoming visible again post userPlay and play the ad. Frame message hook on. App-side DOM changes only.',
    defaults: { mode: 'test', present: 'transparent', layout: '400x300', duration: 150 },
    config: {
      elementAttrs: [{ adstyle: 'rewarded-ad;' }],
      actions: [
        { at: 12000, do: 'hook-guest-frames', label: 'oam' },
        ...[
          [15000, 'none'],
          [16000, ''],
          [45000, 'none'],
          [53000, ''],
        ].map(([at, display]) => ({
          at,
          do: 'page-eval',
          label: `slot display '${display}'`,
          code: `document.querySelector('.slot').style.display = '${display}'; 'ok'`,
        })),
        { at: 60000, do: 'hook-guest-frames', label: 'oam-after-reload' },
        {
          at: 80000,
          do: 'page-eval',
          label: 'slot out of viewport',
          code: `document.body.style.paddingTop = '4000px'; 'ok'`,
        },
        {
          at: 81000,
          do: 'page-eval',
          label: 'slot back',
          code: `document.body.style.paddingTop = ''; 'ok'`,
        },
        { at: 100000, do: 'hook-guest-frames', label: 'oam-late' },
      ],
    },
  },

  'adstyle-probe': {
    describe:
      'R3: which element attribute values switch the ad library options (OAM `options` on the wire: rewarded, enableHighImpact, forceAdUnit, performanceAd). Eight 300x250 slots, one candidate each; read the options from netlog-requests.json (adformat-report.mjs).',
    defaults: {
      mode: 'test',
      present: 'transparent',
      layout: '300x250,300x250,300x250,300x250,300x250,300x250,300x250,300x250',
      duration: 45,
    },
    config: {
      elementAttrs: [
        { adstyle: 'high-impact-ad;' },
        { adstyle: 'rewarded;' },
        { adstyle: 'rewarded-ad;' },
        { adstyle: 'reward-ad;' },
        { adstyle: 'reward;' },
        { adstyle: 'rewarded: true;' },
        { unit: 'parity_unit' },
        { adstyle: 'rewarded-ads;' },
      ],
      actions: [],
    },
  },

  sizes: {
    describe:
      'R3: every documented standard container size (working-with-ads#list-of-ad-sizes) in one 1420x860 window laid out so every slot is inside the viewport (a slot below the fold only logs "<owadview> is not visible. waiting..."): 400x600, 160x600, 400x300, 300x250 / 970x90, 400x60 / 728x90.',
    defaults: {
      mode: 'test',
      present: 'transparent',
      layout: '400x600,160x600,400x300,300x250,970x90,400x60,728x90',
      duration: 120,
    },
    config: {
      window: { width: 1420, height: 860 },
      actions: [
        { at: 20000, do: 'probe-guests', label: 'sizes+20s' },
        { at: 60000, do: 'probe-guests', label: 'sizes+60s' },
      ],
    },
  },

  house: {
    describe:
      'R3: house-ad path: standard slots with ad demand unreachable (run with --offline --offline-allow "*.overwolf.com,overwolf.com" so only Overwolf hosts load). No fill from partners; what the slot shows and dispatches.',
    defaults: {
      mode: 'test',
      present: 'transparent',
      layout: '400x300,300x250,400x600',
      duration: 120,
      offline: true,
      'offline-allow': '*.overwolf.com,overwolf.com',
    },
    config: {
      actions: [
        { at: 20000, do: 'probe-guests', label: 'house+20s' },
        { at: 70000, do: 'probe-guests', label: 'house+70s' },
      ],
    },
  },

  // --- Lab checks (AD-FORMATS-SPEC section 7; test mode; no input into ads) ---
  'lab-layers': {
    describe:
      'L1-L4, L10: a ready (unplayed) 400x300 reward slot and a 300x250 standard slot, both over a red container background, an app button at (900, 600), and at 5 s a performance ad with adstyle "background-color: rgba(0, 0, 0, 0.8); background-blur: 3;". Hit probes (page hit test; Tauri also the native hit test and per-webview snapshots) before, while the interstitial loads (plus one test-mode click into the app button, refused unless it routes to the app), after display_ad_loaded, and after a standard slot is remounted under the overlay.',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 45 },
    config: {
      window: { width: 1200, height: 800 },
      elementSpec: [
        {
          layout: '400x300',
          cid: 'parity_lab_reward',
          attrs: { adstyle: 'rewarded-ad;' },
          containerBackground: 'rgb(255, 0, 0)',
        },
        { layout: '300x250', cid: 'parity_lab_std', containerBackground: 'rgb(255, 0, 0)' },
        { control: { id: 'parity-control', x: 900, y: 600, width: 160, height: 60 } },
        { at: 5000, ...PERF_LAB },
      ],
      actions: [
        hitProbe(1000, 'early', { snapshot: true }),
        hitProbe(4000, 'before-perf', { snapshot: true }),
        hitProbe(5600, 'perf-loading', { snapshot: true, click: 'control' }),
        hitProbe(16000, 'perf-loaded', { snapshot: true, click: 'control' }),
        {
          at: 18000,
          do: 'page-eval',
          label: 'remount the standard slot',
          code: `(() => { const s = document.querySelector('owadview[cid="parity_lab_std"]').parentElement; s.remove(); window.__parityAddAd({ layout: '300x250', cid: 'parity_lab_std2', containerBackground: 'rgb(255, 0, 0)' }); return 'ok'; })()`,
        },
        hitProbe(26000, 'after-remount', { snapshot: true }),
        { at: 27000, do: 'probe-guests', label: 'lab+27s' },
      ],
    },
  },

  'reward-optin': {
    describe:
      'L7: four ready rewarded 400x300 slots; the app hides one slot at a time (display:none) for 1 frame, 50 ms, 500 ms and 2 s, 20 s apart, and shows it again. Which hides make the ad play (userPlay, play, impression)? Frame message hook on. App-side DOM changes only.',
    defaults: {
      mode: 'test',
      present: 'transparent',
      layout: '400x300,400x300,400x300,400x300',
      duration: 115,
    },
    config: {
      window: { width: 1000, height: 760 },
      elementAttrs: [0, 1, 2, 3].map(() => ({ adstyle: 'rewarded-ad;' })),
      actions: [
        { at: 12000, do: 'hook-guest-frames', label: 'oam' },
        ...[
          ['1 frame', 'raf'],
          ['50 ms', 50],
          ['500 ms', 500],
          ['2 s', 2000],
        ].flatMap(([name, hide], i) => [
          {
            at: 20000 + i * 20000,
            do: 'page-eval',
            label: `slot ${i} hidden for ${name}`,
            code: `(() => { const s = document.querySelectorAll('.slot')[${i}]; s.style.display = 'none'; const show = () => { s.style.display = ''; }; ${
              hide === 'raf'
                ? 'requestAnimationFrame(() => requestAnimationFrame(show));'
                : `setTimeout(show, ${hide});`
            } return 'ok'; })()`,
          },
          { at: 24000 + i * 20000, do: 'hook-guest-frames', label: `oam-after-${i}` },
        ]),
        { at: 100000, do: 'probe-guests', label: 'optin-end' },
      ],
    },
  },

  'perf-minimize': {
    describe:
      'L8: performance ad (docs example) at 3 s; the app window is minimized at 20 s and restored at 26 s (alpha-0 window). Element events (performance_ad_dismiss?), guest state and the host signals around it.',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 50 },
    config: {
      window: { width: 1200, height: 800 },
      elementSpec: [{ at: 3000, ...PERF_DOC }],
      actions: [
        { at: 15000, do: 'probe-guests', label: 'perf+12s' },
        { at: 20000, do: 'window', method: 'minimize' },
        { at: 26000, do: 'window', method: 'restore' },
        { at: 32000, do: 'probe-guests', label: 'restored+6s' },
      ],
    },
  },

  'standard-remove': {
    describe:
      'L6, L9: a 300x250 standard slot removed by the app at 20 s, a new one added at 25 s and that one moved to another container at 35 s (detach + attach). Element events (destroyed?), guest close and the removal timings.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 50 },
    config: {
      actions: [
        { at: 15000, do: 'probe-guests', label: 'std+15s' },
        {
          at: 20000,
          do: 'page-eval',
          label: 'app removes the standard ad',
          code: `document.querySelector('owadview').remove(); 'ok'`,
        },
        {
          at: 25000,
          do: 'page-eval',
          label: 'app adds a new standard ad',
          code: `window.__parityAddAd({ layout: '300x250', cid: 'parity_std_again' }); 'ok'`,
        },
        {
          at: 35000,
          do: 'page-eval',
          label: 'app moves the new ad to another container',
          code: `(() => { const ad = document.querySelector('owadview[cid="parity_std_again"]'); const box = document.createElement('div'); box.className = 'slot'; box.style.width = '300px'; box.style.height = '250px'; document.body.appendChild(box); box.appendChild(ad); return 'ok'; })()`,
        },
        { at: 42000, do: 'probe-guests', label: 'moved+7s' },
      ],
    },
  },

  audio: {
    describe:
      "L5: a 400x300 standard slot and a 400x300 rewarded slot; the app calls setAudioMuted(false) on both at 10 s, opts in to the reward at 15 s, and calls setAudioMuted(true) at 45 s. The mute calls each host makes on each guest (ipc.jsonl), and each guest's mute state read back at 5, 12 and 47 s (hit probes without points; ow-tauri reads it natively on Windows).",
    defaults: { mode: 'test', present: 'transparent', layout: '400x300,400x300', duration: 60 },
    config: {
      elementAttrs: [{}, { adstyle: 'rewarded-ad;' }],
      actions: [
        { at: 5000, do: 'hit-probe', label: 'mute-initial', points: [] },
        muteAll(10000, false),
        { at: 12000, do: 'hit-probe', label: 'mute-after-unmute', points: [] },
        {
          at: 15000,
          do: 'page-eval',
          label: 'reward slot hidden',
          code: `document.querySelectorAll('.slot')[1].style.display = 'none'; 'ok'`,
        },
        {
          at: 16000,
          do: 'page-eval',
          label: 'reward slot shown (opt-in)',
          code: `document.querySelectorAll('.slot')[1].style.display = ''; 'ok'`,
        },
        { at: 30000, do: 'probe-guests', label: 'audio+30s' },
        muteAll(45000, true),
        { at: 47000, do: 'hit-probe', label: 'mute-after-mute', points: [] },
      ],
    },
  },

  owadtestad: {
    describe:
      'L11: sets localStorage.owAdTestAd = "true" in the ad guest origin at 10 s (persistent profile: --home profile:<name>), reloads the slot, and records the ad library options. A later --mode live run on the same profile shows whether the guest switches to test ads.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: {
      actions: [
        {
          at: 10000,
          do: 'guest-eval',
          label: 'owAdTestAd before',
          code: `({ origin: location.origin, value: localStorage.getItem('owAdTestAd') })`,
        },
        {
          at: 11000,
          do: 'guest-eval',
          label: 'set owAdTestAd',
          code: `(localStorage.setItem('owAdTestAd', 'true'), { origin: location.origin, value: localStorage.getItem('owAdTestAd') })`,
        },
        {
          at: 12000,
          do: 'page-eval',
          label: 'reload the slot',
          code: `document.querySelector('owadview').reload(); 'ok'`,
        },
        { at: 25000, do: 'probe-guests', label: 'testad+25s' },
      ],
    },
  },

  'ipc-probe': {
    describe:
      "Guest IPC diagnosis: at 8 s each ad guest posts one request to the host's IPC endpoint (ow-tauri: the invoke URL; ow-electron has none), sends one adview_event through Tauri's invoke and queries the local network permission states; at 14 s the outcome is read back (guest-eval). Shows whether a remote guest page can reach the host on this platform.",
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 20 },
    config: {
      actions: [
        {
          at: 8000,
          do: 'guest-eval',
          label: 'ipc probe start',
          code: `(() => {
  const w = window;
  const r = (w.__ipcProbe = {
    origin: location.origin,
    internals: typeof w.__TAURI_INTERNALS__,
    ipcPostMessage: typeof (w.ipc && w.ipc.postMessage),
    webviewPostMessage: typeof (w.chrome && w.chrome.webview && w.chrome.webview.postMessage),
  });
  const t0 = performance.now();
  try {
    r.url = w.__TAURI_INTERNALS__.convertFileSrc('plugin:overwolf|ipc_probe', 'ipc');
  } catch (e) {
    r.url = null;
    r.urlError = String(e);
  }
  if (r.url) {
    r.fetch = 'pending';
    fetch(r.url, { method: 'POST', body: '{}', headers: { 'Content-Type': 'application/json' } }).then(
      (res) => { r.fetch = { status: res.status, tauriResponse: res.headers.get('Tauri-Response'), ms: Math.round(performance.now() - t0) }; },
      (e) => { r.fetch = { error: String(e), ms: Math.round(performance.now() - t0) }; },
    );
    r.invoke = 'pending';
    Promise.resolve()
      .then(() => w.__TAURI_INTERNALS__.invoke('plugin:overwolf|adview_event', { name: 'ipc-probe', data: null }))
      .then(
        (v) => { r.invoke = { ok: true, value: v ?? null, ms: Math.round(performance.now() - t0) }; },
        (e) => { r.invoke = { error: typeof e === 'string' ? e : JSON.stringify(e) ?? String(e), ms: Math.round(performance.now() - t0) }; },
      );
  }
  for (const name of ['local-network-access', 'loopback-network', 'local-network']) {
    try {
      navigator.permissions.query({ name }).then(
        (s) => { r[name] = s.state; },
        (e) => { r[name] = 'unsupported: ' + e.name; },
      );
    } catch (e) {
      r[name] = 'throws: ' + e.name;
    }
  }
  return { started: true };
})()`,
        },
        {
          at: 14000,
          do: 'guest-eval',
          label: 'ipc probe result',
          code: `window.__ipcProbe ?? null`,
        },
        { at: 15000, do: 'probe-guests', label: 'ipc-probe+15s' },
      ],
    },
  },

  'owadtestad-live': {
    describe:
      'L11 (live, 1 load): a 300x250 slot on a profile where owAdTestAd was set (run owadtestad first with the same --home profile:<name>); reads the stored value and the ad library options. Needs --mode live --live-ok --max-live-loads 1.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: {
      actions: [
        {
          at: 8000,
          do: 'guest-eval',
          label: 'owAdTestAd stored',
          code: `({ origin: location.origin, value: localStorage.getItem('owAdTestAd') })`,
        },
        { at: 20000, do: 'probe-guests', label: 'testad-live+20s' },
      ],
    },
  },

  'tower-plus': {
    describe:
      'Live preset: the Tower Plus layout page, a 400x600 and a 400x60 slot (Overwolf ad sizes). Run with --mode live --live-ok --max-live-loads 2.',
    defaults: { mode: 'test', present: 'transparent', layout: '400x600,400x60', duration: 60 },
    config: {
      window: { width: 900, height: 760 },
      actions: [{ at: 30000, do: 'probe-guests', label: 'tower+30s' }],
    },
  },

  'high-impact-only': {
    describe:
      'Live preset: the documented high-impact zone without the 400x60 container (one live load). Run with --mode live --live-ok --max-live-loads 1.',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 90 },
    config: {
      window: { width: 1000, height: 760 },
      elementSpec: [{ zone: 'high-impact', small: false }],
      actions: [
        { at: 15000, do: 'probe-guests', label: 'hi+15s' },
        { at: 60000, do: 'probe-guests', label: 'hi+60s' },
      ],
    },
  },

  'inview-probe': {
    describe:
      'In-view rule: 300x250 slots fixed 25 %, 50 % and 75 % inside the viewport at the bottom edge (vertical) and at the right edge (horizontal), one fully inside, and one sweep slot moved across the top edge (0 % to 100 % and back, 5 % steps every 1.5 s), then across the left edge. Which share in view makes the embedder report the guest visible, and which static slots fill.',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 150 },
    config: {
      window: { width: 1400, height: 900 },
      elementSpec: [
        ...[
          ['v25', 'left:60px;top:calc(100vh - 62.5px)'],
          ['v50', 'left:380px;top:calc(100vh - 125px)'],
          ['v75', 'left:700px;top:calc(100vh - 187.5px)'],
          ['h25', 'top:40px;left:calc(100vw - 75px)'],
          ['h50', 'top:300px;left:calc(100vw - 150px)'],
          ['h75', 'top:560px;left:calc(100vw - 225px)'],
          ['full', 'left:500px;top:300px'],
          ['sweep', 'left:100px;top:-250px'],
        ].map(([id, place]) => ({
          layout: '300x250',
          cid: `parity_inview_${id}`,
          slotId: `inview-${id}`,
          slotStyle: `position:fixed;margin:0;${place}`,
        })),
      ],
      actions: [
        ...inviewSweep(6000, 'top', -250, 2.5),
        {
          at: 69000,
          do: 'page-eval',
          label: 'sweep: to the left edge, 0 % in view',
          code: `(() => { const s = document.getElementById('inview-sweep').style; s.top = '300px'; s.left = '-300px'; return 'ok'; })()`,
        },
        ...inviewSweep(72000, 'left', -300, 3),
        { at: 30000, do: 'probe-guests', label: 'inview+30s' },
        { at: 140000, do: 'probe-guests', label: 'inview+140s' },
      ],
    },
  },

  'inview-fine': {
    describe:
      'In-view rule, fine: one 300x250 sweep slot moved across the top edge from 44 % to 52 % in view and back in 1 % steps every 1.5 s, then across the left edge the same way. The share at which the embedder reports the guest visible, and hidden again.',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 70 },
    config: {
      window: { width: 1400, height: 900 },
      elementSpec: [
        {
          layout: '300x250',
          cid: 'parity_inview_sweep',
          slotId: 'inview-sweep',
          slotStyle: 'position:fixed;margin:0;left:100px;top:-140px',
        },
      ],
      actions: [
        ...inviewSweep(5000, 'top', -250, 2.5, { lo: 44, hi: 52, pct: 1 }),
        {
          at: 31000,
          do: 'page-eval',
          label: 'sweep: to the left edge, 44 % in view',
          code: `(() => { const s = document.getElementById('inview-sweep').style; s.top = '300px'; s.left = '-168px'; return 'ok'; })()`,
        },
        ...inviewSweep(34000, 'left', -300, 3, { lo: 44, hi: 52, pct: 1 }),
      ],
    },
  },

  packages: {
    describe: 'R2-15: package manager surface with gep+overlay listed (offline).',
    defaults: {
      mode: 'test',
      present: 'hidden',
      duration: 10,
      packages: 'gep,overlay',
      offline: true,
    },
    config: {
      noWindow: true,
      actions: [
        { at: 100, do: 'pkg-call', fn: 'getChannel', args: ['gep'] },
        { at: 300, do: 'pkg-call', fn: 'setChannel', args: ['gep', 'beta'] },
        { at: 600, do: 'pkg-call', fn: 'getAvailableChannels', args: ['gep'] },
        { at: 900, do: 'pkg-call', fn: 'hasPendingUpdates', args: [] },
        { at: 1200, do: 'pkg-call', fn: 'relaunch', args: ['gep'] },
        { at: 1500, do: 'listeners', label: 'packages' },
      ],
    },
  },

  // --- W3 record-first observations (DESIGN-v2 §5.2 #12, #14, #17, #18) ------
  'email-hashes-clear': {
    describe:
      'SEC-M9: setUserEmailHashes with a hash set, then with undefined, no argument, null and {} (each after a fresh set). The state file after every call (state-file.jsonl), the eHashes messages each guest gets, a slot reloaded and a new slot added after the undefined call.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 62 },
    config: {
      actions: [
        { at: 8000, do: 'state-file', label: 'start' },
        { at: 10000, do: 'ow-call', fn: 'setUserEmailHashes', generateFrom: TEST_EMAIL },
        { at: 11000, do: 'state-file', label: 'after-set-1' },
        ...emailHashCall(13000, 'undefined', [{ $undefined: true }]),
        {
          at: 17000,
          do: 'page-eval',
          label: 'reload the first slot',
          code: `document.querySelector('owadview').reload(); 'ok'`,
        },
        {
          at: 19000,
          do: 'page-eval',
          label: 'add a slot after the undefined call',
          code: `window.__parityAddAd({ layout: '300x250', cid: 'parity_after_clear' }); 'ok'`,
        },
        { at: 27000, do: 'probe-guests', label: 'after-undefined-new-slot' },
        { at: 28000, do: 'state-file', label: 'after-undefined+15s' },
        { at: 30000, do: 'ow-call', fn: 'setUserEmailHashes', generateFrom: TEST_EMAIL },
        { at: 31000, do: 'state-file', label: 'after-set-2' },
        ...emailHashCall(33000, 'no argument', []),
        { at: 36000, do: 'ow-call', fn: 'setUserEmailHashes', generateFrom: TEST_EMAIL },
        { at: 37000, do: 'state-file', label: 'after-set-3' },
        ...emailHashCall(39000, 'null', [null]),
        { at: 42000, do: 'ow-call', fn: 'setUserEmailHashes', generateFrom: TEST_EMAIL },
        { at: 43000, do: 'state-file', label: 'after-set-4' },
        ...emailHashCall(45000, '{}', [{}]),
        { at: 48000, do: 'ow-call', fn: 'setUserEmailHashes', generateFrom: TEST_EMAIL },
        { at: 49000, do: 'state-file', label: 'after-set-5' },
        ...emailHashCall(51000, "''", ['']),
        { at: 56000, do: 'probe-guests', label: 'end-of-calls' },
      ],
    },
  },

  'last-window-during-consent': {
    describe:
      'PAR-M10, first launch: the app quits when its last window closes (window-all-closed -> app.quit()). Its only window is created at ready without waiting for isCMPRequired() and closes 450 ms later, while the startup consent window is open and its page has not saved yet. When the app exits, the windows still open at before-quit, whether the consent page sent its Counter, and the cmp bytes written.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: {
      quitOnAllClosed: true,
      skipStartupCalls: true,
      closeMainWindowAtMs: 450,
      actions: [],
    },
  },

  'last-window-before-consent': {
    describe:
      'PAR-M10, first launch: as last-window-during-consent, but the only window closes as soon as it is created, before the startup consent window exists.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: { quitOnAllClosed: true, skipStartupCalls: true, closeMainWindowAtMs: 0, actions: [] },
  },

  'last-window-consent-saved': {
    describe:
      'PAR-M10, first launch: the app waits for isCMPRequired() as usual, then closes its only window 200 ms after creating it: the consent page has saved, its window is still open.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: { quitOnAllClosed: true, closeMainWindowAtMs: 200, actions: [] },
  },

  'last-window-after-consent': {
    describe:
      'Control for last-window-during-consent: the same app closes its only window 10 s after creating it, after the startup consent has finished.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: { quitOnAllClosed: true, closeMainWindowAtMs: 10000, actions: [] },
  },

  'corrupt-state': {
    describe:
      'V13/RK15: a launch on a profile whose ow-electron.json run.mjs --corrupt-state replaced before launch (garbage, truncated, empty, array, null, wrong-types, missing). The state file at startup and later (state-file.jsonl), whether the startup consent window opens, isCMPRequired(), first-launch analytics and the consent messages guests get. Without --corrupt-state it seeds the profile.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 30 },
    config: {
      actions: [
        { at: 0, do: 'state-file', label: 'window-loaded' },
        { at: 2000, do: 'ow-call', fn: 'isCMPRequired', label: 'isCMPRequired+2s' },
        { at: 10000, do: 'state-file', label: '+10s' },
        { at: 12000, do: 'probe-guests', label: '+12s' },
        { at: 25000, do: 'state-file', label: '+25s' },
      ],
    },
  },

  'gesture-timing': {
    describe:
      'PAR-M11, SEC-M2: the first ad guest is switched to a loopback fixture page (never an ad; test mode only) that opens owparity-canary:// URLs (an unregistered scheme). A trusted click (sendInputEvent) in the fixture, then window.open after 0, 0.5, 2, 4, 5, 6 s; no click; a click in the app page instead; two opens from one click; a target=_blank link; Return on the focused button; a script top-level navigation 0, 0.25, 0.5, 1, 1.5, 2, 6 s after a click. Records what the host window-open handler and navigation events saw and answered (click-outs.jsonl). shell.openExternal is replaced by a recorder.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 165 },
    config: {
      gestureFixture: true,
      stubOpenExternal: true,
      actions: [
        { at: 6000, do: 'guest-fixture', label: 'fixture' },
        { at: 10000, do: 'gesture-case', id: 1, kind: 'open', delay: 0 },
        { at: 19000, do: 'gesture-case', id: 2, kind: 'open', delay: 500 },
        { at: 28000, do: 'gesture-case', id: 3, kind: 'open', delay: 2000 },
        { at: 37000, do: 'gesture-case', id: 4, kind: 'open', delay: 4000 },
        { at: 46000, do: 'gesture-case', id: 5, kind: 'open', delay: 6000 },
        { at: 56000, do: 'gesture-case', id: 6, kind: 'no-gesture' },
        { at: 61000, do: 'gesture-case', id: 7, kind: 'embedder-click' },
        { at: 67000, do: 'gesture-case', id: 8, kind: 'open-twice', delay: 0 },
        { at: 74000, do: 'gesture-case', id: 9, kind: 'anchor' },
        { at: 81000, do: 'gesture-case', id: 10, kind: 'open', delay: 0, input: 'key' },
        { at: 88000, do: 'gesture-case', id: 11, kind: 'open', delay: 5000 },
        { at: 98000, do: 'gesture-case', id: 12, kind: 'top', delay: 0 },
        { at: 106000, do: 'gesture-case', id: 13, kind: 'top', delay: 2000 },
        { at: 115000, do: 'gesture-case', id: 14, kind: 'top', delay: 6000 },
        { at: 124000, do: 'gesture-case', id: 15, kind: 'top', delay: 250 },
        { at: 131000, do: 'gesture-case', id: 16, kind: 'top', delay: 500 },
        { at: 138000, do: 'gesture-case', id: 17, kind: 'top', delay: 1000 },
        { at: 145000, do: 'gesture-case', id: 18, kind: 'top', delay: 1500 },
        { at: 155000, do: 'probe-guests', label: 'gesture-end' },
      ],
    },
  },

  // --- DESIGN §5.2 (the W3/W4 checks; SECTION_5_2 below indexes them) -----

  'title-default': {
    describe:
      '§5.2 #2, PAR-minor-2: the ad window has no configured title; its title, the window name the guests get and the analytics labels follow the app name.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 30 },
    config: {
      actions: [
        { at: 2000, do: 'window', method: 'getTitle' },
        { at: 4000, do: 'probe-guests', label: 'title' },
      ],
    },
  },

  'title-set-in-setup': {
    describe:
      '§5.2 #2, PAR-minor-2: the ad window has a configured title and the app retitles it right after creating it (setupTitle), before its page loads; which title wins.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 30 },
    config: {
      windowTitle: 'Configured Title',
      setupTitle: 'Set In Setup',
      actions: [
        { at: 2000, do: 'window', method: 'getTitle' },
        { at: 4000, do: 'probe-guests', label: 'title' },
      ],
    },
  },

  'custom-ua': {
    describe:
      '§5.2 #3, PAR-M3: the ad window has its own user agent (Custom/1.0). The host requests, the guest document requests and the consent window keep the host-shaped user agent.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: {
      windowUserAgent: 'Custom/1.0',
      actions: [
        { at: 1000, do: 'page-eval', label: 'page user agent', code: 'navigator.userAgent' },
        { at: 3000, do: 'cmp-open', fn: 'openAdPrivacySettingsWindow', label: 'consent window' },
        { at: 12000, do: 'cmp-close', label: 'consent window' },
        { at: 15000, do: 'probe-guests', label: 'ua' },
      ],
    },
  },

  'ready-burst': {
    describe:
      '§5.2 #4: a plain launch to time the launch burst: cmp-eu-only inside the 250 ms burst window, burst start at most 100 ms after Ready (compareHostRequests).',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 25 },
    config: { actions: [] },
  },

  'exit-last-window': {
    describe:
      '§5.2 #5: the app quits when its last window closes (15 s after creation, after consent): window_closed sent once, the process exits without a forced exit.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: { quitOnAllClosed: true, closeMainWindowAtMs: 15000, actions: [] },
  },

  'exit-app-exit': {
    describe:
      '§5.2 #5: the run ends with app.exit(0) while the ad window is open (Tauri AppHandle::exit, ow-electron app.exit).',
    defaults: {
      mode: 'test',
      present: 'transparent',
      layout: '300x250',
      duration: 30,
      quitStyle: 'exit',
    },
    config: { actions: [] },
  },

  'exit-terminate': {
    describe:
      '§5.2 #5 (macOS): the run ends with the app menu Quit, [NSApp terminate:], while the ad window is open (ow-electron: app.quit(), the same call).',
    defaults: {
      mode: 'test',
      present: 'transparent',
      layout: '300x250',
      duration: 30,
      quitStyle: 'terminate',
    },
    config: { actions: [] },
  },

  'exit-tray-alive': {
    describe:
      '§5.2 #5, V2: the app keeps running with no window, as a tray app (the last window closes at 10 s, the app prevents the exit) until the timed quit.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: {
      closeMainWindowAtMs: 10000,
      actions: [{ at: 25000, do: 'snapshot', label: 'no-window+15s' }],
    },
  },

  'close-to-tray': {
    describe:
      '§5.2 #6, PAR-B1: the app handles a close of its window by hiding it (Rust prevent_close + hide; ow-electron close + preventDefault + hide), then shows it again (inactive). window-hidden count and position, visibility spans and the guest requests.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 50 },
    config: {
      closeHandler: 'tray',
      actions: [
        { at: 12000, do: 'window', method: 'close' },
        { at: 24000, do: 'window', method: 'showInactive' },
        { at: 40000, do: 'probe-guests', label: 'after-show' },
      ],
    },
  },

  'close-to-tray-js': {
    describe:
      "§5.2 #6, PAR-B1: as close-to-tray, with the close handled by the window's page (Tauri onCloseRequested + preventDefault + hide; ow-electron: the same handler in its main process).",
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 50 },
    config: {
      closeHandler: 'tray-js',
      actions: [
        { at: 12000, do: 'window', method: 'close' },
        { at: 24000, do: 'window', method: 'showInactive' },
        { at: 40000, do: 'probe-guests', label: 'after-show' },
      ],
    },
  },

  'close-js-delayed': {
    describe:
      '§5.2 #7, PAR-M2: the close handler waits 500 ms, then destroys the window (visibility, messages, the guest requests around the close).',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: {
      closeHandler: 'delay-destroy',
      actions: [{ at: 12000, do: 'window', method: 'close' }],
    },
  },

  'close-confirm-5s': {
    describe:
      '§5.2 #7, PAR-M2: the close handler keeps the window (a confirm the user cancels) and shows it again 5 s later.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: {
      closeHandler: 'confirm-5s',
      actions: [
        { at: 12000, do: 'window', method: 'close' },
        { at: 30000, do: 'probe-guests', label: 'after-confirm' },
      ],
    },
  },

  'destroy-direct': {
    describe: '§5.2 #7, PAR-M2: the app destroys its window without a close request.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: { actions: [{ at: 12000, do: 'window', method: 'destroy' }] },
  },

  'heartbeat-silence': {
    describe:
      "§5.2 #11, R4, PAR-M5: the guest shim's heartbeat paused for 120 s on a live guest must recreate nothing; the request stream equals the control. Needs a plugin lab hook (heartbeat-pause); ow-electron has no shim heartbeat and runs as the control.",
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 180 },
    config: {
      actions: [
        { at: 15000, do: 'probe-guests', label: 'before-pause' },
        { at: 20000, do: 'heartbeat-pause', ms: 120000 },
        { at: 150000, do: 'probe-guests', label: 'after-pause' },
      ],
    },
  },

  'cmpreq-hang-long': {
    describe:
      '§5.2 #13: the cmp-eu-only feature request hangs 70 s: ow-tauri resolves isCMPRequired() true at 60 s (a listed deviation); up to 45 s both hosts match R2-cmpreq-hang. The ow-tauri run needs the loopback feature server (not in the Tauri harness yet).',
    defaults: {
      mode: 'test',
      present: 'transparent',
      layout: '300x250',
      duration: 100,
      features: 'hang-long',
    },
    config: {
      actions: [
        { at: 0, do: 'ow-call', fn: 'isCMPRequired', label: 'isCMPRequired at load' },
        { at: 75000, do: 'ow-call', fn: 'isCMPRequired', label: 'isCMPRequired after the hang' },
      ],
    },
  },

  'window-before-ready': {
    describe:
      '§5.2 #15, V8: an ow-tauri window created off the main thread and shown before the plugin registered it; the launch order (#5) is unchanged. ow-electron runs as usual (control). Not in the Tauri harness yet (windowBeforeReady).',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 30 },
    config: { windowBeforeReady: true, actions: [] },
  },

  'parked-show-unfocused': {
    describe:
      '§5.2 #16, PAR-M9: the ad window is created hidden (never shown at startup) and parked past consent; 12 s after load it is shown without focus (Tauri show on a non-focusable window, ow-electron showInactive), visible 5 s, hidden again. Launch order, the first visible heartbeat, timings and window_closed.',
    defaults: { mode: 'test', present: 'hidden', layout: '300x250', duration: 40 },
    config: {
      actions: [
        { at: 12000, do: 'window', method: 'showInactive' },
        { at: 17000, do: 'window', method: 'hide' },
        { at: 25000, do: 'probe-guests', label: 'after-park' },
      ],
    },
  },

  'email-hashes-golden': {
    describe:
      '§5.2 #17, D16, SEC-M9: setUserEmailHashes(generateUserEmailHashes(x)) for an ASCII, an upper-case, a padded and a non-ASCII address, then setUserEmailHashes(undefined): the eHashes messages and the state file after each.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 50 },
    config: {
      actions: [
        ...[
          ['ascii', TEST_EMAIL],
          ['upper', 'Test.Email@Overwolf.COM'],
          ['padded', `  ${TEST_EMAIL}  `],
          ['non-ascii', 'tëst.émail@overwolf.com'],
        ].flatMap(([name, email], i) => [
          {
            at: 8000 + i * 6000,
            do: 'ow-call',
            fn: 'setUserEmailHashes',
            generateFrom: email,
            label: name,
          },
          { at: 9000 + i * 6000, do: 'state-file', label: `after-${name}` },
          { at: 10000 + i * 6000, do: 'probe-guests', label: `after-${name}` },
        ]),
        ...emailHashCall(34000, 'undefined', [{ $undefined: true }]),
      ],
    },
  },

  'dialog-probe': {
    describe:
      "§5.2 #19, OQ-39: what alert(), confirm() and prompt() return in an ad guest, and how long they block. ow-tauri only: ow-electron shows a native dialog for a guest's alert(), which the invisible lab must not show; its answers come from the Windows CI lab (--ci-visible).",
    hosts: ['tauri'],
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 30 },
    config: {
      actions: [
        {
          at: 10000,
          do: 'guest-eval',
          label: 'dialogs',
          code: `(() => ['alert', 'confirm', 'prompt'].map((name) => {
            const t0 = performance.now();
            let value;
            try { value = window[name]('parity'); } catch (error) { value = 'threw ' + error; }
            return { name, value: value === undefined ? 'undefined' : value, ms: Math.round(performance.now() - t0) };
          }))()`,
        },
      ],
    },
  },

  'build-identity': {
    describe:
      '§5.2 #20, PAR-M1, PAR-B2 (Windows CI lab): a build whose version is overridden (tauri build --config) installs and uninstalls with the install-record and uninstall Counter values equal to getInfo() and the launch Counter; the signing uid from a mocked /sign/electron. Needs the installer, so Windows CI only.',
    windowsOnly: true,
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 30 },
    config: { actions: [{ at: 2000, do: 'ow-call', fn: 'getInfo', label: 'getInfo' }] },
  },

  'no-analytics-config': {
    describe:
      '§5.2 #21, PAR-M6, R10: anonymous analytics off from the configuration (plugins.overwolf.analytics.disableAnonymous); ow-electron: disableAnonymousAnalytics() at module load.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 30 },
    config: {
      // ow-electron has no configuration switch: its twin calls
      // disableAnonymousAnalytics() at module load.
      disableAnalytics: 'electron-only',
      overwolfConfig: { analytics: { disableAnonymous: true } },
      actions: [],
    },
  },

  'no-analytics-setup': {
    describe:
      '§5.2 #21: anonymous analytics off from the app setup (Rust disableAnonymousAnalytics before Ready); ow-electron at module load.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 30 },
    config: { disableAnalytics: true, actions: [] },
  },

  'no-analytics-persisted': {
    describe:
      '§5.2 #21: the preference persisted by an earlier launch (setAnonymousAnalyticsPreference(false) in the first run) on the second launch. Run twice with the same --home profile:NAME; the first run sets it.',
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 30 },
    config: {
      actions: [
        { at: 2000, do: 'ow-call', fn: 'setAnonymousAnalyticsPreference', args: [false] },
        { at: 3000, do: 'state-file', label: 'after-preference' },
      ],
    },
  },

  'local-frame': {
    describe:
      "§5.2 #22, SEC-B1: an ad guest adds frames to the app's own origins (tauri://localhost, http://tauri.localhost, http://asset.localhost, a dev server on localhost, the app's file:// page) and probes the core IPC; none may load and no command may answer (core and app commands through the guest's IPC). The Windows half (http://tauri.localhost) runs in the Windows CI lab.",
    defaults: { mode: 'test', present: 'transparent', layout: '300x250', duration: 40 },
    config: {
      actions: [
        {
          at: 10000,
          do: 'guest-eval',
          label: 'add local frames',
          code: `(() => {
            const urls = ['tauri://localhost/', 'http://tauri.localhost/', 'http://asset.localhost/x', 'http://localhost:1420/', 'file:///'];
            window.__parityLocal = urls.map((url) => {
              const frame = document.createElement('iframe');
              const entry = { url, loads: 0 };
              frame.onload = () => { entry.loads += 1; };
              frame.style.cssText = 'position:absolute;width:1px;height:1px;opacity:0;pointer-events:none';
              frame.src = url;
              document.body.appendChild(frame);
              entry.frame = frame;
              return entry;
            });
            return { ipc: typeof window.__TAURI_INTERNALS__, ipcPost: typeof window.ipc };
          })()`,
        },
        {
          at: 16000,
          do: 'guest-eval',
          label: 'local frames',
          code: `(() => (window.__parityLocal || []).map(({ url, loads, frame }) => {
            let href;
            try { href = frame.contentWindow.location.href; } catch (error) { href = 'cross-origin'; }
            let ipc;
            try { ipc = typeof frame.contentWindow.__TAURI_INTERNALS__; } catch (error) { ipc = 'cross-origin'; }
            frame.remove();
            return { url, loads, href, ipc };
          }))()`,
        },
        {
          at: 20000,
          do: 'guest-eval',
          label: 'core commands',
          code: `(() => {
            const ipc = window.__TAURI_INTERNALS__;
            if (!ipc || typeof ipc.invoke !== 'function') return 'no ipc';
            window.__parityCore = {};
            for (const cmd of ['plugin:app|version', 'plugin:window|get_all_windows', 'plugin:event|emit', 'plugin:webview|get_all_webviews', 'harness_config']) {
              Promise.resolve()
                .then(() => ipc.invoke(cmd, cmd === 'plugin:event|emit' ? { event: 'parity', payload: null } : {}))
                .then(() => { window.__parityCore[cmd] = 'answered'; }, (error) => { window.__parityCore[cmd] = 'refused: ' + String(error).slice(0, 120); });
            }
            return 'sent';
          })()`,
        },
        {
          at: 24000,
          do: 'guest-eval',
          label: 'core command answers',
          code: `window.__parityCore || null`,
        },
      ],
    },
  },

  long: {
    describe:
      'R2-9: a long hidden session (no ads, window never shown) to watch the periodic heartbeat. Needs --allow-long.',
    defaults: { mode: 'test', present: 'hidden', layout: 'none', duration: 46800 },
    config: { tickMs: 600000, actions: [] },
  },
};

// §5.2 #8: messages with the plugin's macOS recreate-on-reload on and off.
SCENARIOS['recreate-reload'] = {
  describe:
    "§5.2 #8 (macOS): messages with plugins.overwolf.ads.recreateOnReload on (the default): element events, messages, the 400025 count and the guest's request stream equal ow-electron's.",
  defaults: SCENARIOS.messages.defaults,
  config: { ...SCENARIOS.messages.config, overwolfConfig: { ads: { recreateOnReload: true } } },
};
SCENARIOS['recreate-reload-off'] = {
  describe:
    '§5.2 #8 (macOS): the control of recreate-reload: the guest reloads in place (recreateOnReload false).',
  defaults: SCENARIOS.messages.defaults,
  config: { ...SCENARIOS.messages.config, overwolfConfig: { ads: { recreateOnReload: false } } },
};

// §5.2 #10: crash with the macOS crash hook left unwired.
SCENARIOS['crash-fallback'] = {
  describe:
    '§5.2 #10 (macOS): crash with on_web_content_process_terminate left unwired: the plugin finds the dead guest by probing and recovers it, and sends no 400024. ow-electron runs crash (control).',
  defaults: SCENARIOS.crash.defaults,
  config: { ...SCENARIOS.crash.config, crashHook: false },
};

/**
 * DESIGN §5.2: each check, the scenarios that run it and where.
 *
 * - `lab`: `both` (the macOS invisible lab and the Windows CI lab),
 *   `macos`, or `windows-ci` (Windows only: `ci/windows-lab.mjs`);
 * - `missing`: what the harness still lacks for the ow-tauri run (the
 *   scenario is defined; that host records `action-unsupported` or runs the
 *   control).
 * @type {Array<{n: number, check: string, scenarios: string[], lab: 'both' | 'macos' | 'windows-ci', missing?: string}>}
 */
export const SECTION_5_2 = [
  { n: 1, check: 'window URLs and names', scenarios: ['windows-urls'], lab: 'both' },
  {
    n: 2,
    check: 'default and setup titles',
    scenarios: ['title-default', 'title-set-in-setup'],
    lab: 'both',
  },
  { n: 3, check: 'user agent, custom UA', scenarios: ['custom-ua'], lab: 'both' },
  { n: 4, check: 'launch burst at Ready', scenarios: ['ready-burst'], lab: 'both' },
  {
    n: 5,
    check: 'exit paths',
    scenarios: ['exit-last-window', 'exit-app-exit', 'exit-terminate', 'exit-tray-alive'],
    lab: 'both',
    missing: 'relaunch (tauri-plugin-process restart sentinel) is not in the harness app',
  },
  { n: 6, check: 'close to tray', scenarios: ['close-to-tray', 'close-to-tray-js'], lab: 'both' },
  {
    n: 7,
    check: 'delayed close, confirm, destroy',
    scenarios: ['close-js-delayed', 'close-confirm-5s', 'destroy-direct'],
    lab: 'both',
  },
  {
    n: 8,
    check: 'recreate on reload',
    scenarios: ['recreate-reload', 'recreate-reload-off'],
    lab: 'macos',
  },
  { n: 9, check: 'guest crash (hook wired)', scenarios: ['crash'], lab: 'both' },
  { n: 10, check: 'guest crash (hook unwired)', scenarios: ['crash-fallback'], lab: 'macos' },
  {
    n: 11,
    check: 'heartbeat silence',
    scenarios: ['heartbeat-silence'],
    lab: 'both',
    missing: 'a plugin lab hook to pause the shim heartbeat (change request)',
  },
  {
    n: 12,
    check: 'gestures',
    scenarios: ['gesture-timing'],
    lab: 'both',
    missing: 'a plugin lab hook to load the loopback fixture in a guest (change request)',
  },
  {
    n: 13,
    check: 'consent request hang > 60 s',
    scenarios: ['cmpreq-hang-long'],
    lab: 'both',
    missing: 'the loopback feature server for ow-tauri (Builder::endpoints)',
  },
  {
    n: 14,
    check: 'last window during consent',
    scenarios: ['last-window-during-consent'],
    lab: 'both',
  },
  {
    n: 15,
    check: 'window before Ready',
    scenarios: ['window-before-ready'],
    lab: 'both',
    missing: 'windowBeforeReady in the Tauri harness app',
  },
  { n: 16, check: 'parked, shown unfocused', scenarios: ['parked-show-unfocused'], lab: 'both' },
  {
    n: 17,
    check: 'email hash golden values',
    scenarios: ['email-hashes-golden', 'email-hashes-clear'],
    lab: 'both',
  },
  { n: 18, check: 'corrupt state file', scenarios: ['corrupt-state'], lab: 'both' },
  { n: 19, check: 'guest dialogs', scenarios: ['dialog-probe'], lab: 'both' },
  {
    n: 20,
    check: 'build identity',
    scenarios: ['build-identity'],
    lab: 'windows-ci',
    missing: 'the build-override, install and uninstall steps in ci/windows-lab.mjs',
  },
  {
    n: 21,
    check: 'no analytics three ways',
    scenarios: ['no-analytics-config', 'no-analytics-setup', 'no-analytics-persisted'],
    lab: 'both',
  },
  { n: 22, check: 'local frames', scenarios: ['local-frame'], lab: 'both' },
  {
    n: 23,
    check: 'single instance, second launch',
    scenarios: [],
    lab: 'both',
    missing: 'tauri-plugin-single-instance in the harness app and a second-launch runner step',
  },
];
