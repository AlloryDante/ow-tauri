import { describe, expect, it, vi } from 'vitest';

import { restartApp } from './restart';

describe('restartApp', () => {
  it('asks the app to restart in the given mode', async () => {
    const call = vi.fn().mockResolvedValue(undefined);
    await expect(restartApp('live', call)).resolves.toBeNull();
    expect(call).toHaveBeenCalledWith('sample_restart', { mode: 'live' });
  });

  it('answers the refusal text (the tauri dev notice)', async () => {
    const call = vi.fn().mockRejectedValue('Restart needs a built app');
    await expect(restartApp('test', call)).resolves.toBe('Restart needs a built app');
  });

  it('turns any other rejection into text', async () => {
    const call = vi.fn().mockRejectedValue(new Error('gone'));
    await expect(restartApp('test', call)).resolves.toBe('Error: gone');
  });
});
