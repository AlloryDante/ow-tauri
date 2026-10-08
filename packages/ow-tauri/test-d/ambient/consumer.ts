// Type-level check of `compilerOptions.types: ["ow-tauri/types"]` (CONTRACT
// B.4): the ambient `electron` module wins over `paths`, so it must carry the
// same `app.overwolf` augmentation as dist/types/electron.d.ts. The
// `@overwolf/ow-electron` path stays, as documented: it keeps an installed
// ow-electron's typings out of the program (CB-1, src/typings-coexist.test.ts).
/// <reference path="../../src/types/index.d.ts" />
import type { OverwolfGameEventPackage } from '@overwolf/ow-electron-packages-types';
import electron, { app } from 'electron';

export function ambient(): void {
  const gep: OverwolfGameEventPackage | undefined = app.overwolf.packages.gep;
  const legacy = app as overwolf.OverwolfApp;
  const dir: string = legacy.getPath('userData');
  const name: string = legacy.name;
  const api: overwolf.OverwolfApi = electron.app.overwolf;
  void [gep, dir, name, api];
}
