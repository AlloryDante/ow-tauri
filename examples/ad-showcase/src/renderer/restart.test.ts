import { describe, expect, it } from 'vitest';

import { restartRefusal } from './restart.js';

describe('restartRefusal', () => {
  it('shows the ow-tauri reason as it is', () => {
    expect(restartRefusal('Restart needs a built app: run `npm run start:tauri`.')).toBe(
      'Restart refused: Restart needs a built app: run `npm run start:tauri`.',
    );
  });

  it('drops the Electron invoke prefix', () => {
    const error = new Error(
      "Error invoking remote method 'showcase:restart': TypeError: mode must be test or live",
    );
    expect(restartRefusal(error)).toBe('Restart refused: mode must be test or live');
  });

  it('never shows an empty reason', () => {
    expect(restartRefusal('')).toBe('Restart refused: unknown reason');
    expect(restartRefusal(undefined)).toBe('Restart refused: undefined');
  });
});
