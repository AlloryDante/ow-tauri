#!/usr/bin/env node
// Summarises one harness run (captures/<run-id>/) into report.md + report.json:
// host analytics in order, ad-guest request shaping (Referer, Origin, x-ow-*
// headers), consent cookies, the guest's window.__overwolf__ keys, and the
// files ow-electron wrote.
//
//   node analyze.mjs captures/<run-id>

import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { argv } from 'node:process';

const readJson = (file) => JSON.parse(readFileSync(file, 'utf8'));
const readJsonl = (file) =>
  existsSync(file)
    ? readFileSync(file, 'utf8')
        .split('\n')
        .filter(Boolean)
        .map((line) => JSON.parse(line))
    : [];

/** Value of a header in a net-log header list ("Name: value"), case-insensitive. */
function header(list, name) {
  const prefix = `${name.toLowerCase()}:`;
  const line = (list ?? []).find((h) => h.toLowerCase().startsWith(prefix));
  return line === undefined ? null : line.slice(prefix.length).trim();
}

const HOST_ANALYTICS = /^https:\/\/(analyticsnew|tracking|features)\.overwolf\.com\//;

/** Requests the app's main process made (no initiating origin). */
function hostAnalytics(requests, t0) {
  return requests
    .filter((r) => r.initiator === 'not an origin' && HOST_ANALYTICS.test(r.url))
    .map((r) => {
      const url = new URL(r.url);
      const query = Object.fromEntries(url.searchParams);
      if (query.Extra) {
        try {
          query.Extra = JSON.parse(query.Extra);
        } catch {
          // keep the raw string
        }
      }
      let body = r.uploadBody?.text ?? null;
      try {
        body = body === null ? null : JSON.parse(body);
      } catch {
        // keep text
      }
      return {
        atSeconds: Number(((Date.parse(r.startedAt) - t0) / 1000).toFixed(1)),
        method: r.method,
        endpoint: `${url.origin}${url.pathname}`,
        query,
        queryOrder: [...url.searchParams.keys()],
        body,
        status: r.status,
        headers: r.sentHeaders,
      };
    });
}

/** Header shaping seen on requests from <owadview> guests (initiator www.overwolf.com or main frame). */
function guestShaping(requests) {
  const guest = requests.filter(
    (r) =>
      r.sentHeaders &&
      (r.initiator === 'https://www.overwolf.com' ||
        (r.requestType === 'main frame' && r.url.startsWith('https://www.overwolf.com/'))),
  );
  const documents = guest
    .filter((r) => r.requestType === 'main frame')
    .map((r) => ({ url: r.url, protocol: r.protocol, headers: r.sentHeaders.map(redactCookie) }));
  const owads = guest
    .filter((r) => /owads(\.min)?\.js/.test(r.url))
    .map((r) => ({ url: r.url, headers: r.sentHeaders.map(redactCookie) }));
  const originCounts = {};
  for (const r of requests) {
    if (!r.sentHeaders || !r.initiator || r.initiator === 'not an origin') continue;
    const key = `initiator=${r.initiator} mode=${header(r.sentHeaders, 'sec-fetch-mode')} origin=${header(r.sentHeaders, 'origin')}`;
    originCounts[key] = (originCounts[key] ?? 0) + 1;
  }
  const xow = requests
    .filter((r) => (r.sentHeaders ?? []).some((h) => /^x-ow-/i.test(h)))
    .map((r) => ({ url: r.url, xow: r.sentHeaders.filter((h) => /^x-ow-/i.test(h)) }));
  return { documents, owads, xow, originCounts };
}

function redactCookie(line) {
  return /^cookie:/i.test(line) ? `${line.split(':')[0]}: <${line.length} chars>` : line;
}

function main() {
  const runDir = resolve(argv[2] ?? '');
  if (!argv[2] || !existsSync(join(runDir, 'meta.json'))) {
    console.error('Usage: node analyze.mjs captures/<run-id>');
    process.exit(2);
  }
  const meta = readJson(join(runDir, 'meta.json'));
  const requests = existsSync(join(runDir, 'netlog-requests.json'))
    ? readJson(join(runDir, 'netlog-requests.json'))
    : [];
  const t0 = requests.length ? Date.parse(requests[0].startedAt) : 0;
  const overwolf = existsSync(join(runDir, 'overwolf.json'))
    ? readJson(join(runDir, 'overwolf.json'))
    : null;
  const guestFiles = readdirSync(runDir).filter((f) => /^guest-\d+-dom-ready-0\.json$/.test(f));
  const guest = guestFiles.length ? readJson(join(runDir, guestFiles[0])) : null;
  const consentCookies = readJsonl(join(runDir, 'cookie-changes.jsonl'))
    .filter((e) => ['euconsent-v2', 'acconsent'].includes(e.cookie.name) && !e.removed)
    .map((e) => ({
      t: e.t,
      cause: e.cause,
      name: e.cookie.name,
      domain: e.cookie.domain,
      path: e.cookie.path,
      secure: e.cookie.secure,
      httpOnly: e.cookie.httpOnly,
      sameSite: e.cookie.sameSite,
      expires: e.cookie.expirationDate
        ? new Date(e.cookie.expirationDate * 1000).toISOString()
        : null,
    }));
  const report = {
    runId: meta.runId,
    owElectron: meta.owElectron,
    mode: meta.options?.mode,
    present: meta.options?.present,
    uid: overwolf?.snapshots?.[0]?.members?.uid?.value ?? null,
    muid: overwolf?.snapshots?.[0]?.members?.muid?.value ?? null,
    phasePercent: overwolf?.snapshots?.[0]?.members?.phasePercent?.value ?? null,
    calls: overwolf?.calls ?? [],
    hostAnalytics: hostAnalytics(requests, t0),
    guestShaping: guestShaping(requests),
    guestOverwolfKeys: guest?.overwolf ? Object.keys(guest.overwolf) : null,
    guestReferrer: guest?.referrer ?? null,
    consentCookies,
    cmpPages: readJsonl(join(runDir, 'cmp-pages.jsonl')).map((p) => ({
      href: p.href,
      cmp: p.cmp,
      privacy: p.privacy,
    })),
    packageEvents: readJsonl(join(runDir, 'packages.jsonl')),
    liveLoads: readJsonl(join(runDir, 'live-loads.jsonl')).length,
    fileDiff: meta.fileDiff,
  };
  writeFileSync(join(runDir, 'report.json'), JSON.stringify(report, null, 2) + '\n');

  const lines = [
    `# Run ${report.runId}`,
    '',
    `ow-electron ${report.owElectron}, ${report.mode} ads, ${report.present} window, uid \`${report.uid}\`, phase ${report.phasePercent}`,
    '',
    '## Host analytics (main process, in order)',
    '',
    '| t (s) | Method | Endpoint | Name / Kind | Fields |',
    '|---|---|---|---|---|',
    ...report.hostAnalytics.map((a) => {
      const name = a.query.Name ?? (a.body && a.body.Kind) ?? '';
      const fields =
        a.query.Extra && typeof a.query.Extra === 'object'
          ? Object.keys(a.query.Extra).join(', ')
          : a.body && a.body.Extra !== undefined
            ? `Extra="${a.body.Extra}"`
            : '';
      return `| ${a.atSeconds} | ${a.method} | ${a.endpoint} | ${name} | ${fields} |`;
    }),
    '',
    '## Ad guest request shaping',
    '',
    ...report.guestShaping.documents.flatMap((d) => [
      `Document ${d.url} (${d.protocol}):`,
      '',
      '```',
      ...d.headers,
      '```',
      '',
    ]),
    ...report.guestShaping.owads
      .slice(0, 1)
      .flatMap((d) => [`Ad library ${d.url}:`, '', '```', ...d.headers, '```', '']),
    'Origin by initiator and fetch mode:',
    '',
    '| Initiator / mode / Origin | Requests |',
    '|---|---|',
    ...Object.entries(report.guestShaping.originCounts).map(([k, v]) => `| ${k} | ${v} |`),
    '',
    '## Consent cookies',
    '',
    ...report.consentCookies.map(
      (c) =>
        `- t=${c.t} ms ${c.cause}: ${c.name} domain=${c.domain} path=${c.path} secure=${c.secure} httpOnly=${c.httpOnly} sameSite=${c.sameSite} expires=${c.expires}`,
    ),
    '',
    `Guest window.__overwolf__ keys: ${(report.guestOverwolfKeys ?? []).join(', ')}`,
    '',
    `Guest document.referrer: ${report.guestReferrer}`,
    '',
  ];
  writeFileSync(join(runDir, 'report.md'), lines.join('\n'));
  console.log(join(runDir, 'report.md'));
}

main();
