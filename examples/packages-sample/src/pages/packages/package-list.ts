/**
 * The Overwolf packages of the upstream sample and why none of them is on
 * Tauri: ow-electron loads them into its patched Electron at run time, and
 * Tauri has no such package runtime. tauri-plugin-overwolf has no API for
 * them in 1.0 (no stand-ins either).
 *
 * @packageDocumentation
 */

/** One package of the upstream sample. */
export interface PackageInfo {
  /** The ow-electron package name. */
  readonly id: 'gep' | 'overlay' | 'recorder' | 'utility';
  /** The display name. */
  readonly name: string;
  /** What it does on ow-electron. */
  readonly does: string;
  /** Why it is not available on Tauri. */
  readonly reason: string;
}

/** The packages, in the upstream sample's order. */
export const PACKAGES: readonly PackageInfo[] = [
  {
    id: 'gep',
    name: 'Game Events Provider (GEP)',
    does: 'Live game events and info for supported games.',
    reason: "Runs inside ow-electron's package runtime, which has no Tauri equivalent.",
  },
  {
    id: 'overlay',
    name: 'Overlay',
    does: 'Windows drawn inside the game, with hotkeys and exclusive input.',
    reason:
      "Injects into the game through ow-electron's patched Electron; Tauri windows cannot be hosted there.",
  },
  {
    id: 'recorder',
    name: 'Recorder',
    does: 'Game capture, replays and audio tracks.',
    reason:
      "A native capture package of ow-electron's package runtime, which has no Tauri equivalent.",
  },
  {
    id: 'utility',
    name: 'Utility',
    does: 'Installed-game scans and the high-elevation helper that lets the overlay reach elevated games.',
    reason: "Part of ow-electron's package runtime, which has no Tauri equivalent.",
  },
];
