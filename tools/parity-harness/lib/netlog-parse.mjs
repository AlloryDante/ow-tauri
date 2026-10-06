#!/usr/bin/env node
// Parses a Chromium net log (written by `--log-net-log=<file>
// --net-log-capture-mode=Everything`) into one record per URL request: URL,
// method, the headers as they went on the wire (in order), cookies sent and
// stored, upload body, response status, headers and body.
//
// The net log covers every process of the app, so it also shows requests made
// from ow-electron's main process (analytics, feature flags) that no renderer
// hook can see.
//
// Usage: node lib/netlog-parse.mjs <netlog.json> [out.json]

import { readFileSync, writeFileSync } from 'node:fs';
import { argv } from 'node:process';
import { fileURLToPath } from 'node:url';

const BODY_LIMIT = 64 * 1024;

/**
 * Reads a net log, repairing a file that was cut short (the app was killed
 * before Chromium wrote the closing brackets).
 * @param {string} text
 */
export function readNetlog(text) {
  try {
    return JSON.parse(text);
  } catch {
    const cut = text.lastIndexOf('},\n');
    if (cut < 0) throw new Error('net log is empty or not JSON');
    return JSON.parse(text.slice(0, cut + 1) + ']}');
  }
}

/** Decodes base64 bytes to text when they look like UTF-8 text. */
function decodeBytes(base64) {
  const buf = Buffer.from(base64, 'base64');
  const text = buf.toString('utf8');
  // eslint-disable-next-line no-control-regex
  const binary = /[\u0000-\u0008\u000e-\u001f]/.test(text) || text.includes('�');
  return binary
    ? { base64: buf.toString('base64'), bytes: buf.length }
    : { text, bytes: buf.length };
}

function concatBodies(chunks) {
  const buf = Buffer.concat(chunks.map((c) => Buffer.from(c, 'base64')));
  if (buf.length === 0) return null;
  const capped = buf.subarray(0, BODY_LIMIT);
  const decoded = decodeBytes(capped.toString('base64'));
  return { ...decoded, totalBytes: buf.length, truncated: buf.length > BODY_LIMIT };
}

/**
 * Extracts HTTP/2 DATA frame payloads per stream id from the plaintext bytes a
 * socket sent (SSL_SOCKET_BYTES_SENT events, in order).
 * @param {Buffer} stream
 * @returns {Map<number, Buffer[]>}
 */
function h2DataFrames(stream) {
  const out = new Map();
  const preface = Buffer.from('PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n');
  let offset = stream.subarray(0, preface.length).equals(preface) ? preface.length : 0;
  while (offset + 9 <= stream.length) {
    const length = stream.readUIntBE(offset, 3);
    const type = stream[offset + 3];
    const flags = stream[offset + 4];
    const streamId = stream.readUInt32BE(offset + 5) & 0x7fffffff;
    const end = offset + 9 + length;
    if (end > stream.length) break;
    if (type === 0) {
      let payload = stream.subarray(offset + 9, end);
      if (flags & 0x8) payload = payload.subarray(1, payload.length - payload[0]); // PADDED
      if (!out.has(streamId)) out.set(streamId, []);
      out.get(streamId).push(payload);
    }
    offset = end;
  }
  return out;
}

/**
 * Turns a parsed net log into request records.
 * @param {string | object} input net log text or parsed object
 */
export function parseNetlog(input) {
  const log = typeof input === 'string' ? readNetlog(input) : input;
  const c = log.constants;
  const ev = Object.fromEntries(Object.entries(c.logEventTypes).map(([k, v]) => [v, k]));
  const src = Object.fromEntries(Object.entries(c.logSourceType).map(([k, v]) => [v, k]));
  const offset = Number(c.timeTickOffset);

  /** @type {Map<number, any[]>} */
  const bySource = new Map();
  for (const e of log.events) {
    if (!bySource.has(e.source.id)) bySource.set(e.source.id, []);
    bySource.get(e.source.id).push(e);
  }
  const sourceType = (id) => src[bySource.get(id)?.[0]?.source.type];

  // Plaintext bytes each socket sent, for upload bodies.
  const socketSent = new Map();
  for (const [id, events] of bySource) {
    const chunks = events
      .filter((e) => ev[e.type] === 'SSL_SOCKET_BYTES_SENT' && e.params && e.params.bytes)
      .map((e) => Buffer.from(e.params.bytes, 'base64'));
    if (chunks.length) socketSent.set(id, Buffer.concat(chunks));
  }
  // HTTP/2 sessions: socket and the header blocks they sent, per stream.
  const h2Sessions = [];
  for (const [id, events] of bySource) {
    if (sourceType(id) !== 'HTTP2_SESSION') continue;
    const init = events.find((e) => ev[e.type] === 'HTTP2_SESSION_INITIALIZED');
    const socketId = init?.params?.source_dependency?.id;
    const sends = events
      .filter((e) => ev[e.type] === 'HTTP2_SESSION_SEND_HEADERS')
      .map((e) => ({
        headers: JSON.stringify(e.params.headers),
        time: e.time,
        jobId: e.params.source_dependency?.id,
        streamId: e.params.stream_id,
      }));
    h2Sessions.push({ id, socketId, sends, frames: null });
  }
  const framesOf = (session) => {
    if (!session.frames) {
      const bytes = socketSent.get(session.socketId);
      session.frames = bytes ? h2DataFrames(bytes) : new Map();
    }
    return session.frames;
  };

  const requests = [];
  for (const [id, events] of bySource) {
    if (sourceType(id) !== 'URL_REQUEST') continue;
    const r = {
      id,
      url: null,
      method: null,
      startedAt: new Date(offset + Number(events[0].time)).toISOString(),
      initiator: null,
      requestType: null,
      networkIsolationKey: null,
      siteForCookies: null,
      loadFlags: null,
      protocol: null,
      rendererHeaders: null,
      sentHeaders: null,
      cookiesSent: [],
      cookiesStored: [],
      uploadSize: null,
      uploadBody: null,
      status: null,
      responseHeaders: null,
      responseBody: null,
      redirects: [],
      netError: null,
    };
    const bodyChunks = [];
    let h2StreamId = null;
    let h2SentAt = null;
    const jobIds = [];
    for (const e of events) {
      const name = ev[e.type];
      const p = e.params || {};
      switch (name) {
        case 'REQUEST_ALIVE':
          if (p.url) r.url = p.url;
          if (e.phase === 2 && p.net_error) r.netError = p.net_error;
          break;
        case 'CORS_REQUEST':
          if (p.request_headers) r.rendererHeaders = p.request_headers.headers;
          break;
        case 'URL_REQUEST_START_JOB':
          if (p.url) {
            r.url = p.url;
            r.method = p.method;
            r.initiator = p.initiator;
            r.requestType = p.request_type;
            r.networkIsolationKey = p.network_isolation_key;
            r.siteForCookies = p.site_for_cookies;
            r.loadFlags = p.load_flags;
          }
          break;
        case 'HTTP_TRANSACTION_SEND_REQUEST_HEADERS':
          r.protocol = 'http/1.1';
          r.sentHeaders = p.headers;
          r.requestLine = p.line?.trim();
          break;
        case 'HTTP_TRANSACTION_HTTP2_SEND_REQUEST_HEADERS':
          r.protocol = 'h2';
          r.sentHeaders = p.headers;
          h2SentAt = e.time;
          break;
        case 'HTTP_TRANSACTION_QUIC_SEND_REQUEST_HEADERS':
          r.protocol = 'h3';
          r.sentHeaders = p.headers;
          break;
        case 'HTTP_STREAM_REQUEST_BOUND_TO_JOB':
          if (p.source_dependency) jobIds.push(p.source_dependency.id);
          break;
        case 'HTTP2_STREAM_UPDATE_SEND_WINDOW':
          if (h2StreamId === null && p.stream_id !== undefined) h2StreamId = p.stream_id;
          break;
        case 'COOKIE_INCLUSION_STATUS':
          (p.operation === 'store' ? r.cookiesStored : r.cookiesSent).push({
            name: p.name,
            domain: p.domain,
            path: p.path,
            status: p.status,
          });
          break;
        case 'UPLOAD_DATA_STREAM_INIT':
          if (e.phase === 2) r.uploadSize = p.total_size ?? null;
          break;
        case 'HTTP_TRANSACTION_READ_RESPONSE_HEADERS':
          r.responseHeaders = p.headers;
          r.status = p.headers?.[0] ?? null;
          break;
        case 'URL_REQUEST_REDIRECTED':
        case 'URL_REQUEST_REDIRECT_JOB':
          if (p.location) r.redirects.push(p.location);
          break;
        case 'URL_REQUEST_JOB_FILTERED_BYTES_READ':
          if (p.bytes) bodyChunks.push(p.bytes);
          break;
        case 'FAILED':
          r.netError = p.net_error ?? r.netError;
          break;
        default:
          break;
      }
    }
    if (!r.url) continue;
    r.responseBody = concatBodies(bodyChunks);
    if (r.uploadSize) r.uploadBody = uploadBodyFor(r, h2StreamId, h2SentAt, jobIds);
    requests.push(r);
  }
  requests.sort((a, b) => a.startedAt.localeCompare(b.startedAt) || a.id - b.id);
  return requests;

  function uploadBodyFor(r, streamIdHint, sentAt, jobIds) {
    if (r.protocol === 'h2' && r.sentHeaders) {
      // The session's SEND_HEADERS names the request's HTTP stream job as its
      // source dependency; otherwise match the header block and tick.
      const key = JSON.stringify(r.sentHeaders);
      for (const session of h2Sessions) {
        const send =
          session.sends.find((s) => s.jobId !== undefined && jobIds.includes(s.jobId)) ??
          session.sends.find(
            (s) => s.jobId === undefined && s.headers === key && s.time === sentAt,
          );
        if (!send) continue;
        const streamId = send.streamId ?? streamIdHint;
        const parts = framesOf(session).get(streamId);
        if (parts) return concatBodies(parts.map((b) => b.toString('base64')));
      }
      return { unavailable: 'h2 stream not found' };
    }
    if (r.protocol === 'http/1.1' && r.requestLine) {
      for (const bytes of socketSent.values()) {
        const at = bytes.indexOf(r.requestLine);
        if (at < 0) continue;
        const headerEnd = bytes.indexOf('\r\n\r\n', at);
        if (headerEnd < 0) continue;
        const body = bytes.subarray(headerEnd + 4, headerEnd + 4 + r.uploadSize);
        return concatBodies([body.toString('base64')]);
      }
      return { unavailable: 'http/1.1 bytes not found' };
    }
    return { unavailable: `${r.protocol ?? 'unknown protocol'}: bodies are not in the net log` };
  }
}

/**
 * Counts requests per host and per method + path (query removed).
 * @param {ReturnType<typeof parseNetlog>} requests
 */
export function summarize(requests) {
  const hosts = {};
  const endpoints = {};
  for (const r of requests) {
    let url;
    try {
      url = new URL(r.url);
    } catch {
      continue;
    }
    hosts[url.host] = (hosts[url.host] ?? 0) + 1;
    const key = `${r.method} ${url.protocol}//${url.host}${url.pathname}`;
    endpoints[key] = (endpoints[key] ?? 0) + 1;
  }
  return { total: requests.length, hosts, endpoints };
}

if (argv[1] && fileURLToPath(import.meta.url) === argv[1]) {
  const [, , input, output] = argv;
  if (!input) {
    console.error('Usage: node lib/netlog-parse.mjs <netlog.json> [out.json]');
    process.exit(2);
  }
  const requests = parseNetlog(readFileSync(input, 'utf8'));
  const json = JSON.stringify(requests, null, 2) + '\n';
  if (output) writeFileSync(output, json);
  else process.stdout.write(json);
  console.error(JSON.stringify(summarize(requests), null, 2));
}
