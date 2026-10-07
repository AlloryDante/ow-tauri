#!/usr/bin/env node
// Ad-format timeline for one harness run (round 3): what the app set on each
// <owadview>, what the ad library made of it (the OAM `options` it requests),
// and, in time order, the element events, the guest -> host trigger events,
// the host -> guest messages, element removal, ad-format console lines and
// the ad-unit requests. Writes adformats.json and adformats.md into the run.
//
//   node lib/adformat-report.mjs captures/<run-id>

import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { argv } from 'node:process';

const readJsonl = (file) =>
  existsSync(file)
    ? readFileSync(file, 'utf8')
        .split('\n')
        .filter(Boolean)
        .map((line) => JSON.parse(line))
    : [];
const readJson = (file) => (existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : null);

// Webview plumbing the host forwards for every guest; not ad-format specific.
const PLUMBING =
  /^(did-frame-|load-commit|did-start-navigation|did-navigate|console-message|media-|did-stop-loading|did-start-loading|page-title|page-favicon|update-target-url|devtools-|-focus-change|will-frame-navigate|did-redirect-navigation|ipc-message|context-menu|found-in-page|dom-ready|did-attach|did-finish-load|did-fail-load|destroyed|render-process-gone|did-change-theme-color)/;

// Requests that tell which ad unit / creative / reward endpoint was used.
const AD_REQUEST =
  /oam\/releases\/.*options=|\/gampad\/ads|content\.overwolf\.com\/testing\/|adview\.html|owads(\.min)?\.js|reward|house|performance|vast|\.mp4|\.webm|ima3|imasdk|prebid|amazon-adsystem\.com\/e\/dtb\/bid|analyticsnew\.overwolf\.com\/analytics\/Counter\?Name=owads|InsertStats/i;

const CONSOLE_KEYWORDS =
  /reward|perform|interstitial|house|high.?impact|takeover|shutdown|complete|skip|dismiss|no.?fill|close|minimum size|too small|not visible|waiting|stop CB|staring CB|starting CB|grant/i;

function decodeOptions(url) {
  try {
    const u = new URL(url);
    const raw = u.searchParams.get('options');
    if (!raw) return null;
    return {
      options: JSON.parse(decodeURIComponent(raw)),
      containerId: u.searchParams.get('containerId'),
    };
  } catch {
    return null;
  }
}

function parseArgs(raw) {
  try {
    return JSON.parse(raw);
  } catch {
    return raw;
  }
}

/** Element lifecycle events whose first-occurrence order both hosts share. */
export const LIFECYCLE = [
  'did-attach',
  'dom-ready',
  'did-finish-load',
  'display_ad_loaded',
  'performance_ad_loaded',
  'high-impact-ad-loaded',
  'high-impact-ad-removed',
  'performance_ad_dismiss',
  'video_ad_ready',
  'destroyed',
];

/** The decoded OAM `options` of an ad library URL, or null. */
export function oamOptionsOf(url) {
  if (!/oam\/releases\//.test(url)) return null;
  return decodeOptions(url)?.options ?? null;
}

/** An element's key in both hosts' records: its cid, or `performance`. */
const elementKey = (cid) => cid ?? 'performance';

/** The DOM state of one element as a layout sample describes it. */
function domState(el) {
  if (!el) return null;
  const overlay = (el.children ?? []).find((c) => c.tag === 'div');
  return {
    connected: el.connected,
    display: el.style?.display ?? null,
    pointerEvents: el.style?.pointerEvents ?? null,
    inlineStyle: el.inlineStyle ?? null,
    overlay: overlay
      ? {
          position: overlay.style?.position,
          pointerEvents: overlay.style?.pointerEvents,
          zIndex: overlay.style?.zIndex,
        }
      : null,
  };
}

/** Coarse colour class of an RGBA sample (0-1), for cross-engine comparison. */
export function colourClass(rgba) {
  if (!Array.isArray(rgba)) return null;
  const [r, g, b, a] = rgba;
  if (a < 0.01) return 'clear';
  if (r > 0.7 && g < 0.4 && b < 0.4) return 'red';
  if (r < 0.3 && g < 0.3 && b < 0.3) return 'dark';
  if (r > 0.6 && g > 0.6 && b > 0.6) return 'light';
  return 'other';
}

/**
 * The colour a viewer sees at each probe point. ow-electron captures the
 * composited window; ow-tauri snapshots each webview, so the snapshots are
 * composited bottom to top in the native order (source-over).
 */
export function compositeAt(probe) {
  const out = {};
  const shots = probe.native?.snapshots ?? probe.snapshots;
  if (!shots) return null;
  if (probe.host !== 'tauri') {
    for (const s of shots.embedder?.samples ?? []) out[s.name] = s.rgba;
    return out;
  }
  const order = (probe.native?.order ?? []).map((o) => o.label);
  for (const point of probe.dom?.points ?? []) {
    let colour = null;
    for (const label of order) {
      if (probe.native?.webviews?.[label]?.hidden) continue;
      const sample = shots[label]?.samples?.find((x) => x.name === point.name)?.rgba;
      if (!sample) continue;
      if (!colour) {
        colour = sample;
        continue;
      }
      const a = sample[3];
      colour = [0, 1, 2].map((i) => sample[i] * a + colour[i] * (1 - a)).concat([1]);
    }
    out[point.name] = colour && colour.map((v) => Math.round(v * 1000) / 1000);
  }
  return out;
}

/**
 * A copy of `value` with object keys sorted at every depth, so two option
 * sets compare by content and not by key order.
 * @param {unknown} value
 * @returns {unknown}
 */
export function sortKeys(value) {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value && typeof value === 'object')
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((k) => [k, sortKeys(value[k])]),
    );
  return value;
}

/**
 * Host-independent facts of one run for parity-diff (both hosts write the
 * same page records; the host traces are normalised here):
 * - elements: per element key, the lifecycle order, payload keys, removal
 *   and `destroyed` timings, and the DOM state before and after its first
 *   `display_ad_loaded`;
 * - oamOptions: per element key, the ad library option sets on the wire
 *   (ow-electron net log; ow-tauri guest resource lists);
 * - mute: each guest's sequence of mute states (L5);
 * - probes: lab hit probes (page and native routing, clicks, composited
 *   colour classes) by label (L1-L4);
 * - clicks: app control pointer and click records;
 * - bounds: ow-tauri native guest sizes against the element rects (L2);
 * - front: whether the app ever became the frontmost app.
 */
export function adformatFacts(runDir) {
  const pageEvents = readJsonl(join(runDir, 'page-events.jsonl'));
  const ipc = readJsonl(join(runDir, 'ipc.jsonl'));
  const events = readJsonl(join(runDir, 'events.jsonl'));
  const wcEvents = readJsonl(join(runDir, 'wc-events.jsonl'));
  const requests = readJson(join(runDir, 'netlog-requests.json')) ?? [];

  const elements = {};
  const element = (cid) => {
    const key = elementKey(cid);
    elements[key] ??= {
      key,
      created: [],
      order: [],
      counts: {},
      payloadKeys: {},
      removedAfter: null,
      destroyedAfter: null,
      removalOrder: [],
      firstLoadAt: null,
      modalAt: null,
    };
    return elements[key];
  };
  for (const e of pageEvents) {
    if (e.kind === 'owadview-created') element(e.cid).created.push(e.at);
  }
  for (const e of pageEvents) {
    if (e.kind !== 'owadview-event' && e.kind !== 'owadview-removed') continue;
    const el = element(e.cid);
    const since = e.at - (el.created[0] ?? 0);
    if (e.kind === 'owadview-removed') {
      el.removedAfter ??= since;
      el.removalOrder.push('removed');
      continue;
    }
    el.counts[e.event] = (el.counts[e.event] ?? 0) + 1;
    if (LIFECYCLE.includes(e.event) && !el.order.includes(e.event)) el.order.push(e.event);
    const detail = e.info?.detail;
    if (detail && typeof detail === 'object' && !Array.isArray(detail))
      el.payloadKeys[e.event] ??= Object.keys(detail).sort();
    if (e.event === 'destroyed') {
      el.destroyedAfter ??= since;
      el.removalOrder.push('destroyed');
    }
    if (e.event === 'display_ad_loaded') el.firstLoadAt ??= e.at;
    if (e.event === 'performance_ad_loaded') el.modalAt ??= e.at;
  }
  // DOM state before each element's first display_ad_loaded, and after it
  // settled: a performance element switches at its first
  // performance_ad_loaded [OBS], which follows display_ad_loaded within
  // ~100 ms, so its "after" is sampled after that event (a sample between
  // the two would only measure when the page happened to sample).
  const samples = pageEvents.filter((e) => e.kind === 'layout-sample');
  const find = (sample, key) =>
    sample.elements?.find((x) =>
      key === 'performance' ? x.attributes?.some(([n]) => n === 'performance') : x.cid === key,
    );
  for (const el of Object.values(elements)) {
    const at = el.firstLoadAt;
    const settled = el.modalAt ?? at;
    const before = samples.filter((s) => (at === null || s.at < at) && find(s, el.key)).at(-1);
    const after =
      settled === null
        ? null
        : (samples.find((s) => s.at >= settled + 40 && find(s, el.key)) ?? null);
    el.dom = {
      before: domState(before && find(before, el.key)),
      after: domState(after && find(after, el.key)),
    };
    el.removalOrder = [...new Set(el.removalOrder)];
  }

  // Ad library options on the wire.
  const urls = requests.map((r) => r.url);
  for (const f of existsSync(runDir) ? readdirSync(runDir) : []) {
    if (/^guest-\d+-.*\.json$/.test(f))
      urls.push(...(readJson(join(runDir, f))?.labResources ?? []));
  }
  // Per element: ow-tauri only sees the option sets of guests a probe
  // reached, so parity-diff compares the elements both hosts have evidence of.
  const oam = {};
  for (const url of urls) {
    const options = oamOptionsOf(url);
    if (options) {
      const { containerId, ...rest } = options;
      const key = elementKey(containerId || null);
      oam[key] ??= new Set();
      oam[key].add(JSON.stringify(sortKeys(rest)));
    }
  }

  // Mute states per guest, in the order the guests were first muted.
  const mute = new Map();
  for (const e of ipc) {
    let guest = null;
    let muted = null;
    if (e.via === 'webContents.setAudioMuted' && e.type === 'owadview') {
      guest = e.webContentsId;
      muted = Boolean(parseArgs(e.args)?.[0]);
    } else if (e.via === 'set-muted') {
      guest = e.label;
      muted = Boolean(e.muted);
    }
    if (guest === null) continue;
    const list = mute.get(guest) ?? [];
    if (list.at(-1) !== muted) list.push(muted);
    mute.set(guest, list);
  }

  // Lab hit probes.
  const probes = {};
  for (const e of events) {
    if (e.kind !== 'hit-probe') continue;
    const embedder = (label) => (label && /^bw-/.test(label) ? 'app' : label ? 'ad' : null);
    const composite = compositeAt(e);
    probes[e.label] = {
      points: Object.fromEntries(
        (e.dom?.points ?? []).map((p) => [
          p.name,
          {
            dom: p.target
              ? p.target.kind === 'ad' && p.target.performance
                ? 'ad-perf'
                : p.target.kind
              : null,
            native:
              e.host === 'tauri'
                ? embedder(e.native?.hits?.find((h) => h.name === p.name)?.target?.label)
                : undefined,
            colour: composite ? colourClass(composite[p.name]) : undefined,
          },
        ]),
      ),
      performance: e.dom?.performance ?? [],
      click: e.native?.click ?? e.click ?? null,
    };
  }
  const clicks = {
    pointer: pageEvents.filter((e) => e.kind === 'app-pointer').length,
    received: pageEvents.filter((e) => e.kind === 'app-click').length,
    sent: Object.values(probes).filter((p) => p.click?.sent).length,
  };

  // ow-tauri: native guest sizes (last bounds of each guest still open).
  const bounds = {};
  for (const e of wcEvents) {
    if (e.type !== 'owadview') continue;
    if (e.kind === 'bounds') bounds[e.label] = e.bounds.slice(2);
    if (e.kind === 'closed') delete bounds[e.label];
  }
  const front = readJsonl(join(runDir, 'front-monitor.jsonl')).find((e) => e.kind === 'end');
  return {
    elements,
    oamOptions: Object.fromEntries(
      Object.entries(oam)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([k, v]) => [k, [...v].sort()]),
    ),
    mute: [...mute.values()],
    probes,
    clicks,
    bounds: Object.values(bounds),
    front: front ? front.everFront : null,
  };
}

export function adformatReport(runDir) {
  const meta = readJson(join(runDir, 'meta.json'));
  const t0 = meta ? Date.parse(meta.startedAt) : 0;
  const pageEvents = readJsonl(join(runDir, 'page-events.jsonl'));
  const ipc = readJsonl(join(runDir, 'ipc.jsonl'));
  const consoleLines = readJsonl(join(runDir, 'console.jsonl'));
  const requests = readJson(join(runDir, 'netlog-requests.json')) ?? [];
  const actions = readJsonl(join(runDir, 'actions.jsonl'));
  const monitor = readJsonl(join(runDir, 'window-monitor.jsonl')).find((e) => e.kind === 'end');
  const liveLoads = readJsonl(join(runDir, 'live-loads.jsonl'));

  // What the app created and what ow-electron forwarded to the guest.
  const created = pageEvents.filter((e) => e.kind === 'owadview-created');
  const attach = ipc
    .filter(
      (e) =>
        e.channel === 'GUEST_VIEW_MANAGER_CREATE_AND_ATTACH_GUEST-adview' && e.dir === 'page->host',
    )
    .map((e) => ({ t: e.t, params: parseArgs(e.args)?.[0]?.[2] ?? null }));

  // The ad library's options as requested on the wire (one per guest load).
  const seen = new Set();
  const oamOptions = [];
  for (const r of requests) {
    if (!/oam\/releases\//.test(r.url)) continue;
    const decoded = decodeOptions(r.url);
    if (!decoded) continue;
    const key = JSON.stringify(decoded.options);
    if (seen.has(key)) continue;
    seen.add(key);
    oamOptions.push({
      atSeconds: r.startedAt ? Number(((Date.parse(r.startedAt) - t0) / 1000).toFixed(1)) : null,
      path: new URL(r.url).pathname,
      ...decoded,
    });
  }

  // Map guest webContents id -> element: each attach request is followed by
  // the host loading the ad page into that guest's webContents.
  const guestCid = {};
  let pendingAttach = [];
  for (const e of ipc) {
    if (
      e.channel === 'GUEST_VIEW_MANAGER_CREATE_AND_ATTACH_GUEST-adview' &&
      e.dir === 'page->host'
    ) {
      const params = parseArgs(e.args)?.[0]?.[2] ?? {};
      pendingAttach.push(
        params.cid ||
          (params.performance ? `performance#${params.instanceId}` : `#${params.instanceId}`),
      );
    } else if (e.via === 'webContents.loadURL' && e.type === 'owadview' && pendingAttach.length) {
      if (guestCid[e.webContentsId] === undefined)
        guestCid[e.webContentsId] = pendingAttach.shift();
    }
  }

  const timeline = [];
  for (const e of pageEvents) {
    if (e.kind === 'owadview-event' && !PLUMBING.test(e.event)) {
      timeline.push({
        t: e.t,
        src: 'element',
        what: e.event,
        cid: e.cid,
        detail: e.info?.detail ?? null,
      });
    } else if (
      e.kind === 'owadview-event' &&
      ['did-fail-load', 'render-process-gone'].includes(e.event)
    ) {
      const own = e.info?.own ?? {};
      if (own.isMainFrame !== false)
        timeline.push({ t: e.t, src: 'element', what: e.event, cid: e.cid, detail: own });
    } else if (e.kind === 'owadview-removed') {
      timeline.push({ t: e.t, src: 'element', what: 'REMOVED from DOM', cid: e.cid });
    } else if (e.kind === 'owadview-created') {
      timeline.push({ t: e.t, src: 'app', what: 'created', cid: e.cid, detail: e.spec });
    } else if (e.kind === 'hi-zone') {
      timeline.push({ t: e.t, src: 'app', what: `hi-zone ${e.action}` });
    } else if (e.kind === 'page-visibility') {
      timeline.push({ t: e.t, src: 'app', what: `page ${e.visibilityState}` });
    }
  }
  for (const e of ipc) {
    if (e.via !== 'webContents._sendInternal' && e.dir === 'host->page') continue; // frame duplicates
    const args = parseArgs(e.args);
    if (e.channel === 'GUEST_ADVIEW_TRIGGER_EVENT') {
      const [name, data] = args?.[0] ?? [];
      timeline.push({
        t: e.t,
        src: 'guest->host',
        what: `TRIGGER ${name}`,
        wc: e.webContentsId,
        cid: guestCid[e.webContentsId],
        detail: data,
      });
    } else if (
      e.dir === 'page->host' &&
      /^GUEST_ADVIEW_|GUEST_VIEW_EMBEDDER_VISIBILITY/.test(e.channel ?? '')
    ) {
      if (e.channel === 'GUEST_ADVIEW_APPLY_SETTINGS') continue;
      timeline.push({
        t: e.t,
        src: `${e.type}->host`,
        what: e.channel,
        wc: e.webContentsId,
        cid: guestCid[e.webContentsId],
        detail: args,
      });
    } else if (e.dir === 'host->page' && Array.isArray(args)) {
      const [channel, ...rest] = args;
      if (typeof channel !== 'string') continue;
      if (channel.startsWith('GUEST_VIEW_INTERNAL_DISPATCH_EVENT')) {
        if (!PLUMBING.test(rest[0]))
          timeline.push({
            t: e.t,
            src: 'host->element',
            what: `DISPATCH ${rest[0]}`,
            detail: rest[1],
          });
      } else if (/^GUEST_VIEW_PRIVATE_MESSAGE|^GUEST_INSTANCE_/.test(channel)) {
        if (rest[0]?.type === 'consent') continue;
        timeline.push({
          t: e.t,
          src: 'host->guest',
          what: channel,
          wc: e.webContentsId,
          cid: guestCid[e.webContentsId],
          detail: rest,
        });
      }
    } else if (
      e.dir === 'host->page' &&
      e.type === 'owadview' &&
      /reload|loadURL|setAudioMuted/.test(e.via)
    ) {
      if (e.via === 'webContents.setAudioMuted') continue;
      timeline.push({
        t: e.t,
        src: 'host->guest',
        what: e.via,
        wc: e.webContentsId,
        cid: guestCid[e.webContentsId],
      });
    }
  }
  for (const c of consoleLines) {
    if (c.type !== 'owadview' || !CONSOLE_KEYWORDS.test(c.message)) continue;
    if (/Electron Security Warning/.test(c.message)) continue;
    timeline.push({
      t: c.t,
      src: 'guest console',
      what: c.message.slice(0, 240),
      wc: c.webContentsId,
      cid: guestCid[c.webContentsId],
    });
  }
  for (const a of actions) {
    if (a.phase === 'start' && a.do !== 'probe-guests')
      timeline.push({
        t: a.t,
        src: 'action',
        what: `${a.do}${a.label ? ` (${a.label})` : ''}${a.method ? ` ${a.method}` : ''}`,
      });
  }
  const adRequests = requests
    .filter((r) => AD_REQUEST.test(r.url))
    .map((r) => {
      const at = r.startedAt ? Date.parse(r.startedAt) - t0 : null;
      let url = r.url;
      if (/options=/.test(url)) url = url.replace(/options=[^&]*/, 'options=<decoded above>');
      return {
        atMs: at,
        method: r.method,
        status: r.status,
        initiator: r.initiator,
        url: url.slice(0, 300),
      };
    });
  timeline.sort((a, b) => a.t - b.t);

  const result = {
    runDir,
    scenario: meta?.options?.scenario ?? null,
    mode: meta?.options?.mode ?? null,
    everVisible: monitor ? monitor.everVisible : null,
    liveLoads: liveLoads.length,
    created: created.map((e) => ({ t: e.t, cid: e.cid, layout: e.layout, spec: e.spec })),
    attach,
    oamOptions,
    timeline,
    adRequests,
    facts: adformatFacts(runDir),
  };
  return result;
}

function toMarkdown(r) {
  const lines = [];
  lines.push(`# Ad formats: ${r.scenario ?? '(no scenario)'} (${r.mode})`, '');
  lines.push(`- run: \`${r.runDir}\``);
  lines.push(`- window monitor everVisible: ${r.everVisible}`);
  lines.push(`- live loads logged: ${r.liveLoads}`, '');
  lines.push('## Attach params (embedder -> host)', '');
  for (const a of r.attach) lines.push(`- t=${a.t} \`${JSON.stringify(a.params)}\``);
  lines.push('', '## Ad library options on the wire (OAM `options`)', '');
  for (const o of r.oamOptions)
    lines.push(`- ${o.atSeconds}s \`${o.path}\` \`${JSON.stringify(o.options)}\``);
  lines.push(
    '',
    '## Timeline (ms since app start)',
    '',
    '| t | source | what | cid | detail |',
    '| --- | --- | --- | --- | --- |',
  );
  for (const e of r.timeline) {
    const detail =
      e.detail === undefined || e.detail === null ? '' : JSON.stringify(e.detail).slice(0, 200);
    lines.push(
      `| ${e.t} | ${e.src} | ${String(e.what).replace(/\|/g, '\\|').replace(/\n/g, ' ')} | ${e.cid ?? ''} | ${detail.replace(/\|/g, '\\|')} |`,
    );
  }
  lines.push(
    '',
    '## Ad-unit / creative requests',
    '',
    '| ms | method | status | initiator | url |',
    '| --- | --- | --- | --- | --- |',
  );
  for (const q of r.adRequests)
    lines.push(
      `| ${q.atMs} | ${q.method} | ${q.status} | ${q.initiator} | ${q.url.replace(/\|/g, '%7C')} |`,
    );
  return lines.join('\n') + '\n';
}

if (import.meta.url === `file://${resolve(argv[1] ?? '')}`) {
  const runDir = resolve(argv[2] ?? '');
  const report = adformatReport(runDir);
  writeFileSync(join(runDir, 'adformats.json'), JSON.stringify(report, null, 2) + '\n');
  writeFileSync(join(runDir, 'adformats.md'), toMarkdown(report));
  console.log(join(runDir, 'adformats.md'));
}
