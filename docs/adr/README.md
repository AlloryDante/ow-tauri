# Architecture decision records

Each record captures one decision: the context, the decision, its
consequences and the alternatives we rejected. Records are immutable once
accepted; a later record supersedes an earlier one and both link to each other.

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

Template: copy any record, keep the headings (Status, Date, Context, Decision,
Consequences, Alternatives considered).
