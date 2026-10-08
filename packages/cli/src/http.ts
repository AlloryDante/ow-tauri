/// <reference types="node" />
/**
 * HTTP calls of the signing step, shaped like Overwolf's builder: JSON and
 * multipart posts with `Authorization: Key <email>:<apiKey>` and
 * `x-ow-app-key`, and plain downloads.
 *
 * @packageDocumentation
 */

import { createReadStream } from 'node:fs';
import { stat } from 'node:fs/promises';
import * as http from 'node:http';
import * as https from 'node:https';
import { basename } from 'node:path';

/** Signing service credentials (`OW_CLI_EMAIL`, `OW_CLI_API_KEY`, `OW_BUILD_KEY`). */
export interface Credentials {
  /** `OW_CLI_EMAIL`. */
  readonly email: string;
  /** `OW_CLI_API_KEY`. */
  readonly apiKey: string;
  /** `OW_BUILD_KEY`, sent as `x-ow-app-key`. */
  readonly appKey: string;
}

/** The largest response body read into memory (256 MiB). */
const MAX_BODY_BYTES = 256 * 1024 * 1024;
/** Redirects followed by a download. */
const MAX_REDIRECTS = 5;
/** Socket inactivity limit. */
const TIMEOUT_MS = 120_000;

/**
 * Whether `url` may be used: `https`, or `http` on a loopback host (local
 * test servers).
 *
 * @param url - the URL
 * @returns `true` when allowed
 */
export function allowedUrl(url: URL): boolean {
  if (url.protocol === 'https:') return true;
  return (
    url.protocol === 'http:' &&
    (url.hostname === 'localhost' || url.hostname === '127.0.0.1' || url.hostname === '[::1]')
  );
}

function transport(url: URL): typeof http | typeof https {
  return url.protocol === 'https:' ? https : http;
}

function headers(creds: Credentials): Record<string, string> {
  return { Authorization: `Key ${creds.email}:${creds.apiKey}`, 'x-ow-app-key': creds.appKey };
}

/** The server's error text: its JSON `message`, else the raw body. */
function serverMessage(body: Buffer): string {
  const raw = body.toString('utf8');
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed === 'object' && parsed !== null && 'message' in parsed) {
      const message = parsed.message;
      if (typeof message === 'string') return message;
    }
  } catch {
    // Not JSON: the raw text is the message.
  }
  return raw;
}

function collect(
  res: http.IncomingMessage,
  label: string,
  resolve: (b: Buffer) => void,
  reject: (e: Error) => void,
): void {
  const chunks: Buffer[] = [];
  let size = 0;
  res.on('data', (chunk: Buffer) => {
    size += chunk.length;
    if (size > MAX_BODY_BYTES) {
      res.destroy(new Error(`${label} response is too large`));
      return;
    }
    chunks.push(chunk);
  });
  res.on('error', reject);
  res.on('end', () => {
    const body = Buffer.concat(chunks);
    const status = res.statusCode ?? 0;
    if (status >= 400) {
      reject(new Error(`${label} returned ${String(status)}: ${serverMessage(body)}`));
    } else {
      resolve(body);
    }
  });
}

/**
 * `POST` a JSON body (the builder's `postJson`).
 *
 * @param url - the endpoint
 * @param body - the JSON body
 * @param creds - the credentials
 * @returns the response body
 */
export function postJson(url: URL, body: unknown, creds: Credentials): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    if (!allowedUrl(url)) {
      reject(new Error(`[OW] refusing a non-https signing URL: ${url.origin}`));
      return;
    }
    const payload = Buffer.from(JSON.stringify(body), 'utf8');
    const req = transport(url).request(
      url,
      {
        method: 'POST',
        timeout: TIMEOUT_MS,
        headers: {
          'Content-Type': 'application/json',
          'Content-Length': String(payload.byteLength),
          ...headers(creds),
        },
      },
      (res) => {
        collect(res, '[OW] Signing API', resolve, reject);
      },
    );
    req.on('timeout', () => req.destroy(new Error('[OW] Signing API timed out')));
    req.on('error', reject);
    req.end(payload);
  });
}

/**
 * `POST` a file as multipart field `file` (the builder's `postFile`).
 *
 * @param url - the endpoint
 * @param filePath - the file to send
 * @param creds - the credentials
 * @returns the response body
 */
export async function postFile(url: URL, filePath: string, creds: Credentials): Promise<Buffer> {
  if (!allowedUrl(url)) throw new Error(`[OW] refusing a non-https signing URL: ${url.origin}`);
  let size: number;
  try {
    size = (await stat(filePath)).size;
  } catch (error) {
    throw new Error(`[OW] cannot read executable to sign: ${(error as Error).message}`, {
      cause: error,
    });
  }
  const boundary = `----owBuilderBoundary${Date.now().toString(16)}${Math.random().toString(16).slice(2)}`;
  const preamble = Buffer.from(
    `--${boundary}\r\n` +
      `Content-Disposition: form-data; name="file"; filename="${basename(filePath)}"\r\n` +
      `Content-Type: application/octet-stream\r\n\r\n`,
    'utf8',
  );
  const epilogue = Buffer.from(`\r\n--${boundary}--\r\n`, 'utf8');
  return new Promise((resolve, reject) => {
    const req = transport(url).request(
      url,
      {
        method: 'POST',
        timeout: TIMEOUT_MS,
        headers: {
          'Content-Type': `multipart/form-data; boundary=${boundary}`,
          'Content-Length': String(preamble.byteLength + size + epilogue.byteLength),
          ...headers(creds),
        },
      },
      (res) => {
        collect(res, '[OW] Certificate signing API', resolve, reject);
      },
    );
    req.on('timeout', () => req.destroy(new Error('[OW] Certificate signing API timed out')));
    req.on('error', reject);
    req.write(preamble);
    const stream = createReadStream(filePath);
    stream.on('error', (error) => {
      req.destroy();
      reject(new Error(`[OW] failed reading executable to sign: ${error.message}`));
    });
    stream.on('end', () => req.end(epilogue));
    stream.pipe(req, { end: false });
  });
}

/**
 * `GET` a URL into memory, following up to five redirects to allowed URLs.
 *
 * @param url - the URL
 * @param redirects - redirects followed so far
 * @returns the body
 */
export function download(url: URL, redirects = 0): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    if (!allowedUrl(url)) {
      reject(new Error(`refusing a non-https download URL: ${url.origin}`));
      return;
    }
    const req = transport(url).get(url, { timeout: TIMEOUT_MS }, (res) => {
      const status = res.statusCode ?? 0;
      const location = res.headers.location;
      if (status >= 300 && status < 400 && location !== undefined) {
        res.resume();
        if (redirects >= MAX_REDIRECTS) {
          reject(new Error('too many redirects'));
          return;
        }
        download(new URL(location, url), redirects + 1).then(resolve, reject);
        return;
      }
      collect(res, 'download', resolve, reject);
    });
    req.on('timeout', () => req.destroy(new Error('download timed out')));
    req.on('error', reject);
  });
}
