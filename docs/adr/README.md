# Architecture decision records

Each record captures one decision: the context, the decision, its
consequences and the alternatives we rejected. After the first release,
records are immutable. A later record supersedes an earlier one, and both
link to each other. Until then a record may be amended in place, and each
amendment is listed in its "Amendments" section.

Records 0001 to 0016 describe the first design, which emulated Electron.
[ADR 0017](0017-tauri-native-pivot.md) replaced it with a Tauri-native
plugin. Superseded records are kept for history; read the record that
supersedes them first.

| # | Title | Status |
|---|---|---|
| [0001](0001-hidden-main-webview.md) | Run main-process code in a hidden main webview | Superseded by 0017 |
| [0002](0002-electron-subset-alias.md) | Provide an Electron-compatible subset behind a bundler alias | Superseded by 0017 |
| [0003](0003-owadview-native-child-webviews.md) | Implement `<owadview>` with a MutationObserver and native child webviews | Accepted (amended) |
| [0004](0004-packages-backend-selection.md) | Packages: report them as unavailable, defer the package runtime | Accepted (amended: no packages API in 1.0) |
| [0005](0005-ads-test-live-parity.md) | Keep ow-electron's test/live ad semantics | Accepted (amended) |
| [0006](0006-analytics-labelling.md) | Send ow-electron's analytics, labelled "tauri" through one setting | Accepted (amended) |
| [0007](0007-state-file-continuity.md) | Share ow-electron's per-app state directory, file encoding and uid | Accepted (amended) |
| [0008](0008-updater-client.md) | Ship an electron-updater compatible update client | Accepted (amended: Windows only, Tauri updater shape) |
| [0009](0009-main-webview-liveness-and-lifecycle.md) | Keep the main webview alive and give it one lifecycle | Superseded by 0017, 0018 |
| [0010](0010-per-webview-ipc-channels.md) | Deliver host messages over one IPC channel per webview | Superseded by 0017 |
| [0011](0011-remote-guest-ipc.md) | Give each remote guest one scoped, rate-limited command | Accepted (amended by 0020) |
| [0012](0012-js-runtime-singleton.md) | One injected JS runtime per webview, with thin npm facades | Superseded by 0017 |
| [0013](0013-request-shaping-per-os.md) | Shape ad guest requests like ow-electron, per OS | Accepted |
| [0014](0014-machine-id-parity.md) | Derive the machine id exactly as ow-electron does | Accepted |
| [0015](0015-startup-consent-window.md) | Run ow-electron's hidden startup consent window on every launch | Accepted (amended) |
| [0016](0016-signing-approach.md) | Sign Tauri builds with Overwolf's published flow, never fake integrity | Accepted |
| [0017](0017-tauri-native-pivot.md) | Adapt Overwolf's SDK to Tauri instead of emulating Electron | Accepted |
| [0018](0018-lifecycle-ready-exit.md) | Start at `RunEvent::Ready`, drain at `RunEvent::Exit`, never hold the exit | Accepted |
| [0019](0019-window-tracking-and-naming.md) | Track app windows from native Tauri events and name them like ow-electron | Accepted |
| [0020](0020-native-gesture-authority.md) | Let only native user activation open the system browser from an ad | Accepted |
| [0021](0021-package-split.md) | Ship two crates and two npm packages, each with one job | Accepted |
| [0022](0022-http-clients.md) | Keep hyper for Overwolf's endpoints and reqwest for the updater | Accepted |
| [0023](0023-unstable-and-macos-input.md) | Use Tauri's `unstable` child webviews, and repair macOS key input | Accepted |
| [0024](0024-recreate-on-reload.md) | Recreate a macOS ad guest on reload instead of reloading in place | Accepted |

Template: copy any record and keep its headings: Status, Date, Context,
Decision, Consequences, Alternatives considered, and Amendments when there
are any.
