// Ambient module declarations (CONTRACT B.4) for projects that list
// `ow-tauri/types` in `compilerOptions.types` instead of using `paths`.
// This file must stay a script (no top-level import or export).

declare module 'electron' {
  export * from 'ow-tauri/electron';
  export { default } from 'ow-tauri/electron';
}

declare module '@overwolf/ow-electron' {}
