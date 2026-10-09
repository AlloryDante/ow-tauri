/**
 * Installs `window.showcase` (see `showcase-api.ts`) and the `<owadview>`
 * element runtime before the showcase page starts. The Tauri entry
 * (`index.ts`) imports this first.
 *
 * @packageDocumentation
 */
import 'tauri-plugin-overwolf-api/adview';

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  generateUserEmailHashes,
  isCMPRequired,
  openAdPrivacySettingsWindow,
} from 'tauri-plugin-overwolf-api';

import type { WindowEvent } from '../shared/ipc.js';
import { createShowcaseApi } from './showcase-api.js';

window.showcase = createShowcaseApi({
  invoke: (command, args) => invoke(command, args),
  listen: (event, handler) => listen<WindowEvent>(event, handler),
  isCMPRequired,
  openAdPrivacySettingsWindow: () => openAdPrivacySettingsWindow(),
  generateUserEmailHashes: async (email) => ({ ...(await generateUserEmailHashes(email)) }),
});
