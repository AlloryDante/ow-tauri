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

function walk(root, dir, outDir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
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
    const stat = statSync(full);
    const record = { path: rel, size: stat.size, mtime: stat.mtime.toISOString(), copied: false };
    if (stat.size <= COPY_LIMIT) {
      const bytes = readFileSync(full);
      record.sha256 = createHash('sha256').update(bytes).digest('hex');
      const target = join(outDir, 'files', rel);
      mkdirSync(dirname(target), { recursive: true });
      copyFileSync(full, target);
      record.copied = true;
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
