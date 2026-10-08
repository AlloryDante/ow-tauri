# `ow-tauri/renderer`

The renderer runtime of the app's UI windows (`bw-*` webviews). The plugin
injects it into every UI window, so `<owadview>` works even if the app never
imports this module; importing it gives typed access to the same objects.

Generated reference: the `renderer` module in `packages/ow-tauri/docs-out`
([how to build it](README.md#build-the-reference)).

| Export | What it is | Specification |
|---|---|---|
| `ipcRenderer`, `contextBridge` | the same objects as in `ow-tauri/electron` | [CONTRACT B.2.3](../CONTRACT.md#b23-ipcmain-main-and-ipcrenderer-preload-and-renderer), [B.2.4](../CONTRACT.md#b24-contextbridge-preload) |
| `owadview` | `upgrade(element)` and `elements()`, for tests | [CONTRACT B.3](../CONTRACT.md#b3-ow-taurirenderer) |
| `AdviewAttributes`, `AdviewRect` | types of the element's attributes and box | [CONTRACT B.3.2](../CONTRACT.md#b32-attributes) |

The `<owadview>` element itself (attributes, methods, events, lifecycle) is
specified in [CONTRACT B.3](../CONTRACT.md#b3-ow-taurirenderer); the ad
formats and how to use each are in [AD-FORMATS.md](../AD-FORMATS.md).
