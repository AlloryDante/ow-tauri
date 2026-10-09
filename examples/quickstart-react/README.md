# Quickstart (React)

The React variant of [the vanilla quickstart](../quickstart-vanilla/README.md):
an app created with `npm create tauri-app@latest -- --template react-ts`,
then the same Rust, configuration and capability steps. One window, one
400x300 Overwolf ad, test ads on, under `React.StrictMode`.

- [src/main.tsx](src/main.tsx) imports `tauri-plugin-overwolf-api/adview`
  once, before React renders: it installs the `<owadview>` element runtime.
- [src/AdSlot.tsx](src/AdSlot.tsx) renders `<owadview>` (typed by
  `import type {} from 'tauri-plugin-overwolf-api/jsx'`) and attaches its
  listeners with a `ref` and `addEventListener`: the ad events are DOM
  events with underscore names (`display_ad_loaded`), which React's `on*`
  props do not map. StrictMode runs the effect twice; the element stays
  mounted, so the ad loads once.
- [src/App.tsx](src/App.tsx) reads `getInfo()` and `isCMPRequired()` and
  shows the ad privacy settings button only when consent rules apply.

## Run

From the repository root, `npm install` once. Then, from
`examples/quickstart-react`: `npm run tauri dev` (hot reload) or `npm start`
(a debug build with the page embedded, [scripts/run.mjs](scripts/run.mjs)).
