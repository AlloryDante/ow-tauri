# Architecture decision records

Each record captures one decision: the context, the decision, its
consequences and the alternatives we rejected. After the first release,
records are immutable; a later record supersedes an earlier one and both link
to each other. Until then a record may be amended in place, and each amendment
is listed in its "Amendments" section.

| # | Title | Status |
|---|---|---|
| [0001](0001-hidden-main-webview.md) | Run main-process code in a hidden main webview | Accepted |
| [0002](0002-electron-subset-alias.md) | Provide an Electron-compatible subset behind a bundler alias | Accepted |
| [0003](0003-owadview-native-child-webviews.md) | Implement `<owadview>` with a MutationObserver and native child webviews | Accepted (amended) |
| [0004](0004-packages-backend-selection.md) | Packages: report them as unavailable, defer the package runtime | Accepted (amended) |
| [0005](0005-ads-test-live-parity.md) | Keep ow-electron's test/live ad semantics | Accepted (amended) |
| [0006](0006-analytics-labelling.md) | Send ow-electron's analytics, labelled "tauri" through one setting | Accepted (amended) |
| [0007](0007-state-file-continuity.md) | Share ow-electron's per-app state directory, file encoding and uid | Accepted (amended) |
| [0008](0008-updater-client.md) | Ship an electron-updater compatible update client | Accepted (amended) |
| [0009](0009-main-webview-liveness-and-lifecycle.md) | Keep the main webview alive and give it one lifecycle | Accepted |
| [0010](0010-per-webview-ipc-channels.md) | Deliver host messages over one IPC channel per webview | Accepted |
| [0011](0011-remote-guest-ipc.md) | Give each remote guest one scoped, rate-limited command | Accepted (amended) |
| [0012](0012-js-runtime-singleton.md) | One injected JS runtime per webview, with thin npm facades | Accepted |
| [0013](0013-request-shaping-per-os.md) | Shape ad guest requests like ow-electron, per OS | Accepted |
| [0014](0014-machine-id-parity.md) | Derive the machine id exactly as ow-electron does | Accepted |
| [0015](0015-startup-consent-window.md) | Run ow-electron's hidden startup consent window on every launch | Accepted |
| [0016](0016-signing-approach.md) | Sign Tauri builds with Overwolf's published flow, never fake integrity | Accepted |

Template: copy any record, keep the headings (Status, Date, Context, Decision,
Consequences, Alternatives considered, and Amendments when there are any).
