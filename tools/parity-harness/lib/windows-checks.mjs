// Windows lab checks (AD-FORMATS-SPEC section 7) from one ow-electron run
// and one ow-tauri run of the same scenario on the same Windows runner:
//
//   L1-W  transparency: the composed ow-tauri window (PrintWindow) shows the
//         app's own colour where a guest has no content, as ow-electron's
//         window does (lab-layers: the red container under the ready
//         reward slot; the interstitial's dim over the app control).
//   L2    z-order: the performance guest's container is the top child
//         window once it is mounted, and stays on top after a standard slot
//         is remounted (lab-layers).
//   L3-W  pass-through: while the interstitial loads its container has an
//         empty window region and a click at the app control reaches the
//         app (one SendInput click, test mode); once the modal has loaded
//         (performance_ad_loaded; the probe at 16 s) the region is gone
//         (NULL) and the click is refused because it would reach the ad
//         (lab-layers).
//   L5    mute: each guest's mute state read back natively (WebView2
//         IsMuted) equals ow-electron's isAudioMuted() at the same moments
//         (audio).
//   G1    window frame: the app window's getBounds() when its page has
//         loaded equals ow-electron's (CONTRACT B.2.2: width / height size
//         the frame, the frame is clamped to the work area, then x / y
//         apply). Every scenario.
//   G2    content area: getContentBounds() at the same moment equals
//         ow-electron's. Advisory (never fails the job): the two hosts'
//         native frames may differ in thickness; the detail shows both.
//
// Each check returns {id, scenario, pass, detail}. `pass` is null when the
// run has nothing to judge (for example no display_ad_loaded before the
// probe).

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import {
  colourClass,
  compositeAt,
  embedderLabels,
  guestMuted,
  webviewOwner,
} from './adformat-report.mjs';

function readJsonl(file) {
  if (!existsSync(file)) return [];
  return readFileSync(file, 'utf8')
    .split('\n')
    .filter(Boolean)
    .flatMap((line) => {
      try {
        return [JSON.parse(line)];
      } catch {
        return [];
      }
    });
}

/** Hit probes of a run by label. */
export function probesOf(runDir) {
  const out = {};
  for (const e of readJsonl(join(runDir, 'events.jsonl'))) {
    if (e.kind === 'hit-probe') out[e.label] = e;
  }
  return out;
}

/** The labels of the run's performance guests (those mounted pass-through). */
export function performanceGuests(runDir) {
  return [
    ...new Set(
      readJsonl(join(runDir, 'wc-events.jsonl'))
        .filter((e) => e.kind === 'passthrough' && e.on === true)
        .map((e) => e.label),
    ),
  ];
}

/** The top labelled child window in a native order (bottom to top). */
export function topLabel(probe) {
  const order = (probe?.native?.order ?? []).filter((o) => o.label && !o.hidden);
  return order.length ? order[order.length - 1].label : null;
}

/** Colour class at `point` in a probe (composited window). */
export function colourAt(probe, point) {
  const composite = probe ? compositeAt(probe) : null;
  return composite ? colourClass(composite[point]) : null;
}

/** The label a native hit test at `point` reached. */
function nativeHit(probe, point) {
  return probe?.native?.hits?.find((h) => h.name === point)?.target?.label ?? null;
}

/**
 * L1-W, L2, L3-W for a lab-layers pair.
 * @param {string} electronDir
 * @param {string} tauriDir
 */
export function labLayersChecks(electronDir, tauriDir) {
  const e = probesOf(electronDir);
  const t = probesOf(tauriDir);
  const perf = performanceGuests(tauriDir);
  const embedders = embedderLabels(readJsonl(join(tauriDir, 'wc-events.jsonl')));
  const checks = [];

  // L1-W: the ready reward slot is transparent over its red container,
  // before the interstitial; the app control under the interstitial's dim.
  for (const [label, point] of [
    ['early', 'reward-slot'],
    ['before-perf', 'reward-slot'],
    ['perf-loaded', 'control'],
  ]) {
    const want = colourAt(e[label], point);
    const got = colourAt(t[label], point);
    const screen = t[label]?.native?.composite?.screen?.samples?.find((s) => s.name === point);
    checks.push({
      id: 'L1-W',
      scenario: 'lab-layers',
      probe: `${label} ${point}`,
      pass: want && got ? want === got : null,
      detail: {
        electron: want,
        tauri: got,
        tauriScreen: screen ? colourClass(screen.rgba) : null,
        guestBackgrounds: Object.fromEntries(
          Object.entries(t[label]?.native?.webviews ?? {})
            .filter(([l]) => webviewOwner(l, embedders) === 'ad')
            .map(([l, f]) => [l, f.backgroundArgb ?? null]),
        ),
      },
    });
  }

  // L2: the performance guest is the top child window.
  for (const label of ['perf-loading', 'perf-loaded', 'after-remount']) {
    const top = topLabel(t[label]);
    checks.push({
      id: 'L2',
      scenario: 'lab-layers',
      probe: label,
      pass: perf.length && t[label] ? perf.includes(top) : null,
      detail: { performanceGuests: perf, top, order: t[label]?.native?.order ?? null },
    });
  }

  // L3-W: empty region and a delivered click while loading; no region and
  // a refused click once the modal has loaded (performance_ad_loaded).
  const region = (label) =>
    perf.map((g) => t[label]?.native?.webviews?.[g]?.region?.kind ?? null).find(Boolean) ?? null;
  const loading = t['perf-loading'];
  const loaded = t['perf-loaded'];
  const clicks = readJsonl(join(tauriDir, 'page-events.jsonl')).filter(
    (p) => p.kind === 'app-click',
  ).length;
  checks.push({
    id: 'L3-W',
    scenario: 'lab-layers',
    probe: 'perf-loading',
    pass: loading
      ? region('perf-loading') === 'empty' &&
        webviewOwner(nativeHit(loading, 'control'), embedders) === 'app' &&
        loading.native?.click?.sent === true &&
        clicks >= 1
      : null,
    detail: {
      region: region('perf-loading'),
      hit: nativeHit(loading, 'control'),
      click: loading?.native?.click ?? null,
      appClicksReceived: clicks,
    },
  });
  const adLoaded = readJsonl(join(tauriDir, 'wc-events.jsonl')).some(
    (x) => x.kind === 'passthrough' && x.on === false,
  );
  checks.push({
    id: 'L3-W',
    scenario: 'lab-layers',
    probe: 'perf-loaded',
    pass:
      loaded && adLoaded
        ? region('perf-loaded') === 'none' &&
          perf.includes(nativeHit(loaded, 'control')) &&
          loaded.native?.click?.sent === false
        : null,
    detail: {
      displayAdLoaded: adLoaded,
      region: region('perf-loaded'),
      hit: nativeHit(loaded, 'control'),
      click: loaded?.native?.click ?? null,
    },
  });
  return checks;
}

/**
 * L5 for an audio pair: each guest's mute state per probe.
 * @param {string} electronDir
 * @param {string} tauriDir
 */
export function audioChecks(electronDir, tauriDir) {
  const e = probesOf(electronDir);
  const t = probesOf(tauriDir);
  const embedders = embedderLabels(readJsonl(join(tauriDir, 'wc-events.jsonl')));
  const tauriMuted = (p) => (p ? guestMuted({ ...p, host: 'tauri' }, embedders) : null);
  return ['mute-initial', 'mute-after-unmute', 'mute-after-mute'].map((label) => {
    const want = Array.isArray(e[label]?.guestMuted) ? [...e[label].guestMuted].sort() : null;
    const got = tauriMuted(t[label]);
    return {
      id: 'L5',
      scenario: 'audio',
      probe: label,
      pass: want && got ? JSON.stringify(want) === JSON.stringify(got) : null,
      detail: { electron: want, tauri: got },
    };
  });
}

/** The label of the Tauri harness app's ad window (`driver.rs`). */
export const APP_WINDOW_LABEL = 'main';

/**
 * The app window's `did-finish-load` record of a run. ow-tauri's records
 * name their window: the app window is the one labelled
 * {@link APP_WINDOW_LABEL}. ow-electron's records have no label: the app
 * window is the one that loaded the harness page's `index.html` from the
 * app's own origin (`file:`); ow-electron's own internal windows also load
 * an `index.html` (`owepm://index.html/`, a 32 x 31 window, after the
 * app's), which is not the app window.
 * @param {string} runDir
 */
export function appWindowLoaded(runDir) {
  const loads = readJsonl(join(runDir, 'windows.jsonl')).filter(
    (r) => r.kind === 'did-finish-load',
  );
  const labelled = loads.filter((r) => typeof r.label === 'string');
  const app = labelled.length
    ? labelled.filter((r) => r.label === APP_WINDOW_LABEL)
    : loads.filter((r) =>
        /^(file|tauri|https?):\/\/[^?#]*\/index\.html(?:[?#]|$)/.test(String(r.url ?? '')),
      );
  return app.length ? app[app.length - 1] : null;
}

const sameRect = (a, b) =>
  ['x', 'y', 'width', 'height'].every((k) => Math.round(a[k]) === Math.round(b[k]));

const isRect = (r) =>
  r !== null &&
  typeof r === 'object' &&
  ['x', 'y', 'width', 'height'].every((k) => typeof r[k] === 'number');

/**
 * G1 / G2 for any pair: the app window's frame and content area once its
 * page has loaded.
 * @param {string} electronDir
 * @param {string} tauriDir
 */
export function geometryChecks(electronDir, tauriDir) {
  const e = appWindowLoaded(electronDir);
  const t = appWindowLoaded(tauriDir);
  const check = (id, key, advisory) => {
    const want = isRect(e?.[key]) ? e[key] : null;
    const got = isRect(t?.[key]) ? t[key] : null;
    return {
      id,
      scenario: null,
      probe: key,
      pass: want && got ? sameRect(want, got) : null,
      ...(advisory ? { advisory: true } : {}),
      detail: { electron: want, tauri: got },
    };
  };
  return [check('G1', 'bounds', false), check('G2', 'contentBounds', true)];
}

/**
 * The Windows checks that apply to `scenario`.
 * @param {string} scenario
 * @param {string} electronDir
 * @param {string} tauriDir
 */
export function windowsChecks(scenario, electronDir, tauriDir) {
  const geometry = geometryChecks(electronDir, tauriDir).map((c) => ({ ...c, scenario }));
  if (scenario === 'lab-layers') return [...labLayersChecks(electronDir, tauriDir), ...geometry];
  if (scenario === 'audio') return [...audioChecks(electronDir, tauriDir), ...geometry];
  return geometry;
}
