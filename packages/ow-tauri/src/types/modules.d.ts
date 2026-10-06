// Ambient module declarations (CONTRACT B.4) for projects that list
// `ow-tauri/types` in `compilerOptions.types`. TypeScript prefers an ambient
// module over a `paths` mapping, so this declaration must match
// `electron.d.ts`: the facade plus the `app.overwolf` augmentation.
// This file must stay a script (no top-level import or export).

declare module 'electron' {
  import type * as facade from 'ow-tauri/electron';

  export * from 'ow-tauri/electron';

  /**
   * Electron's `app` with ow-electron's augmentation: `app.overwolf` is the
   * global `overwolf.OverwolfApi`, so `@overwolf/ow-electron-packages-types`
   * applies (`app.overwolf.packages.gep`, `.recorder`, ...).
   */
  export type ElectronApp = facade.App & { readonly overwolf: overwolf.OverwolfApi };

  /** Electron's `app` (CONTRACT B.2.1), typed with the `app.overwolf` augmentation. */
  export const app: ElectronApp;

  const electron: Omit<typeof facade.default, 'app'> & { readonly app: ElectronApp };
  export default electron;
}

declare module '@overwolf/ow-electron' {}
