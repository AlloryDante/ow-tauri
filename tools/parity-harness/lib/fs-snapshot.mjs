// Records the files under a directory (path, size, mtime, sha256) and copies
// the small ones, so before/after snapshots show what a run wrote.

import { createHash } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join, relative } from 'node:path';

const SKIP_DIRS = new Set([
  'Cache',
  'Code Cache',
  'GPUCache',
  'DawnGraphiteCache',
  'DawnWebGPUCache',
  'Crashpad',
]);
const COPY_LIMIT = 1024 * 1024;

/**
 * @param {string} root directory to snapshot (may not exist)
 * @param {string} outDir where to write `manifest.json` and copies
 * @returns {{root: string, exists: boolean, files: Array<{path: string, size: number, mtime: string, sha256?: string, copied: boolean}>}}
 */
export function snapshotDir(root, outDir) {
  const result = { root, exists: existsSync(root), files: [] };
  if (result.exists) walk(root, root, outDir, result.files);
  mkdirSync(outDir, { recursive: true });
  writeFileSync(join(outDir, 'manifest.json'), JSON.stringify(result, null, 2) + '\n');
  return result;
}

/**
 * Whether `error` is a file that went away or is held open while the
 * snapshot reads it: on Windows the WebView2 browser processes outlive the
 * app for a moment and remove or lock their files (`lockfile`) meanwhile.
 * @param {unknown} error
 */
export function isTransientFsError(error) {
  const code = /** @type {{code?: unknown}} */ (error)?.code;
  return code === 'ENOENT' || code === 'EBUSY' || code === 'EPERM';
}

function walk(root, dir, outDir, files) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch (error) {
    if (isTransientFsError(error)) return;
    throw error;
  }
  for (const entry of entries) {
    const full = join(dir, entry.name);
    const rel = relative(root, full);
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) {
        files.push({ path: rel + '/', size: 0, mtime: '', copied: false, skipped: true });
        continue;
      }
      walk(root, full, outDir, files);
      continue;
    }
    if (!entry.isFile()) continue;
    let stat;
    try {
      stat = statSync(full);
    } catch (error) {
      if (isTransientFsError(error)) continue;
      throw error;
    }
    const record = { path: rel, size: stat.size, mtime: stat.mtime.toISOString(), copied: false };
    if (stat.size <= COPY_LIMIT) {
      try {
        const bytes = readFileSync(full);
        record.sha256 = createHash('sha256').update(bytes).digest('hex');
        const target = join(outDir, 'files', rel);
        mkdirSync(dirname(target), { recursive: true });
        copyFileSync(full, target);
        record.copied = true;
      } catch (error) {
        if (!isTransientFsError(error)) throw error;
        // Listed with its size, not copied: it was locked or went away.
      }
    }
    files.push(record);
  }
}

/**
 * Lists added, removed and changed files between two snapshots.
 * @param {ReturnType<typeof snapshotDir>} before
 * @param {ReturnType<typeof snapshotDir>} after
 */
export function diffSnapshots(before, after) {
  const map = (s) => new Map(s.files.map((f) => [f.path, f]));
  const a = map(before);
  const b = map(after);
  const added = [...b.keys()].filter((k) => !a.has(k));
  const removed = [...a.keys()].filter((k) => !b.has(k));
  const changed = [...b.keys()].filter(
    (k) => a.has(k) && (a.get(k).sha256 !== b.get(k).sha256 || a.get(k).size !== b.get(k).size),
  );
  return { root: after.root, added, removed, changed };
}
