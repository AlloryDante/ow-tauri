import { describe, expect, it } from 'vitest';

import { PACKAGES } from './package-list';

describe('PACKAGES', () => {
  it('lists the four upstream packages, each with a one-line reason', () => {
    expect(PACKAGES.map((p) => p.id)).toEqual(['gep', 'overlay', 'recorder', 'utility']);
    for (const p of PACKAGES) {
      expect(p.reason.length).toBeGreaterThan(0);
      expect(p.reason).not.toContain('\n');
    }
  });
});
