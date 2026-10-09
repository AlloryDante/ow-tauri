#!/usr/bin/env node
// WebView2 minimum-version check (.github/workflows/windows-lab.yml, job
// "WebView2 minimum"): reads one ow-tauri capture that ran on a fixed-version
// WebView2 Runtime (WEBVIEW2_BROWSER_EXECUTABLE_FOLDER) and checks the
// plugin's floor (W0c ruling 5, `WEBVIEW2_MINIMUM` = 98.0.1108.44):
//
//   supported   - a runtime at or above the floor: the app ran on that
//                 runtime, ad guests were created and a test ad loaded.
//   unsupported - a runtime below the floor: the app ran on that runtime
//                 and no ad guest was ever created (ads fail closed).
//
//   node ci/webview2-min.mjs --capture <dir> --runtime 98.0.1108.56 --expect supported
//
// Prints one JSON verdict and exits 1 when the expectation is not met.

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

/** The message `adview_mount` rejects with below the floor (host/ads.rs). */
export const UNSUPPORTED_MESSAGE = 'ads need the WebView2 Runtime 98.0.1108.44 or newer';

/** JSON lines of `file`, or [] when it is missing. */
function jsonLines(file) {
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

/**
 * The facts of one capture the check needs.
 * @param {string} dir
 */
export function captureFacts(dir) {
  let webview = null;
  try {
    webview =
      JSON.parse(readFileSync(join(dir, 'overwolf.json'), 'utf8'))?.versions?.webview ?? null;
  } catch {
    // no snapshot: the app did not get as far as its page
  }
  const events = jsonLines(join(dir, 'events.jsonl'));
  const pageEvents = jsonLines(join(dir, 'page-events.jsonl'));
  const guests = new Set(
    [...events, ...pageEvents]
      .map((e) => e.webContentsId ?? e.label)
      .filter((id) => typeof id === 'string' && id.startsWith('owad-')),
  );
  const adsLoaded = pageEvents.filter((e) => e.event === 'display_ad_loaded').length;
  const messageSeen = existsSync(dir)
    ? readdirSync(dir)
        .filter((f) => /\.(jsonl?|log)$/.test(f))
        .some((f) => readFileSync(join(dir, f), 'utf8').includes(UNSUPPORTED_MESSAGE))
    : false;
  return { webview, guests: [...guests].sort(), adsLoaded, messageSeen };
}

/**
 * The verdict for `facts` against the expectation.
 * @param {ReturnType<typeof captureFacts>} facts
 * @param {{runtime: string, expect: 'supported' | 'unsupported'}} o
 */
export function verdict(facts, { runtime, expect }) {
  const problems = [];
  if (facts.webview !== runtime)
    problems.push(`the app ran on WebView2 ${facts.webview ?? '(unknown)'}, not ${runtime}`);
  if (expect === 'supported') {
    if (facts.guests.length === 0) problems.push('no ad guest was created');
    if (facts.adsLoaded === 0) problems.push('no test ad loaded (display_ad_loaded)');
  } else if (facts.guests.length > 0) {
    problems.push(`ad guests were created below the floor: ${facts.guests.join(', ')}`);
  }
  return { ok: problems.length === 0, expect, runtime, ...facts, problems };
}

function main() {
  const { values } = parseArgs({
    options: {
      capture: { type: 'string' },
      runtime: { type: 'string' },
      expect: { type: 'string' },
    },
  });
  if (
    !values.capture ||
    !values.runtime ||
    !['supported', 'unsupported'].includes(values.expect ?? '')
  ) {
    console.error(
      'usage: webview2-min.mjs --capture <dir> --runtime <version> --expect supported|unsupported',
    );
    process.exit(2);
  }
  const result = verdict(captureFacts(resolve(values.capture)), {
    runtime: values.runtime,
    expect: /** @type {'supported' | 'unsupported'} */ (values.expect),
  });
  console.log(JSON.stringify(result, null, 2));
  process.exit(result.ok ? 0 : 1);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
