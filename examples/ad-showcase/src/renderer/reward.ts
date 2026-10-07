/**
 * The reward flow (README, "Reward"), as a small state machine
 * (unit-tested in `reward.test.ts`):
 *
 * 1. The `rewarded-ad;` slot (at least 400x300) is mounted visible and
 *    preloads until `video_ad_ready`.
 * 2. Then it is hidden (`display: none`) and the "Watch ad" button is
 *    enabled.
 * 3. The click shows the slot; the ad page sees hidden then visible and plays.
 * 4. On `complete` after a `play` from the same element since its last
 *    `complete`, the reward is granted once, and the slot is hidden again for
 *    the next preload.
 *
 * The grant is purely client-side: Overwolf documents no server-side
 * verification or postback for reward ads (an open question to Overwolf).
 *
 * @packageDocumentation
 */

/** Steps of the stepper. */
export type RewardStep = 'preloading' | 'ready' | 'playing' | 'completed' | 'granted';

/** Every step, in stepper order. */
export const REWARD_STEPS: readonly RewardStep[] = [
  'preloading',
  'ready',
  'playing',
  'completed',
  'granted',
];

/** What the page must do after an input. */
export interface RewardEffect {
  /** Hide the slot (`display: none`). */
  hide?: boolean;
  /** Show the slot. */
  show?: boolean;
  /** Grant the reward now (exactly once per play cycle). */
  grant?: boolean;
}

/** The reward state machine. */
export class RewardFlow {
  #step: RewardStep = 'preloading';
  #playedSinceComplete = false;
  #watchRequested = false;
  #grants = 0;

  /** The current step. */
  get step(): RewardStep {
    return this.#step;
  }

  /** How many rewards were granted. */
  get grants(): number {
    return this.#grants;
  }

  /** Whether the "Watch ad" button is enabled. */
  get canWatch(): boolean {
    return this.#step === 'ready' && !this.#watchRequested;
  }

  /**
   * The user pressed "Watch ad".
   *
   * @returns the effect: show the slot, or nothing when not ready
   */
  watch(): RewardEffect {
    if (!this.canWatch) return {};
    this.#watchRequested = true;
    return { show: true };
  }

  /**
   * An event from the rewarded element.
   *
   * @param name - the event name
   * @returns the effect
   */
  onEvent(name: string): RewardEffect {
    switch (name) {
      case 'video_ad_ready':
        if (this.#step === 'playing') return {};
        this.#step = 'ready';
        this.#watchRequested = false;
        return { hide: true };
      case 'play':
        this.#playedSinceComplete = true;
        this.#step = 'playing';
        return {};
      case 'complete': {
        if (!this.#playedSinceComplete) {
          this.#step = 'completed';
          return {};
        }
        this.#playedSinceComplete = false;
        this.#watchRequested = false;
        this.#grants += 1;
        this.#step = 'granted';
        return { grant: true, hide: true };
      }
      default:
        return {};
    }
  }
}

/** Seconds without `video_ad_ready` after which a reward slot counts as unavailable. */
export const UNAVAILABLE_AFTER_S = 10;
