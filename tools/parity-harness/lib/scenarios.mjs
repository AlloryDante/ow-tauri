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
      'R3: performance ad (docs example) in a 900x500 window, below the documented 1000x600 minimum.',
    defaults: { mode: 'test', present: 'transparent', layout: 'none', duration: 90 },
    config: {
      window: { width: 900, height: 500 },
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

  long: {
    describe:
      'R2-9: a long hidden session (no ads, window never shown) to watch the periodic heartbeat. Needs --allow-long.',
    defaults: { mode: 'test', present: 'hidden', layout: 'none', duration: 46800 },
    config: { tickMs: 600000, actions: [] },
  },
};
