/**
 * The top bar's consent chip: `isCMPRequired()` in plain words. Pure, so it
 * is unit-tested in `consent-chip.test.ts`.
 *
 * @packageDocumentation
 */

/** Where the `isCMPRequired()` check is. */
export type ConsentState = 'checking' | 'required' | 'not-required' | 'failed';

/** What the chip shows. */
export interface ConsentChip {
  /** The chip text. */
  text: string;
  /** The tone class suffix (`tone-<tone>`). */
  tone: 'muted' | 'info' | 'ok' | 'warn';
  /** The tooltip: what the state means for the app. */
  title: string;
}

/**
 * The chip for a consent state.
 *
 * @param state - the check's state
 * @returns text, tone and tooltip
 *
 * @example
 * ```ts
 * consentChip('required').text; // 'consent: EU rules apply'
 * ```
 */
export function consentChip(state: ConsentState): ConsentChip {
  switch (state) {
    case 'checking':
      return {
        text: 'consent: checking…',
        tone: 'muted',
        title: 'Waiting for app.overwolf.isCMPRequired().',
      };
    case 'required':
      return {
        text: 'consent: EU rules apply',
        tone: 'info',
        title:
          'isCMPRequired() is true: the user is under consent rules (GDPR). Offer "Ad privacy settings" in the app; page 8 opens it.',
      };
    case 'not-required':
      return {
        text: 'consent: not required',
        tone: 'ok',
        title: 'isCMPRequired() is false: no consent prompt is needed for this user.',
      };
    case 'failed':
      return {
        text: 'consent: could not check',
        tone: 'warn',
        title: 'isCMPRequired() failed; it normally never rejects.',
      };
  }
}
