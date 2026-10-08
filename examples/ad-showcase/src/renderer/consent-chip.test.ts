import { describe, expect, it } from 'vitest';

import { consentChip, type ConsentState } from './consent-chip.js';

describe('consentChip', () => {
  it('says each state in plain words', () => {
    expect(consentChip('checking').text).toBe('consent: checking…');
    expect(consentChip('required').text).toBe('consent: EU rules apply');
    expect(consentChip('not-required').text).toBe('consent: not required');
    expect(consentChip('failed').text).toBe('consent: could not check');
  });

  it('never shows API or CMP jargon in the chip text, only in the tooltip', () => {
    const states: ConsentState[] = ['checking', 'required', 'not-required', 'failed'];
    for (const state of states) {
      const chip = consentChip(state);
      expect(chip.text).not.toMatch(/CMP|isCMPRequired|true|false/);
      expect(chip.title).toMatch(/isCMPRequired/);
    }
  });
});
