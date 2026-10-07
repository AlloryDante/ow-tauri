import { describe, expect, it } from 'vitest';

import { REWARD_STEPS, RewardFlow, UNAVAILABLE_AFTER_S } from './reward.js';

describe('RewardFlow', () => {
  it('preloads visible, hides on video_ad_ready and enables Watch', () => {
    const flow = new RewardFlow();
    expect(flow.step).toBe('preloading');
    expect(flow.canWatch).toBe(false);
    expect(flow.watch()).toEqual({});
    expect(flow.onEvent('video_ad_ready')).toEqual({ hide: true });
    expect(flow.step).toBe('ready');
    expect(flow.canWatch).toBe(true);
  });

  it('shows on Watch, grants once on complete after play, then hides', () => {
    const flow = new RewardFlow();
    flow.onEvent('video_ad_ready');
    expect(flow.watch()).toEqual({ show: true });
    expect(flow.canWatch).toBe(false);
    expect(flow.watch()).toEqual({});
    expect(flow.onEvent('play')).toEqual({});
    expect(flow.step).toBe('playing');
    expect(flow.onEvent('complete')).toEqual({ grant: true, hide: true });
    expect(flow.grants).toBe(1);
    expect(flow.step).toBe('granted');
    // A second complete without a new play grants nothing.
    expect(flow.onEvent('complete')).toEqual({});
    expect(flow.grants).toBe(1);
    expect(flow.step).toBe('completed');
  });

  it('does not grant a complete that never played', () => {
    const flow = new RewardFlow();
    flow.onEvent('video_ad_ready');
    expect(flow.onEvent('complete')).toEqual({});
    expect(flow.grants).toBe(0);
  });

  it('ignores video_ad_ready while playing, and re-arms after a grant', () => {
    const flow = new RewardFlow();
    flow.onEvent('video_ad_ready');
    flow.watch();
    flow.onEvent('play');
    expect(flow.onEvent('video_ad_ready')).toEqual({});
    expect(flow.step).toBe('playing');
    flow.onEvent('complete');
    expect(flow.onEvent('video_ad_ready')).toEqual({ hide: true });
    expect(flow.canWatch).toBe(true);
    flow.watch();
    flow.onEvent('play');
    flow.onEvent('complete');
    expect(flow.grants).toBe(2);
  });

  it('ignores other events', () => {
    const flow = new RewardFlow();
    expect(flow.onEvent('impression')).toEqual({});
    expect(flow.step).toBe('preloading');
  });

  it('lists the stepper steps and the unavailable delay', () => {
    expect(REWARD_STEPS).toEqual(['preloading', 'ready', 'playing', 'completed', 'granted']);
    expect(UNAVAILABLE_AFTER_S).toBe(10);
  });
});
