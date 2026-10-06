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
| [0003](0003-owadview-native-child-webviews.md) | Implement `<owadview>` with a MutationObserver and native child webviews | Accepted |
| [0004](0004-packages-backend-selection.md) | Select the packages backend: native, simulated or none | Accepted |
| [0005](0005-ads-test-live-parity.md) | Keep ow-electron's test/live ad semantics | Accepted |
| [0006](0006-analytics-labelling.md) | Label analytics honestly as a Tauri host | Accepted |
| [0007](0007-state-file-continuity.md) | Share ow-electron's per-app state directory and uid | Accepted |
| [0008](0008-updater-client.md) | Ship an electron-updater compatible update client | Accepted |
| [0009](0009-main-webview-liveness-and-lifecycle.md) | Keep the main webview alive and give it one lifecycle | Accepted |
| [0010](0010-per-webview-ipc-channels.md) | Deliver host messages over one IPC channel per webview | Accepted |
| [0011](0011-remote-guest-ipc.md) | Give each remote guest one scoped, rate-limited command | Accepted |
| [0012](0012-js-runtime-singleton.md) | One injected JS runtime per webview, with thin npm facades | Accepted |

Template: copy any record, keep the headings (Status, Date, Context, Decision,
Consequences, Alternatives considered, and Amendments when there are any).
