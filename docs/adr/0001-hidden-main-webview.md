# ADR 0001: Run main-process code in a hidden main webview

- Status: Accepted
- Date: 2026-10-06

## Context

In ow-electron every Overwolf API lives in the Electron main process:
`import { app } from 'electron'; app.overwolf...`. The packages are Node
`EventEmitter`s, and the official sample's controllers and services (several
thousand lines of TypeScript under `src/browser/**`) are written against that
model. Tauri's main process is Rust; there is no JavaScript context outside
webviews.

To keep "swap the host, not the app", ow-tauri needs a JavaScript context that
can run that code with the same object graph.

## Decision

The plugin creates one hidden, never-focused window with label `ow-main`
whose webview loads the app's bundled main-process code. `ow-tauri/main`
provides `app.overwolf`, and `ow-tauri/electron` provides `app`,
`BrowserWindow`, `ipcMain` and the other main-process modules (ADR 0002).
This webview is the only one with the broad `overwolf:main` permission set.

Synchronous APIs are served from a snapshot injected as an initialization
script before any page script runs, plus ordered state patches pushed from
Rust (CONTRACT B.1.6).

## Consequences

- App code keeps its structure: controllers, services and event wiring port
  with import changes only.
- Node built-ins are unavailable. `fs`, `path`, `child_process` and `os` uses
  must be replaced (`files` from `ow-tauri/main`, a `path` polyfill, the opener
  plugin). The port map lists each one.
- One extra webview costs memory (one renderer process on WebView2, shared web
  content process pools elsewhere). It never paints.
- Everything privileged sits in one webview with a local origin, which makes
  the capability model simple to audit (ARCHITECTURE section 5).
- Startup gains one hop: UI windows exist only after the main webview has run.
- Hidden webviews are throttled by every engine, and `ow-main` must not be.
  How it stays alive, and what happens on reload, crash and quit, is
  [ADR 0009](0009-main-webview-liveness-and-lifecycle.md).

## Alternatives considered

- **Node sidecar running the original main-process code.** Closest to
  Electron, but reintroduces Node (size, signing, a second runtime to update)
  and still needs a bridge for every Electron API. Rejected.
- **Rewrite main-process logic in Rust.** Maximum performance, but every app
  would have to rewrite its controllers. Rejected as the default; Rust-first
  apps can still use `OverwolfExt` directly.
- **Run main-process code inside the first UI window.** Couples app lifetime to
  a visible window and gives a renderer the main permission set. Rejected.

## Amendments

- 2026-10-06, contract review: liveness and lifecycle moved to ADR 0009
  (consequence added above).
