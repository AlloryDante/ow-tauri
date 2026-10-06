// Copies the hand-written B.4 declarations (src/types/*.d.ts) to dist/types;
// tsc does not emit .d.ts inputs.
import { copyFileSync, mkdirSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const from = join(root, 'src', 'types');
const to = join(root, 'dist', 'types');
mkdirSync(to, { recursive: true });
for (const name of readdirSync(from)) {
  if (name.endsWith('.d.ts')) copyFileSync(join(from, name), join(to, name));
}
