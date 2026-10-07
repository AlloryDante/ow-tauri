#!/usr/bin/env node
// Compares an ow-electron capture with an ow-tauri capture of the same
// scenario (both made by run.mjs, the second with --host tauri) and lists
// every observable difference, each classified:
//
//   intended:host-label  the owner's labelling rule (tauri_*, tauri-<tv>, Tauri/<tv>)
//   intended:os-gap      a documented platform gap (WebKit, macOS; CONTRACT / PARITY)
//   intended:optimised   documented "optimised, same outcome"
//   intended:deviation   a documented ow-tauri decision (PARITY deviations)
//   variance             differs between two ow-electron runs too (ad content, network)
//   not-mirrored         a harness step the Tauri edition cannot run (README)
//   BUG                  anything else
//
// Volatile values (timestamps, session ids, TCF strings, cache-busters) are
// normalised before comparing. Durations the app reports (window_closed
// `length`) compare within --tolerance-ms; requests ow-electron sends
// together must leave ow-tauri within --burst-ms of each other.
//
//   node parity-diff.mjs captures/<electron-run> captures/<tauri-run> [--tolerance-ms 1500] [--burst-ms 250]
//
// Ad formats (sections adformat-*): per element the lifecycle order, payload
// keys, removal and `destroyed`, the DOM state before and after the first
// display_ad_loaded; the ad library options on the wire; guest mute states;
// lab hit probes (page routing, ow-tauri native routing, composited colour
// at each point, app clicks) and whether the app became frontmost
// (lib/adformat-report.mjs adformatFacts).
//
// Writes parity-diff.json and parity-diff.md into the Tauri run and exits 1
// when a BUG remains.

import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { argv, exit } from 'node:process';
import { fileURLToPath } from 'node:url';

import { adformatFacts } from './lib/adformat-report.mjs';

const readJson = (file) => (existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : null);
const readJsonl = (file) =>
  existsSync(file)
    ? readFileSync(file, 'utf8')
        .split('\n')
        .filter(Boolean)
        .flatMap((line) => {
          try {
            return [JSON.parse(line)];
          } catch {
            return [];
          }
        })
    : [];

// ---------------------------------------------------------------------------
// Normalisation

/** Replaces host-label and volatile values in any string. */
export function normalise(text) {
  if (typeof text !== 'string') return text;
  return (
    text
      // Host label (CONTRACT 0).
      .replace(/\b(?:electron|tauri)_(app_|window_|owadview_|sub_)/g, '<label>_$1')
      .replace(/owver=(?:\d+_\d+_\d+|tauri-\d+_\d+_\d+)/g, 'owver=<owver>')
      .replace(/(owe?Version["=:]+"?)(?:\d+\.\d+\.\d+|tauri-\d+\.\d+\.\d+)/g, '$1<owVersion>')
      .replace(/"owver":"[^"]*"/g, '"owver":"<owver>"')
      // Consent strings (TCF v2 and the additional-consent string).
      .replace(/C[A-Za-z0-9_-]{60,}(?:\.[A-Za-z0-9_-]+)*/g, '<tcf>')
      .replace(/2~[0-9.]{20,}~dv\.?/g, '<ac>')
      // Timestamps: epoch ms, epoch s.
      .replace(/\b1[6-9]\d{11}\b/g, '<ts-ms>')
      .replace(/\b1[6-9]\d{8}\b(?!\.\d)/g, '<ts-s>')
      .replace(/\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z/g, '<iso>')
  );
}

/**
 * Whether `tauri` is ow-electron's user agent `electron` under the labelling
 * rule (CONTRACT E.1): the same platform part and app token
 * (`<PNNS>/<ver>`), a `Tauri/<tv>` token and no `Electron/` token. The
 * engine part may differ (WebKit); anything else is not the host label.
 */
/**
 * Traffic of ow-electron's package manager (OWEPM), which it starts on
 * Windows: its launch and loaded counters, the dev-credentials error and
 * the tracking stats that carry its version (400037, 400043) or the
 * missing credentials (400029). ow-tauri has no package runtime
 * (CONTRACT H).
 */
export const PACKAGE_RUNTIME_REQUEST =
  /\/(Counter electron_pm_[a-z_]+|Counter electron_cs_error|InsertStats 4000(29|37|43))$/;

/** The package manager's own state: its log and its switch in ow-electron.json. */
export const PACKAGE_RUNTIME_FILE = /(^|[\\/])owpm\.log$/;
const PACKAGE_RUNTIME_KEY = 'owepm.enabled';

export function labelledUserAgent(electron, tauri) {
  if (typeof electron !== 'string' || typeof tauri !== 'string') return false;
  const platform = /^Mozilla\/5\.0 \([^)]*\)/.exec(electron)?.[0];
  const app = /\s(\S+\/\S+)\s+Chrome\//.exec(electron)?.[1];
  if (!platform || !app) return false;
  return (
    tauri.startsWith(platform) &&
    tauri.split(' ').includes(app) &&
    /(?:^|\s)Tauri\/\d+\.\d+\.\d+(?:\s|$)/.test(tauri) &&
    !tauri.includes('Electron/')
  );
}

/** Fields whose whole value is the host version (CONTRACT 0). */
const VERSION_FIELDS = new Set(['owver', 'owVersion', 'oweVersion']);

/** Normalises every string inside a JSON value, and epoch numbers. */
export function normaliseDeep(value, key = '') {
  if (VERSION_FIELDS.has(key) && typeof value === 'string') {
    return /^(?:tauri-)?\d+[._]\d+[._]\d+$/.test(value) ? '<owVersion>' : value;
  }
  if (typeof value === 'string') return normalise(value);
  if (typeof value === 'number') {
    if (value > 1.6e12 && value < 2e12) return '<ts-ms>';
    if (value > 1.6e9 && value < 2e9) return '<ts-s>';
    return value;
  }
  if (Array.isArray(value)) return value.map((v) => normaliseDeep(v));
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, normaliseDeep(v, k)]));
  }
  return value;
}

const stable = (v) => JSON.stringify(v);

/** Query parameters that only bust caches. */
const CACHE_BUSTERS = new Set(['_', 'cb', 'rnd', 'random', 'ts', 't', 'cachebuster']);

function parseUrl(url) {
  const u = new URL(url);
  const query = [];
  for (const [k, v] of u.searchParams) {
    if (CACHE_BUSTERS.has(k)) continue;
    let value = v;
    if (k === 'Extra') {
      try {
        value = JSON.parse(v);
      } catch {
        // keep the raw string
      }
    }
    query.push([k, normaliseDeep(value, k)]);
  }
  return { endpoint: `${u.origin}${u.pathname}`, query };
}

function headerMap(lines) {
  const out = [];
  for (const line of lines ?? []) {
    const i = line.indexOf(':', line.startsWith(':') ? 1 : 0);
    out.push([line.slice(0, i).trim().toLowerCase(), line.slice(i + 1).trim()]);
  }
  return out;
}

function cookieNames(value) {
  return (value ?? '')
    .split(';')
    .map((s) => s.trim().split('=')[0])
    .filter(Boolean);
}

// ---------------------------------------------------------------------------
// Capture model: the same view of either host's capture.

function hostRequests(runDir) {
  const all = readJson(join(runDir, 'netlog-requests.json')) ?? [];
  const host = all.filter(
    (r) =>
      r.initiator === 'not an origin' &&
      r.requestType !== 'main frame' &&
      /^https:\/\/(analyticsnew|tracking|features)\.overwolf\.com\//.test(r.url),
  );
  const t0 = host.length ? Date.parse(host[0].startedAt) : 0;
  return host.map((r) => {
    const { endpoint, query } = parseUrl(r.url);
    let body = r.uploadBody?.text ?? null;
    try {
      body = body === null ? null : JSON.parse(body);
    } catch {
      // keep text
    }
    body = normaliseDeep(body);
    const headers = headerMap(r.sentHeaders);
    const name = query.find(([k]) => k === 'Name')?.[1] ?? (body && body.Kind) ?? '';
    return {
      key: `${r.method} ${endpoint} ${name}`,
      at: Date.parse(r.startedAt) - t0,
      method: r.method,
      endpoint,
      query,
      body,
      protocol: r.protocol,
      headers,
      // Cookies on the wire: the cookie header, or the net log's inclusion
      // records minus the excluded ones (EXCLUDE_*: not sent).
      cookies: r.cookiesSent?.length
        ? r.cookiesSent.filter((c) => !/^EXCLUDE_/.test(c.status ?? '')).map((c) => c.name)
        : cookieNames(headers.find(([n]) => n === 'cookie')?.[1]),
      status: r.status,
    };
  });
}

function adDocuments(runDir) {
  const all = readJson(join(runDir, 'netlog-requests.json')) ?? [];
  return all
    .filter(
      (r) =>
        r.requestType === 'main frame' &&
        r.url.startsWith('https://www.overwolf.com/monsdk/electron/') &&
        r.url.includes('adview'),
    )
    .map((r) => ({ url: r.url, headers: headerMap(r.sentHeaders), hostOnly: !!r.hostHeadersOnly }));
}

function cmpDocuments(runDir) {
  const all = readJson(join(runDir, 'netlog-requests.json')) ?? [];
  return all
    .filter(
      (r) => r.requestType === 'main frame' && r.url.startsWith('https://content.overwolf.com/'),
    )
    .map((r) => normalise(r.url));
}

/**
 * Names each guest by its element's containerId (from the guest probes):
 * ow-tauri probes carry the guest's label; newer ow-electron probes carry the
 * guest's webContents id. Older ow-electron probes are numbered in dom-ready
 * order, which is not webContents id order when a later guest is ready
 * first: the harness writes each probe file just before its `guest-probe`
 * record, so file write order pairs the files with the records' ids. Only
 * when that pairing is incomplete does webContents id order decide.
 */
function guestNamer(runDir) {
  const probes = readdirSync(runDir)
    .map((f) => /^guest-(\d+)-dom-ready-0\.json$/.exec(f))
    .filter(Boolean)
    .map((m) => ({
      n: Number(m[1]),
      written: statSync(join(runDir, m[0])).mtimeMs,
      probe: readJson(join(runDir, m[0])),
    }))
    .sort((a, b) => a.n - b.n);
  const byLabel = new Map();
  const byId = new Map();
  const byNumber = [];
  for (const { probe } of probes) {
    const cid = probe?.overwolf?.containerId ?? null;
    if (probe?.label) byLabel.set(probe.label, cid);
    if (typeof probe?.webContentsId === 'number') byId.set(probe.webContentsId, cid);
    byNumber.push(cid);
  }
  const records = readJsonl(join(runDir, 'events.jsonl')).filter(
    (e) => e.kind === 'guest-probe' && e.label === 'dom-ready-0',
  );
  if (!byId.size && records.length === probes.length) {
    const inWriteOrder = [...probes].sort((a, b) => a.written - b.written || a.n - b.n);
    records.forEach((r, i) =>
      byId.set(r.webContentsId, inWriteOrder[i].probe?.overwolf?.containerId ?? null),
    );
  }
  const ids = [];
  return {
    /** Registers an ow-electron guest webContents id (call in any order). */
    see(id) {
      if (typeof id === 'number' && !ids.includes(id)) ids.push(id);
    },
    name(id) {
      if (typeof id === 'string') return byLabel.get(id) ?? id;
      if (byId.has(id)) return byId.get(id);
      const sorted = [...ids].sort((a, b) => a - b);
      return byNumber[sorted.indexOf(id)] ?? `wc${id}`;
    },
  };
}

/**
 * ow-tauri: per element cid, the guest reloads the ad page asked for after
 * the host had told it `hidden` (lab `reload` records).
 */
function pageReloads(runDir) {
  const namer = guestNamer(runDir);
  const hidden = new Set();
  const counts = {};
  const records = [
    ...readJsonl(join(runDir, 'ipc.jsonl')).filter(
      (e) => e.via === 'guest-call' && e.function === 'setVisibility',
    ),
    ...readJsonl(join(runDir, 'wc-events.jsonl')).filter(
      (e) => e.kind === 'reload' && e.type === 'owadview',
    ),
  ].sort((x, y) => x.t - y.t);
  for (const r of records) {
    if (r.function === 'setVisibility') {
      if (r.args === 'hidden') hidden.add(r.label);
      else hidden.delete(r.label);
    } else if (hidden.has(r.label)) {
      const cid = namer.name(r.label);
      counts[cid] = (counts[cid] ?? 0) + 1;
    }
  }
  return counts;
}

/** Host -> guest private messages, per guest. */
function privateMessages(runDir) {
  const namer = guestNamer(runDir);
  const raw = [];
  for (const e of readJsonl(join(runDir, 'ipc.jsonl'))) {
    if (e.via === 'private-message') {
      raw.push({ t: e.t, id: e.label, type: e.message?.type, data: e.message?.data });
    } else if (e.via === 'webContents._sendInternal' && e.dir === 'host->page') {
      if (e.type === 'owadview') namer.see(e.webContentsId);
      const m = /^\["GUEST_VIEW_PRIVATE_MESSAGE",(.*)\]$/s.exec(e.args ?? '');
      if (!m) continue;
      let msg = null;
      try {
        msg = JSON.parse(m[1]);
      } catch {
        const type = /"type":"([^"]+)"/.exec(m[1]);
        msg = { type: type ? type[1] : '?', data: '<truncated>' };
      }
      raw.push({ t: e.t, id: e.webContentsId, type: msg.type, data: msg.data });
    }
  }
  return raw.map((m) => ({
    t: m.t,
    guest: namer.name(m.id),
    type: m.type,
    data: normaliseDeep(m.data),
  }));
}

/** Visibility the host reported to each guest's document, repeats collapsed. */
/**
 * Whether a close counter's `length` (seconds) is the run's own visible
 * span: from the first-visible-window heartbeat (the second
 * `app_heartbeat`) to the close counter, within 2 s.
 * @param {{key: string, at: number}[]} requests one capture's host requests
 * @param {number} closedAt when the close counter started (ms)
 * @param {number} length the counter's `Extra.length`
 */
export function matchesOwnSpan(requests, closedAt, length) {
  const beats = requests.filter((r) => /_app_heartbeat$/.test(r.key)).sort((x, y) => x.at - y.at);
  const shown = beats[1]?.at;
  if (shown === undefined || typeof closedAt !== 'number' || closedAt < shown) return false;
  return Math.abs((closedAt - shown) / 1000 - length) <= 2;
}

export function guestVisibility(runDir) {
  const namer = guestNamer(runDir);
  const raw = [];
  for (const e of readJsonl(join(runDir, 'ipc.jsonl'))) {
    if (e.via === 'guest-call' && e.function === 'setVisibility') {
      raw.push([e.label, e.args]);
    } else if (e.via === 'webContents._sendInternal') {
      if (e.type === 'owadview') namer.see(e.webContentsId);
      const m = /^\["GUEST_INSTANCE_VISIBILITY_CHANGE","(\w+)"\]$/.exec(e.args ?? '');
      // A state sent before the guest has a document (url '') never reaches
      // the ad page; ow-electron sends the current state again once it has
      // one. Only what the page can observe is compared.
      if (m && e.url !== '') raw.push([e.webContentsId, m[1]]);
    }
  }
  const seq = {};
  for (const [id, state] of raw) {
    const g = namer.name(id);
    seq[g] ??= [];
    if (seq[g][seq[g].length - 1] !== state) seq[g].push(state);
  }
  return seq;
}

/** The element events the harness page saw, per element. */
function elementEvents(runDir) {
  const counts = {};
  for (const e of readJsonl(join(runDir, 'page-events.jsonl'))) {
    if (e.kind !== 'owadview-event') continue;
    const sub =
      e.event === 'did-fail-load' && e.info?.own?.isMainFrame === false ? ' (sub-frame)' : '';
    const key = `${e.cid} ${e.event}${sub}`;
    counts[key] = (counts[key] ?? 0) + 1;
  }
  return counts;
}

function pageRecords(runDir, kind) {
  return readJsonl(join(runDir, 'page-events.jsonl')).filter((e) => e.kind === kind);
}

function guestProbe(runDir, n, phase) {
  return readJson(join(runDir, `guest-${n}-${phase}.json`));
}

function guestProbeCount(runDir) {
  return readdirSync(runDir).filter((f) => /^guest-\d+-dom-ready-0\.json$/.test(f)).length;
}

function consentCookies(runDir) {
  return readJsonl(join(runDir, 'cookie-changes.jsonl'))
    .filter((e) => ['euconsent-v2', 'acconsent'].includes(e.cookie?.name) && !e.removed)
    .map((e) => ({
      t: e.t,
      name: e.cookie.name,
      domain: e.cookie.domain,
      path: e.cookie.path,
      secure: e.cookie.secure,
      httpOnly: e.cookie.httpOnly,
      sameSite: e.cookie.sameSite,
      session: e.cookie.session,
      lifetimeDays: e.cookie.expirationDate
        ? Math.round((e.cookie.expirationDate * 1000 - (e.wall ?? Date.now())) / 86_400_000)
        : null,
      wall: e.wall,
    }));
}

function stateFile(runDir, phase) {
  const dir = join(runDir, 'files', phase, 'ow-electron');
  const manifest = readJson(join(dir, 'manifest.json'));
  const file = join(dir, 'files', 'ow-electron.json');
  return {
    files: manifest?.files?.map((f) => f.path) ?? [],
    text: existsSync(file) ? readFileSync(file, 'utf8') : null,
  };
}

/**
 * Fill impressions (lab check 6) seen by each host's guests: requests in the
 * net log, or for the lab the impression requests of each guest's latest
 * probe (a probe lists every request of the page so far, repeats included,
 * as the net log does).
 */
export function fillImpressions(runDir) {
  const all = readJson(join(runDir, 'netlog-requests.json')) ?? [];
  const fromNetlog = all.filter((r) => /owads_scl_impression/.test(r.url)).length;
  const perGuest = new Map();
  for (const f of readdirSync(runDir)) {
    const m = /^guest-(\d+)-.*\.json$/.exec(f);
    if (!m) continue;
    const resources = readJson(join(runDir, f))?.labResources ?? [];
    const n = resources.filter((url) => /owads_scl_impression/.test(url)).length;
    perGuest.set(m[1], Math.max(perGuest.get(m[1]) ?? 0, n));
  }
  const probed = [...perGuest.values()].reduce((a, b) => a + b, 0);
  return Math.max(fromNetlog, probed);
}

export function loadCapture(runDir) {
  const meta = readJson(join(runDir, 'meta.json'));
  if (!meta) throw new Error(`not a capture: ${runDir}`);
  const overwolf = readJson(join(runDir, 'overwolf.json'));
  return {
    runDir,
    meta,
    host: meta.host ?? 'electron',
    overwolf,
    hostRequests: hostRequests(runDir),
    adDocuments: adDocuments(runDir),
    cmpDocuments: cmpDocuments(runDir),
    cmpPages: readJsonl(join(runDir, 'cmp-pages.jsonl')),
    hasIpc: existsSync(join(runDir, 'ipc.jsonl')),
    privateMessages: privateMessages(runDir),
    visibility: guestVisibility(runDir),
    elementEvents: elementEvents(runDir),
    pageVisibility: pageRecords(runDir, 'page-visibility'),
    elementApi: pageRecords(runDir, 'owadview-api'),
    elementStructure: pageRecords(runDir, 'owadview-structure'),
    hiZone: pageRecords(runDir, 'hi-zone'),
    adLoaded: pageRecords(runDir, 'owadview-event').filter((e) => e.event === 'display_ad_loaded'),
    pageReloads: pageReloads(runDir),
    guestCount: guestProbeCount(runDir),
    consentCookies: consentCookies(runDir),
    stateAfter: stateFile(runDir, 'after'),
    stateBefore: stateFile(runDir, 'before'),
    liveLoads: readJsonl(join(runDir, 'live-loads.jsonl')).filter((l) => !l.fill).length,
    fills: fillImpressions(runDir),
    actions: readJsonl(join(runDir, 'actions.jsonl')),
    formats: adformatFacts(runDir),
    windowEnd:
      readJsonl(join(runDir, 'window-monitor.jsonl')).find((e) => e.kind === 'end') ?? null,
  };
}

// ---------------------------------------------------------------------------
// Comparison

/** Request headers Chromium's HTTP cache adds when it revalidates. */
const CONDITIONAL = ['if-none-match', 'if-modified-since'];

/** Own properties of Electron's `<webview>` element (not of `<owadview>`). */
const WEBVIEW_OWN = [
  'src',
  'disablewebsecurity',
  'blinkfeatures',
  'disableblinkfeatures',
  'webpreferences',
  'contentWindow',
];

/**
 * Classification rules for known differences, first match wins. Each names
 * the document that makes it intended.
 */
const RULES = [
  {
    when: (d) => d.packageRuntime === true,
    cls: 'intended:deviation',
    why: "ow-electron's package manager (OWEPM), which it starts on Windows: its launch counters, tracking stats, log and owepm.enabled switch; ow-tauri has no package runtime (CONTRACT H)",
  },
  {
    when: (d) =>
      d.section === 'element-event' && d.field === 'count' && (d.occludedReload || d.hiddenReload),
    cls: 'variance',
    why: 'the ad page reloaded itself after the host told it hidden (CONTRACT D.5: window occluded, or the slot hidden while the ad was idle); ow-electron does the same, and whether the page asks depends on its ad state and playback speed',
  },
  {
    when: (d) => d.section === 'host-message' && d.field === 'sequence' && d.consentDuringAttach,
    cls: 'variance',
    why: "the guest attached between the startup consent's two messages (or after them) on one host, so it got fewer of them; the consent goes to the guests that exist when it is sent (CONTRACT D.5)",
  },
  {
    when: (d) => d.section === 'host-message' && d.field === 'sequence' && d.consentBeforeGuest,
    cls: 'variance',
    why: 'the startup consent was saved before the guest attached on one host, so there it got no consent message and reads the cookies, as a guest attaching after the save does on either host (CONTRACT D.5: sent to existing guests)',
  },
  {
    when: (d) =>
      d.section === 'host-request' &&
      ((d.field === 'header-order' && d.conditionalOnly) ||
        (d.field === 'status' && d.revalidated)),
    cls: 'intended:optimised',
    why: 'no HTTP cache for host requests: ow-electron revalidates a URL its Chromium cache holds (if-none-match, 304); ow-tauri sends the plain request, which reaches the same server (ARCHITECTURE host requests)',
  },
  {
    when: (d) => d.section === 'ad-document' && d.field === 'from-http-cache',
    cls: 'variance',
    why: 'Chromium served the ad document from its HTTP cache (max-age 180), so ow-electron sent no request to compare',
  },
  {
    when: (d) => d.section === 'element-api' && d.field === 'own' && d.webviewOnly,
    cls: 'intended:os-gap',
    why: "own properties of Electron's <webview> element; <owadview> is not a <webview> (PARITY deviations, CONTRACT B.3.3)",
  },
  {
    when: (d) => d.section === 'host-request' && d.field === 'header-order' && d.pseudoOnly,
    cls: 'intended:os-gap',
    why: 'HTTP/2 pseudo-header order is best effort (CONTRACT E.1); the h2 crate fixes it',
  },
  {
    when: (d) =>
      d.section === 'host-request' &&
      d.field === 'header:user-agent' &&
      labelledUserAgent(d.electron, d.tauri),
    cls: 'intended:host-label',
    why: "UA keeps the platform engine, adds Tauri/<tv> and on WKWebView Safari's product tokens (CONTRACT E.1, PARITY user agent)",
  },
  {
    when: (d) =>
      d.section === 'host-request' &&
      d.electronIncomplete &&
      ['header-order', 'status', 'protocol'].includes(d.field),
    cls: 'variance',
    why: "ow-electron's netlog holds no completed exchange for this request (no sent headers, no response); ow-tauri's completed",
  },
  {
    when: (d) => d.section === 'host-request' && d.field === 'Extra.length' && d.ownSpans,
    cls: 'variance',
    why: "each host reports its own run's visible span (first-visible heartbeat to close); the runs were not equally long (load gate, duration)",
  },
  {
    when: (d) => d.section === 'host-request' && d.field === 'order' && d.cmpFirst,
    cls: 'intended:optimised',
    why: 'cmp-eu-only starts at RunEvent::Ready, the analytics sequence at main_ready (CONTRACT E.2 order and timing)',
  },
  {
    when: (d) => d.section === 'host-request' && d.field === 'protocol' && d.tauri === null,
    cls: 'not-mirrored',
    why: 'the lab sees the protocol only on completed requests',
  },
  {
    when: (d) =>
      d.section === 'consent-cookie' &&
      d.field === 'sameSite' &&
      d.tauri === null &&
      ['no_restriction', 'unspecified'].includes(d.electron),
    cls: 'intended:os-gap',
    why: 'WKHTTPCookieStore reports SameSite=None as no policy (NSHTTPCookie sameSitePolicy nil)',
  },
  {
    when: (d) => d.section === 'consent-cookie' && d.field === 'write-order',
    cls: 'intended:os-gap',
    why: 'the lab polls the WebKit cookie store; two cookies written within one poll keep store order',
  },
  {
    when: (d) =>
      d.section === 'guest' &&
      (d.field === 'cookie-order' ||
        (d.field === 'userAgent' && labelledUserAgent(d.electron, d.tauri))),
    cls: 'intended:os-gap',
    why: 'the guest engine is WebKit (CONTRACT D.1); WebKit orders document.cookie by its store',
  },
  {
    when: (d) => d.section === 'element-event' && /did-fail-load \(sub-frame\)$/.test(d.key),
    cls: 'intended:os-gap',
    why: 'sub-frame did-fail-load only where the platform reports it (CONTRACT D.5 table)',
  },
  {
    when: (d) =>
      d.section === 'element-event' &&
      /\s(did-frame-|media-|did-start-navigation|load-commit|console-message|will-frame-navigate|update-target-url|page-favicon-updated|did-start-loading|did-stop-loading|did-navigate)/.test(
        d.key,
      ),
    cls: 'intended:os-gap',
    why: 'Electron <webview> events with no platform equivalent (PARITY deviations, CONTRACT B.3.5)',
  },
  {
    when: (d) =>
      d.section === 'element-event' &&
      /\s(display_ad_loaded|impression|play|player_loaded|pause|ended|complete|video_ad_ready)$/.test(
        d.key,
      ) &&
      !(d.electron > 0 && d.tauri === 0),
    cls: 'variance',
    why: "ad-driven counts depend on the ads served and on playback speed (ow-electron's opacity-0 harness window plays the 15 s test video about 2.5 times slower, so fewer complete/impression cycles); an event ow-electron reports and ow-tauri never does stays a bug",
  },
  {
    when: (d) =>
      d.section === 'element-structure' && d.field === 'shadowChildren' && d.tauri === null,
    cls: 'intended:os-gap',
    why: 'attachShadow refuses `owadview` (not a valid custom element name) outside Electron; the runtime logs it and skips the anchor (owadview.ts)',
  },
  {
    when: (d) => d.section === 'element-api' && d.field === 'webview-methods',
    cls: 'intended:os-gap',
    why: 'no generic <webview> methods on <owadview> (PARITY deviations, CONTRACT B.3.3)',
  },
  {
    when: (d) =>
      d.section === 'state-file' && d.field === 'extra-file' && d.tauri === 'ow-tauri.json',
    cls: 'intended:optimised',
    why: 'ow-tauri keeps its own options next to ow-electron.json (CONTRACT F)',
  },
  {
    when: (d) => d.section === 'identity' && d.field === 'versions',
    cls: 'intended:host-label',
    why: 'process.versions name the host (CONTRACT B.2)',
  },
  {
    when: (d) => d.section === 'identity' && d.field === 'userAgentFallback',
    cls: 'intended:os-gap',
    why: 'app.userAgentFallback reports the platform webview UA (CONTRACT B.2, E.1)',
  },
  {
    when: (d) => d.section === 'action' && d.field === 'unsupported',
    cls: 'not-mirrored',
    why: 'the Tauri harness has no equivalent for this Electron-only harness step (README)',
  },
  {
    when: (d) =>
      d.section === 'host-request' &&
      d.field === 'status' &&
      d.electron === null &&
      /_window_closed/.test(d.key),
    cls: 'variance',
    why: "ow-electron's net log ends at quit before the response to the last window_closed counter (sent as the window closes)",
  },
  {
    when: (d) =>
      d.section === 'adformat-element' &&
      d.field === 'events' &&
      d.adDriven &&
      !(d.missing ?? []).length,
    cls: 'variance',
    why: 'extra ad-driven events depend on the ads served and playback speed; an event ow-electron reports and ow-tauri never does stays a bug',
  },
  {
    when: (d) => d.section === 'adformat-probe' && d.field === 'click' && d.tauriSentNotDelivered,
    cls: 'not-mirrored',
    why: "WebKit does not turn the lab's synthesized NSEvents into DOM events in the invisible window; the native hit test (field native-routing) is the routing proof, and the owner's rehearsal clicks for real",
  },
  {
    when: (d) => d.section === 'adformat-probe' && d.field === 'colour' && d.ambiguous,
    cls: 'variance',
    why: 'the colour samples a pixel of ad or text content (class other, or a point that lands on a served creative) that differs with the ad served and its paint time; transparency, z-order and blur checks use the red container, the app control and the bare corner',
  },
  {
    when: (d) => d.section === 'adformat-mute' && d.playbackCycles,
    cls: 'variance',
    why: 'one host played more test videos in the run (playback speed, see the ad-driven counts rule); each play unmutes and mutes the guest once more, the sequences agree otherwise',
  },
  {
    when: (d) => d.section === 'element-structure' && d.zoneTiming,
    cls: 'variance',
    why: 'the structure sample fell before the high-impact expansion on one host and after it on the other; the expansion follows the ad served (both rects are checked by the adformat layout samples)',
  },
  {
    when: (d) => d.section === 'element-structure' && d.loadTiming,
    cls: 'variance',
    why: 'the structure sample fell before the first display_ad_loaded on one host and after it on the other, and the attributes differ only in the pointer-events the page switches then (the adformat pointer section compares pointer-events before and after that event)',
  },
  {
    when: (d) => d.section === 'guest-count' || d.section === 'host-request-count-variance',
    cls: 'variance',
    why: 'guest reloads depend on ad content and visibility timing',
  },
];

export function classify(d) {
  for (const rule of RULES) {
    if (rule.when(d)) return { ...d, class: rule.cls, why: rule.why };
  }
  return { ...d, class: 'BUG', why: d.why ?? 'no documented reason' };
}

/** Longest common subsequence of two key lists: matched index pairs. */
function align(a, b) {
  const n = a.length;
  const m = b.length;
  const dp = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const pairs = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      pairs.push([i, j]);
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) i++;
    else j++;
  }
  return pairs;
}

function compareHostRequests(e, t, out, tolerance, burst) {
  const ek = e.hostRequests.map((r) => normalise(r.key));
  const tk = t.hostRequests.map((r) => normalise(r.key));
  // Same multiset, different order?
  const pairs = [];
  const used = new Set();
  for (let i = 0; i < ek.length; i++) {
    const j = tk.findIndex((k, idx) => k === ek[i] && !used.has(idx));
    if (j >= 0) {
      used.add(j);
      pairs.push([i, j]);
    } else {
      out.push({
        section: 'host-request',
        key: ek[i],
        field: 'missing',
        electron: 'sent',
        tauri: 'not sent',
        packageRuntime: PACKAGE_RUNTIME_REQUEST.test(ek[i]),
      });
    }
  }
  tk.forEach((k, j) => {
    if (!used.has(j))
      out.push({
        section: 'host-request',
        key: k,
        field: 'extra',
        electron: 'not sent',
        tauri: 'sent',
      });
  });
  const lcs = align(ek, tk);
  if (lcs.length !== pairs.length) {
    const eOrder = ek.filter((_, i) => pairs.some(([a]) => a === i));
    const tOrder = pairs
      .map(([, j]) => j)
      .sort((a, b) => a - b)
      .map((j) => tk[j]);
    const firstDiff = eOrder.findIndex((k, i) => k !== tOrder[i]);
    out.push({
      section: 'host-request',
      key: 'sequence',
      field: 'order',
      electron: eOrder.slice(Math.max(0, firstDiff - 1), firstDiff + 3),
      tauri: tOrder.slice(Math.max(0, firstDiff - 1), firstDiff + 3),
      cmpFirst:
        eOrder.length === tOrder.length &&
        stable(eOrder.filter((k) => !k.includes('cmp-eu-only'))) ===
          stable(tOrder.filter((k) => !k.includes('cmp-eu-only'))),
    });
  }
  for (const [i, j] of pairs) {
    const a = e.hostRequests[i];
    const b = t.hostRequests[j];
    const key = `${normalise(a.key)} #${i}`;
    // ow-electron's capture holds the request but no response or sent
    // headers: the request never completed in its netlog.
    const electronIncomplete = (a.status ?? null) === null && (b.status ?? null) !== null;
    const push = (field, ev, tv, extra = {}) =>
      out.push({
        section: 'host-request',
        key,
        field,
        electron: ev,
        tauri: tv,
        ...(electronIncomplete ? { electronIncomplete } : {}),
        ...extra,
      });
    if (stable(a.query.map(([k]) => k)) !== stable(b.query.map(([k]) => k))) {
      push(
        'query-order',
        a.query.map(([k]) => k),
        b.query.map(([k]) => k),
      );
    }
    for (const [k, v] of a.query) {
      const other = b.query.find(([bk]) => bk === k)?.[1];
      if (other === undefined) continue;
      if (k === 'Extra' && v && typeof v === 'object' && other && typeof other === 'object') {
        if (stable(Object.keys(v)) !== stable(Object.keys(other))) {
          push('Extra-order', Object.keys(v), Object.keys(other));
        }
        for (const field of Object.keys(v)) {
          if (field === 'length' && typeof v.length === 'number') {
            if (Math.abs(v.length - (other.length ?? -99)) > Math.max(2, tolerance / 1000)) {
              push(`Extra.${field}`, v.length, other.length, {
                why: 'visible period length beyond tolerance',
                ownSpans:
                  matchesOwnSpan(e.hostRequests, a.at, v.length) &&
                  matchesOwnSpan(t.hostRequests, b.at, other.length),
              });
            }
            continue;
          }
          if (stable(v[field]) !== stable(other[field]))
            push(`Extra.${field}`, v[field], other[field]);
        }
      } else if (stable(v) !== stable(other)) {
        push(`query.${k}`, v, other);
      }
    }
    if (stable(a.body) !== stable(b.body)) push('body', a.body, b.body);
    const names = (h) => h.map(([n]) => n);
    if (stable(names(a.headers)) !== stable(names(b.headers))) {
      const regular = (h) => names(h).filter((n) => !n.startsWith(':'));
      const pseudo = (h) => names(h).filter((n) => n.startsWith(':'));
      if (stable(regular(a.headers)) !== stable(regular(b.headers))) {
        // Chromium's HTTP cache revalidates a URL it has seen before.
        const plain = regular(a.headers).filter((n) => !CONDITIONAL.includes(n));
        push('header-order', regular(a.headers), regular(b.headers), {
          conditionalOnly: stable(plain) === stable(regular(b.headers)),
        });
      }
      if (stable(pseudo(a.headers)) !== stable(pseudo(b.headers))) {
        push('header-order', pseudo(a.headers), pseudo(b.headers), { pseudoOnly: true });
      }
    }
    for (const [n, v] of a.headers) {
      if (n.startsWith(':') || n === 'cookie' || CONDITIONAL.includes(n)) continue;
      const other = b.headers.find(([bn]) => bn === n)?.[1];
      if (other === undefined) continue;
      if (n === 'user-agent') {
        if (v !== other) push('header:user-agent', v, other);
        continue;
      }
      if (n === 'content-length' && a.body && b.body && stable(a.body) === stable(b.body)) {
        // Lengths differ only through the host label inside the body.
        continue;
      }
      if (normalise(v) !== normalise(other)) push(`header:${n}`, v, other);
    }
    if (stable(a.cookies) !== stable(b.cookies)) push('cookies', a.cookies, b.cookies);
    if (b.status === null && a.status !== null) {
      // The lab records every completed request; none means it failed.
      push('no-response', a.status, null, {
        why: 'ow-electron got a response, ow-tauri none (failed or never completed)',
      });
      continue;
    }
    if (a.status !== b.status && b.status !== null) {
      push('status', a.status, b.status, {
        revalidated: /\b304\b/.test(a.status ?? '') && /\b200\b/.test(b.status),
      });
    }
    if (a.protocol !== b.protocol) push('protocol', a.protocol, b.protocol);
  }
  // Timing: the hosts start at different speeds, so absolute offsets are not
  // comparable. What is: requests ow-electron sends together (a burst,
  // gaps under 100 ms) must leave ow-tauri together too, within `burst` ms.
  const bursts = [];
  for (const [i, j] of [...pairs].sort((x, y) => x[0] - y[0])) {
    const at = e.hostRequests[i].at;
    const last = bursts[bursts.length - 1];
    if (last && at - last.end < 100) {
      last.end = at;
      last.pairs.push([i, j]);
    } else {
      bursts.push({ start: at, end: at, pairs: [[i, j]] });
    }
  }
  for (const b of bursts) {
    if (b.pairs.length < 2) continue;
    const times = b.pairs.map(([, j]) => t.hostRequests[j].at);
    const spread = Math.max(...times) - Math.min(...times);
    const allowed = b.end - b.start + burst;
    if (spread > allowed) {
      out.push({
        section: 'host-request',
        key: `burst of ${b.pairs.length} at ${b.start} ms (${b.pairs.map(([i]) => normalise(e.hostRequests[i].key).split(' ').pop()).join(', ')})`,
        field: 'timing',
        electron: `spread ${b.end - b.start} ms`,
        tauri: `spread ${spread} ms`,
        why: `requests ow-electron sends together left ${spread} ms apart (allowed ${allowed} ms)`,
      });
    }
  }
}

function compareIdentity(e, t, out) {
  const em = e.overwolf?.snapshots?.[0];
  const tm = t.overwolf?.snapshots?.[0];
  if (!em || !tm) {
    out.push({
      section: 'identity',
      key: 'overwolf.json',
      field: 'missing',
      electron: !!em,
      tauri: !!tm,
    });
    return;
  }
  if (stable(em.env) !== stable(tm.env))
    out.push({ section: 'identity', key: 'env', field: 'env', electron: em.env, tauri: tm.env });
  const ek = Object.keys(em.members ?? {});
  const tk = Object.keys(tm.members ?? {});
  if (stable(ek) !== stable(tk))
    out.push({
      section: 'identity',
      key: 'app.overwolf',
      field: 'members',
      electron: ek,
      tauri: tk,
    });
  for (const k of ek) {
    const a = em.members[k];
    const b = tm.members?.[k];
    if (!b) continue;
    if (stable(normaliseDeep(a)) !== stable(normaliseDeep(b))) {
      out.push({
        section: 'identity',
        key: `app.overwolf.${k}`,
        field: 'value',
        electron: a,
        tauri: b,
      });
    }
  }
  for (const k of ['appName', 'appVersion', 'platform', 'arch']) {
    if (e.overwolf[k] !== t.overwolf[k])
      out.push({
        section: 'identity',
        key: k,
        field: 'value',
        electron: e.overwolf[k],
        tauri: t.overwolf[k],
      });
  }
  if (e.overwolf.userAgentFallback !== t.overwolf.userAgentFallback) {
    out.push({
      section: 'identity',
      key: 'app.userAgentFallback',
      field: 'userAgentFallback',
      electron: e.overwolf.userAgentFallback,
      tauri: t.overwolf.userAgentFallback,
    });
  }
  out.push({
    section: 'identity',
    key: 'process.versions',
    field: 'versions',
    electron: Object.keys(e.overwolf.versions ?? {}).slice(-3),
    tauri: Object.keys(t.overwolf.versions ?? {}),
    informational: true,
  });
  // Calls and later snapshots, by label.
  const tCalls = new Map((t.overwolf.calls ?? []).map((c) => [c.label, c]));
  const unsupported = new Set(
    t.actions.filter((a) => a.phase === 'action-unsupported').map((a) => a.label ?? a.do),
  );
  for (const c of e.overwolf.calls ?? []) {
    const other = tCalls.get(c.label);
    if (!other) {
      out.push({
        section: unsupported.has(c.label) ? 'action' : 'call',
        key: c.label,
        field: unsupported.has(c.label) ? 'unsupported' : 'missing',
        electron: 'called',
        tauri: 'not called',
      });
      continue;
    }
    if (
      c.ok !== other.ok ||
      stable(normaliseDeep(c.result)) !== stable(normaliseDeep(other.result))
    ) {
      out.push({
        section: 'call',
        key: c.label,
        field: 'result',
        electron: { ok: c.ok, result: c.result },
        tauri: { ok: other.ok, result: other.result },
      });
    }
  }
  const tSnaps = new Map((t.overwolf.snapshots ?? []).map((s) => [s.label, s]));
  for (const s of (e.overwolf.snapshots ?? []).slice(1)) {
    const other = tSnaps.get(s.label);
    if (!other) continue;
    if (stable(normaliseDeep(s.changed)) !== stable(normaliseDeep(other.changed))) {
      out.push({
        section: 'call',
        key: `snapshot ${s.label}`,
        field: 'changed',
        electron: s.changed,
        tauri: other.changed,
      });
    }
  }
}

function compareActions(e, t, out) {
  for (const a of t.actions.filter((x) => x.phase === 'action-unsupported')) {
    out.push({
      section: 'action',
      key: `${a.do} ${a.label ?? ''}`.trim(),
      field: 'unsupported',
      electron: 'run',
      tauri: 'not mirrored',
    });
  }
}

function compareAdDocuments(e, t, out) {
  // A document Chromium served from its HTTP cache sent no request (no
  // headers in the net log); compare the first one that went out.
  const ed = e.adDocuments.find((d) => d.headers.length > 0);
  const td = t.adDocuments[0];
  if (e.adDocuments.length > 0 && e.adDocuments[0] !== ed) {
    out.push({
      section: 'ad-document',
      key: 'first load',
      field: 'from-http-cache',
      electron: 'served from cache',
      tauri: td ? 'requested' : null,
    });
    // Every ad document came from the cache: nothing to compare.
    if (!ed) return;
  }
  if (!ed || !td) {
    if (ed || td)
      out.push({
        section: 'ad-document',
        key: 'first load',
        field: 'missing',
        electron: !!ed,
        tauri: !!td,
      });
    return;
  }
  if (normalise(ed.url) !== normalise(td.url))
    out.push({ section: 'ad-document', key: 'url', field: 'url', electron: ed.url, tauri: td.url });
  // The lab sees the fields the host sets; compare those, and the ones the
  // host must not set.
  for (const name of ['referer', 'origin']) {
    const a = ed.headers.find(([n]) => n === name)?.[1] ?? null;
    const b = td.headers.find(([n]) => n === name)?.[1] ?? null;
    if (a !== b)
      out.push({
        section: 'ad-document',
        key: name,
        field: `header:${name}`,
        electron: a,
        tauri: b,
      });
  }
  for (const [n, v] of td.headers) {
    const a = ed.headers.find(([en]) => en === n)?.[1];
    if (a === undefined)
      out.push({ section: 'ad-document', key: n, field: 'extra-header', electron: null, tauri: v });
  }
}

function compareCmp(e, t, out) {
  const ed = [...new Set(e.cmpDocuments)];
  const td = [...new Set(t.cmpDocuments)];
  for (const u of ed)
    if (!td.includes(u))
      out.push({
        section: 'consent-page',
        key: 'document',
        field: 'missing',
        electron: u,
        tauri: null,
      });
  for (const u of td)
    if (!ed.includes(u))
      out.push({
        section: 'consent-page',
        key: 'document',
        field: 'extra',
        electron: null,
        tauri: u,
      });
  const shape = (p) =>
    normaliseDeep({
      href: p.href,
      cmp: p.cmp,
      privacy: p.privacy,
      overwolf: p.overwolf,
      closeNative: p.closeNative,
    });
  const byHref = (pages) => {
    const m = new Map();
    for (const p of pages) m.set(normalise(p.href), shape(p));
    return m;
  };
  const em = byHref(e.cmpPages);
  const tm = byHref(t.cmpPages);
  for (const [href, s] of em) {
    const other = tm.get(href);
    if (!other) continue;
    if (stable(s) !== stable(other))
      out.push({ section: 'consent-page', key: href, field: 'globals', electron: s, tauri: other });
  }
}

function compareConsentCookies(e, t, out) {
  const latest = (list) => {
    const m = new Map();
    for (const c of list) if (!m.has(c.name)) m.set(c.name, c);
    return m;
  };
  const em = latest(e.consentCookies);
  const tm = latest(t.consentCookies);
  for (const [name, a] of em) {
    const b = tm.get(name);
    if (!b) {
      out.push({
        section: 'consent-cookie',
        key: name,
        field: 'missing',
        electron: 'written',
        tauri: 'not written',
      });
      continue;
    }
    for (const f of ['domain', 'path', 'secure', 'httpOnly', 'sameSite', 'session']) {
      if (a[f] !== b[f])
        out.push({ section: 'consent-cookie', key: name, field: f, electron: a[f], tauri: b[f] });
    }
    if (
      a.lifetimeDays !== null &&
      b.lifetimeDays !== null &&
      Math.abs(a.lifetimeDays - b.lifetimeDays) > 1
    ) {
      out.push({
        section: 'consent-cookie',
        key: name,
        field: 'lifetime',
        electron: `${a.lifetimeDays} d`,
        tauri: `${b.lifetimeDays} d`,
      });
    }
  }
  const order = (list) => [...new Set(list.map((c) => c.name))];
  if (em.size === tm.size && stable(order(e.consentCookies)) !== stable(order(t.consentCookies))) {
    out.push({
      section: 'consent-cookie',
      key: 'order',
      field: 'write-order',
      electron: order(e.consentCookies),
      tauri: order(t.consentCookies),
    });
  }
}

/** `value` without the package manager's switch (an object's own key). */
export function withoutPackageRuntime(value) {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return value;
  const { [PACKAGE_RUNTIME_KEY]: _ignored, ...rest } = value;
  return rest;
}

/** Key structure of a JSON text, with volatile values normalised. */
function jsonShape(text) {
  try {
    return normaliseDeep(JSON.parse(text));
  } catch {
    return text;
  }
}

function compareStateFile(e, t, out) {
  for (const phase of ['before', 'after']) {
    const a = phase === 'after' ? e.stateAfter : e.stateBefore;
    const b = phase === 'after' ? t.stateAfter : t.stateBefore;
    for (const f of b.files)
      if (!a.files.includes(f))
        out.push({
          section: 'state-file',
          key: `${phase}/${f}`,
          field: 'extra-file',
          electron: null,
          tauri: f,
        });
    for (const f of a.files)
      if (!b.files.includes(f))
        out.push({
          section: 'state-file',
          key: `${phase}/${f}`,
          field: 'missing-file',
          electron: f,
          tauri: null,
          packageRuntime: PACKAGE_RUNTIME_FILE.test(f),
        });
    if (a.text === null || b.text === null) continue;
    const sa = jsonShape(a.text);
    const sb = jsonShape(b.text);
    if (stable(sa) !== stable(sb))
      out.push({
        section: 'state-file',
        key: `${phase}/ow-electron.json`,
        field: 'content',
        electron: sa,
        tauri: sb,
        packageRuntime: stable(withoutPackageRuntime(sa)) === stable(sb),
      });
    // Byte format: whitespace and key order (values normalised).
    const fmt = (s) => normalise(s).replace(/"(timeStamp)":\d+/g, '"$1":<n>');
    if (stable(sa) === stable(sb) && fmt(a.text) !== fmt(b.text)) {
      out.push({
        section: 'state-file',
        key: `${phase}/ow-electron.json`,
        field: 'bytes',
        electron: fmt(a.text).slice(0, 200),
        tauri: fmt(b.text).slice(0, 200),
      });
    }
  }
}

const GUEST_SKIP = new Set(['localStorage', 'innerSize', 'devicePixelRatio', 'label', 'hasFocus']);

function guestProbesByContainer(runDir) {
  const out = new Map();
  for (const f of readdirSync(runDir).sort()) {
    if (!/^guest-\d+-dom-ready-0\.json$/.test(f)) continue;
    const probe = readJson(join(runDir, f));
    const overwolf = probe?.overwolf;
    if (!overwolf) continue;
    // A performance guest has an empty containerId.
    const cid = overwolf.containerId || (overwolf.performance ? 'performance' : null);
    if (cid && !out.has(cid)) out.set(cid, probe);
  }
  return out;
}

function compareGuests(e, t, out) {
  if (e.guestCount !== t.guestCount)
    out.push({
      section: 'guest-count',
      key: 'guests',
      field: 'count',
      electron: e.guestCount,
      tauri: t.guestCount,
    });
  const ep = guestProbesByContainer(e.runDir);
  const tp = guestProbesByContainer(t.runDir);
  for (const [i, a] of ep) {
    const b = tp.get(i);
    if (!b) continue;
    for (const k of Object.keys(a)) {
      if (GUEST_SKIP.has(k) || k.startsWith('lab')) continue;
      if (k === 'userAgent') {
        if (a[k] !== b[k])
          out.push({
            section: 'guest',
            key: `guest ${i}`,
            field: 'userAgent',
            electron: a[k],
            tauri: b[k],
          });
        continue;
      }
      if (k === 'cookie') {
        const an = cookieNames(a.cookie);
        const bn = cookieNames(b.cookie);
        if (stable([...an].sort()) !== stable([...bn].sort())) {
          const consent = (l) => l.filter((x) => ['euconsent-v2', 'acconsent'].includes(x)).sort();
          if (stable(consent(an)) !== stable(consent(bn)))
            out.push({
              section: 'guest',
              key: `guest ${i}`,
              field: 'consent-cookies',
              electron: consent(an),
              tauri: consent(bn),
            });
        } else if (stable(an) !== stable(bn)) {
          out.push({
            section: 'guest',
            key: `guest ${i}`,
            field: 'cookie-order',
            electron: an,
            tauri: bn,
          });
        }
        continue;
      }
      if (k === 'overwolf') {
        const ak = Object.keys(a.overwolf ?? {});
        const bk = Object.keys(b.overwolf ?? {});
        if (stable(ak) !== stable(bk))
          out.push({
            section: 'guest',
            key: `guest ${i} __overwolf__`,
            field: 'keys',
            electron: ak,
            tauri: bk,
          });
        for (const key of ak) {
          const av = normaliseDeep(a.overwolf[key], key);
          const bv = normaliseDeep(b.overwolf?.[key], key);
          if (stable(av) !== stable(bv))
            out.push({
              section: 'guest',
              key: `guest ${i} __overwolf__.${key}`,
              field: 'value',
              electron: av,
              tauri: bv,
            });
        }
        continue;
      }
      if (stable(normaliseDeep(a[k])) !== stable(normaliseDeep(b[k]))) {
        out.push({
          section: 'guest',
          key: `guest ${i}`,
          field: k,
          electron: normaliseDeep(a[k]),
          tauri: normaliseDeep(b[k]),
        });
      }
    }
  }
}

/** Ad events whose presence depends on the ads served (fills, playback). */
const AD_DRIVEN = new Set([
  'display_ad_loaded',
  'performance_ad_loaded',
  'video_ad_ready',
  'high-impact-ad-loaded',
  'high-impact-ad-removed',
  'performance_ad_dismiss',
]);

/**
 * Probe points that land on a served ad creative once it paints. Their colour
 * depends on the creative and its paint time, so a difference there is
 * variance; the red container, app control and bare corner points carry the
 * transparency, z-order and blur checks.
 */
const AD_CONTENT_POINTS = new Set(['std-slot']);

/** Compares the ad-format facts of both runs (lib/adformat-report.mjs). */
export function compareAdformats(e, t, out) {
  const ef = e.formats;
  const tf = t.formats;
  if (!ef || !tf) return;
  for (const [key, a] of Object.entries(ef.elements)) {
    const b = tf.elements[key];
    if (!b) continue;
    const push = (field, electron, tauri, extra = {}) =>
      out.push({ section: 'adformat-element', key, field, electron, tauri, ...extra });
    const missing = a.order.filter((n) => !b.order.includes(n));
    const extra = b.order.filter((n) => !a.order.includes(n));
    if (missing.length || extra.length)
      push('events', a.order, b.order, {
        missing,
        extra,
        adDriven: [...missing, ...extra].every((n) => AD_DRIVEN.has(n)),
      });
    const common = (list, other) => list.filter((n) => other.includes(n));
    if (stable(common(a.order, b.order)) !== stable(common(b.order, a.order)))
      push('order', a.order, b.order);
    if (stable(a.removalOrder) !== stable(b.removalOrder))
      push('removal', a.removalOrder, b.removalOrder);
    for (const [name, keys] of Object.entries(a.payloadKeys)) {
      const other = b.payloadKeys[name];
      if (other && stable(keys) !== stable(other)) push(`payload-keys ${name}`, keys, other);
    }
    for (const phase of ['before', 'after']) {
      const x = a.dom?.[phase];
      const y = b.dom?.[phase];
      if (!x || !y) continue;
      for (const f of ['display', 'pointerEvents', 'inlineStyle', 'overlay'])
        if (stable(x[f]) !== stable(y[f])) push(`dom-${phase} ${f}`, x[f], y[f]);
    }
  }
  for (const [key, a] of Object.entries(ef.oamOptions)) {
    const b = tf.oamOptions[key];
    // An element one host has no option record for (ow-tauri: no probe
    // reached the guest before it went away) is not compared.
    if (!b?.length || !a.length) continue;
    const missingOptions = a.filter((o) => !b.includes(o));
    const extraOptions = b.filter((o) => !a.includes(o));
    if (missingOptions.length || extraOptions.length)
      out.push({
        section: 'adformat-options',
        key,
        field: 'options',
        electron: missingOptions,
        tauri: extraOptions,
      });
  }
  const mutes = (list) => list.map((m) => stable(m)).sort();
  if (stable(mutes(ef.mute)) !== stable(mutes(tf.mute))) {
    // Each played video unmutes the guest and mutes it again; a host that
    // played more videos (playback speed) has more [false, true] pairs.
    const plays = (f) => Object.values(f.elements).reduce((n, el) => n + (el.counts.play ?? 0), 0);
    const byLength = (list) =>
      [...list].sort((x, y) => x.length - y.length || stable(x).localeCompare(stable(y)));
    const [ea, ta] = [byLength(ef.mute), byLength(tf.mute)];
    const extraCycles = (x, y) => {
      const [short, long] = x.length <= y.length ? [x, y] : [y, x];
      const tail = long.slice(short.length);
      return (
        stable(long.slice(0, short.length)) === stable(short) &&
        tail.length % 2 === 0 &&
        tail.every((m, i) => m === (i % 2 === 0 ? false : true))
      );
    };
    out.push({
      section: 'adformat-mute',
      key: 'guest mute states',
      field: 'sequence',
      electron: mutes(ef.mute),
      tauri: mutes(tf.mute),
      playbackCycles:
        plays(ef) !== plays(tf) &&
        ea.length === ta.length &&
        ea.every((x, i) => extraCycles(x, ta[i])),
    });
  }
  for (const [label, a] of Object.entries(ef.probes)) {
    const b = tf.probes[label];
    if (!b) continue;
    for (const [name, x] of Object.entries(a.points)) {
      const y = b.points[name];
      if (!y) continue;
      const key = `${label} ${name}`;
      if (x.dom !== y.dom)
        out.push({
          section: 'adformat-probe',
          key,
          field: 'page-routing',
          electron: x.dom,
          tauri: y.dom,
        });
      const route = (dom) => (dom === 'ad-perf' ? 'ad' : dom);
      if (y.native !== undefined && y.native !== route(y.dom))
        out.push({
          section: 'adformat-probe',
          key,
          field: 'native-routing',
          electron: route(y.dom),
          tauri: y.native,
        });
      if (x.colour && y.colour && x.colour !== y.colour)
        out.push({
          section: 'adformat-probe',
          key,
          field: 'colour',
          electron: x.colour,
          tauri: y.colour,
          ambiguous: x.colour === 'other' || y.colour === 'other' || AD_CONTENT_POINTS.has(name),
        });
    }
    if (a.guestMuted && b.guestMuted && stable(a.guestMuted) !== stable(b.guestMuted))
      out.push({
        section: 'adformat-probe',
        key: label,
        field: 'guest muted',
        electron: a.guestMuted,
        tauri: b.guestMuted,
      });
    if (
      stable(a.performance.map((p) => p.pointerEvents)) !==
      stable(b.performance.map((p) => p.pointerEvents))
    )
      out.push({
        section: 'adformat-probe',
        key: label,
        field: 'performance pointer-events',
        electron: a.performance,
        tauri: b.performance,
      });
  }
  if (ef.clicks.received !== tf.clicks.received || ef.clicks.sent !== tf.clicks.sent)
    out.push({
      section: 'adformat-probe',
      key: 'app control',
      field: 'click',
      electron: ef.clicks,
      tauri: tf.clicks,
      tauriSentNotDelivered:
        t.host === 'tauri' &&
        ef.clicks.sent === tf.clicks.sent &&
        tf.clicks.pointer === 0 &&
        tf.clicks.received === 0,
    });
  if (tf.front === true || ef.front === true)
    out.push({
      section: 'adformat-front',
      key: 'frontmost app',
      field: 'everFront',
      electron: ef.front,
      tauri: tf.front,
    });
}

/** Whether the OS hid the embedder document during the run. */
function osHidden(capture) {
  return capture.pageVisibility.some((r) => r.visibilityState === 'hidden');
}

function compareElementEvents(e, t, out) {
  const keys = new Set([...Object.keys(e.elementEvents), ...Object.keys(t.elementEvents)]);
  for (const key of [...keys].sort()) {
    const a = e.elementEvents[key] ?? 0;
    const b = t.elementEvents[key] ?? 0;
    if (a === b) continue;
    // Ad events: a test-mode fill must still be reported once.
    out.push({
      section: 'element-event',
      key,
      field: 'count',
      electron: a,
      tauri: b,
      // Extra loads after the OS hid the embedder document (occlusion):
      // the ad page reloads itself after `hidden` (D.5).
      occludedReload:
        b > a && /\s(dom-ready|did-finish-load)$/.test(key) && osHidden(t) && !osHidden(e),
      // Extra loads the ad page asked for itself (`__overwolf__.reload()`)
      // after the host told it `hidden` (D.5). Whether the page asks
      // depends on its ad state (a video still playing does not), which
      // follows playback speed.
      hiddenReload:
        b > a &&
        /\s(dom-ready|did-finish-load)$/.test(key) &&
        b - a <= (t.pageReloads?.[key.split(' ')[0]] ?? 0),
    });
  }
  const ea = e.elementApi[0];
  const ta = t.elementApi[0];
  if (ea && ta) {
    const missing = ea.own.filter((k) => !ta.own.includes(k));
    const extra = ta.own.filter((k) => !ea.own.includes(k));
    if (missing.length || extra.length) {
      out.push({
        section: 'element-api',
        key: 'own members',
        field: 'own',
        electron: ea.own,
        tauri: ta.own,
        webviewOnly: extra.length === 0 && missing.every((k) => WEBVIEW_OWN.includes(k)),
      });
    }
    const proto = (x) => x.chain?.[0]?.names ?? [];
    const owadview = ['pageUrl', 'setPageUrl', 'sendCommand'];
    const lost = owadview.filter(
      (k) => [...proto(ea), ...ea.own].includes(k) && ![...proto(ta), ...ta.own].includes(k),
    );
    if (lost.length)
      out.push({
        section: 'element-api',
        key: 'owadview members',
        field: 'owadview-members',
        electron: owadview,
        tauri: lost,
      });
    const webview = proto(ea).filter(
      (k) => ![...proto(ta), ...ta.own].includes(k) && !owadview.includes(k),
    );
    if (webview.length)
      out.push({
        section: 'element-api',
        key: 'prototype',
        field: 'webview-methods',
        electron: `${webview.length} methods`,
        tauri: 'absent',
      });
  }
  const es = e.elementStructure[0];
  const ts = t.elementStructure[0];
  if (es && ts) {
    // The high-impact zone the page expanded (or not) when the structure
    // was sampled: its rect follows the served ad's timing.
    const zone = (c, at) => c.hiZone?.filter((r) => r.t <= at).at(-1)?.action ?? 'restored';
    for (const f of ['attributes', 'shadowChildren', 'rect']) {
      if (stable(es[f]) !== stable(ts[f]))
        out.push({
          section: 'element-structure',
          key: es.cid,
          field: f,
          electron: es[f],
          tauri: ts[f],
          zoneTiming: f === 'rect' && zone(e, es.t) !== zone(t, ts.t),
          loadTiming:
            f === 'attributes' &&
            loadedBefore(e.adLoaded, es) !== loadedBefore(t.adLoaded, ts) &&
            stable(withoutPointerEvents(es[f])) === stable(withoutPointerEvents(ts[f])),
        });
    }
  }
}

/**
 * Whether two guests' host message sequences differ only in how many of
 * the startup consent's two `consent` messages lead them: the guest
 * attached between (or after) those messages on one host. The consent goes
 * to the guests that exist when it is sent (CONTRACT D.5).
 * @param {string[]} a message types on one host
 * @param {string[]} b message types on the other
 */
export function consentDuringAttach(a, b) {
  const lead = (list) => {
    const n = list.findIndex((x) => x !== 'consent');
    return n < 0 ? list.length : n;
  };
  const [la, lb] = [lead(a), lead(b)];
  return (
    la !== lb && la <= 2 && lb <= 2 && JSON.stringify(a.slice(la)) === JSON.stringify(b.slice(lb))
  );
}

/**
 * Whether the element sampled in `sample` had seen its first
 * display_ad_loaded when the sample was taken.
 * @param {{cid?: string, t: number}[]} loaded the display_ad_loaded events
 * @param {{cid?: string, t: number}} sample the structure sample
 */
export function loadedBefore(loaded, sample) {
  return (loaded ?? []).some((e) => e.cid === sample.cid && e.t <= sample.t);
}

/**
 * The attribute pairs with pointer-events taken out of `style`: the page
 * switches pointer-events at the first display_ad_loaded, which the
 * adformat pointer section compares on its own.
 * @param {[string, string][] | null | undefined} attributes
 */
export function withoutPointerEvents(attributes) {
  return (attributes ?? [])
    .map(([k, v]) => [
      k,
      k === 'style' ? v.replace(/pointer-events:\s*[a-z-]+;?\s*/g, '').trim() : v,
    ])
    .filter(([k, v]) => k !== 'style' || v !== '');
}

/**
 * Whether the Tauri run saved the startup consent (`cmp` in the state file)
 * before its first ad guest was created.
 */
function consentSavedBeforeGuests(runDir) {
  // ow-tauri: the state file write; ow-electron: the consent cookie insert.
  const saved =
    readJsonl(join(runDir, 'state-writes.jsonl')).find((w) => /"cmp":/.test(w.text ?? '')) ??
    readJsonl(join(runDir, 'cookie-changes.jsonl')).find(
      (c) => c.cookie?.name === 'euconsent-v2' && !c.removed,
    );
  const guest = [
    ...readJsonl(join(runDir, 'wc-events.jsonl')),
    ...readJsonl(join(runDir, 'webcontents.jsonl')),
  ].find((w) => w.kind === 'created' && w.type === 'owadview');
  return saved !== undefined && guest !== undefined && saved.t < guest.t;
}

function compareMessages(e, t, out) {
  if (!e.hasIpc) {
    out.push({
      section: 'host-message',
      key: 'all',
      field: 'not-recorded',
      electron: 'no ipc.jsonl (round-1 capture)',
      tauri: t.privateMessages.length,
      informational: true,
    });
    return;
  }
  const per = (list) => {
    const m = new Map();
    for (const x of list) {
      if (!m.has(x.guest)) m.set(x.guest, []);
      m.get(x.guest).push(x);
    }
    return m;
  };
  const em = per(e.privateMessages);
  const tm = per(t.privateMessages);
  const guests = new Set([...em.keys(), ...tm.keys()]);
  for (const g of guests) {
    const a = em.get(g) ?? [];
    const b = tm.get(g) ?? [];
    const at = a.map((x) => x.type);
    const bt = b.map((x) => x.type);
    if (stable(at) !== stable(bt)) {
      // The startup consent may be saved before a guest attaches (timing);
      // that guest then reads consent from the cookies only.
      const leading = (list) => {
        const first = list.findIndex((x) => x !== 'consent');
        return first < 0 ? [] : list.slice(first);
      };
      out.push({
        section: 'host-message',
        key: `guest ${g}`,
        field: 'sequence',
        electron: at,
        tauri: bt,
        consentBeforeGuest:
          (consentSavedBeforeGuests(t.runDir) && stable(leading(at)) === stable(bt)) ||
          (consentSavedBeforeGuests(e.runDir) && stable(leading(bt)) === stable(at)),
        consentDuringAttach: consentDuringAttach(at, bt),
      });
      continue;
    }
    a.forEach((x, i) => {
      const y = b[i];
      if (x.data === '<truncated>' || y.data === '<truncated>') return;
      if (stable(x.data) !== stable(y.data))
        out.push({
          section: 'host-message',
          key: `guest ${g} #${i} ${x.type}`,
          field: 'data',
          electron: x.data,
          tauri: y.data,
        });
    });
  }
}

function compareVisibility(e, t, out) {
  if (!e.hasIpc) return;
  const guests = new Set([...Object.keys(e.visibility), ...Object.keys(t.visibility)]);
  for (const g of guests) {
    const a = e.visibility[g] ?? [];
    const b = t.visibility[g] ?? [];
    if (stable(a) !== stable(b))
      out.push({
        section: 'visibility',
        key: `guest ${g}`,
        field: 'sequence',
        electron: a,
        tauri: b,
      });
  }
}

function compareLive(e, t, out) {
  const mode = t.meta.options?.mode;
  if (mode !== 'live') return;
  out.push({
    section: 'live',
    key: 'loads',
    field: 'loads',
    electron: e.liveLoads,
    tauri: t.liveLoads,
    informational: true,
  });
  out.push({
    section: 'live',
    key: 'fill impressions',
    field: 'fills',
    electron: e.fills,
    tauri: t.fills,
    informational: true,
  });
  if (e.fills > 0 && t.fills === 0)
    out.push({
      section: 'live',
      key: 'fill impressions',
      field: 'no-fill',
      electron: e.fills,
      tauri: 0,
      why: 'ow-electron filled, ow-tauri did not (lab check 6)',
    });
}

export function diffCaptures(electronDir, tauriDir, { tolerance = 1500, burst = 250 } = {}) {
  const e = loadCapture(electronDir);
  const t = loadCapture(tauriDir);
  const raw = [];
  compareIdentity(e, t, raw);
  compareActions(e, t, raw);
  compareHostRequests(e, t, raw, tolerance, burst);
  compareAdDocuments(e, t, raw);
  compareCmp(e, t, raw);
  compareConsentCookies(e, t, raw);
  compareStateFile(e, t, raw);
  compareGuests(e, t, raw);
  compareElementEvents(e, t, raw);
  compareMessages(e, t, raw);
  compareVisibility(e, t, raw);
  compareLive(e, t, raw);
  compareAdformats(e, t, raw);
  const diffs = raw.filter((d) => !d.informational).map(classify);
  const info = raw.filter((d) => d.informational);
  const counts = {};
  for (const d of diffs) counts[d.class] = (counts[d.class] ?? 0) + 1;
  return {
    electron: { runId: e.meta.runId, options: e.meta.options },
    tauri: {
      runId: t.meta.runId,
      options: t.meta.options,
      everVisible: t.windowEnd?.everVisible ?? null,
    },
    tolerance,
    burst,
    counts,
    bugs: diffs.filter((d) => d.class === 'BUG').length,
    diffs,
    info,
  };
}

const short = (v) => {
  const s = typeof v === 'string' ? v : JSON.stringify(v);
  return s === undefined ? '' : s.length > 160 ? `${s.slice(0, 157)}...` : s;
};

export function renderMarkdown(result) {
  const lines = [
    `# Parity diff: ${result.electron.runId} (ow-electron) vs ${result.tauri.runId} (ow-tauri)`,
    '',
    `Tolerance ${result.tolerance} ms, bursts ${result.burst} ms. Tauri windows ever visible: ${result.tauri.everVisible}.`,
    '',
    `Counts: ${
      Object.entries(result.counts)
        .map(([k, v]) => `${k} ${v}`)
        .join(', ') || 'no differences'
    }`,
    '',
    '| Class | Section | Key | Field | ow-electron | ow-tauri | Why |',
    '|---|---|---|---|---|---|---|',
    ...result.diffs.map((d) =>
      `| ${d.class} | ${d.section} | ${short(d.key)} | ${d.field} | ${short(d.electron)} | ${short(d.tauri)} | ${d.why ?? ''} |`.replace(
        /\n/g,
        ' ',
      ),
    ),
    '',
    ...result.info.map(
      (d) =>
        `- ${d.section} ${d.key}: ow-electron ${short(d.electron)}, ow-tauri ${short(d.tauri)}`,
    ),
    '',
  ];
  return lines.join('\n');
}

function main() {
  const args = argv.slice(2);
  const option = (name, fallback) => {
    const i = args.indexOf(name);
    return i >= 0 ? Number(args.splice(i, 2)[1]) : fallback;
  };
  const tolerance = option('--tolerance-ms', 1500);
  const burst = option('--burst-ms', 250);
  if (args.length !== 2) {
    console.error(
      'Usage: node parity-diff.mjs captures/<electron-run> captures/<tauri-run> [--tolerance-ms 1500] [--burst-ms 250]',
    );
    exit(2);
  }
  const [electronDir, tauriDir] = args.map((a) => resolve(a));
  const result = diffCaptures(electronDir, tauriDir, { tolerance, burst });
  writeFileSync(join(tauriDir, 'parity-diff.json'), JSON.stringify(result, null, 2) + '\n');
  const md = renderMarkdown(result);
  writeFileSync(join(tauriDir, 'parity-diff.md'), md);
  console.log(md);
  exit(result.bugs > 0 ? 1 : 0);
}

if (argv[1] && resolve(argv[1]) === fileURLToPath(import.meta.url)) main();
