# Spike: does Tauri `unstable` break text input?

`Window::add_child` (how ow-tauri hosts ad guests) needs Tauri's `unstable`
feature. Cargo unifies features, so the plugin turns it on for the whole app,
and that changes every `WebviewWindow`: its webview is built as a *child* of
the native window instead of the window's content (`tauri-runtime-wry`
`WebviewKind::WindowChild`). This spike checks what that does to typing in
the app's own pages.

A minimal Tauri 2.12.1 app: one window with six text fields (input, textarea,
contenteditable, a contenteditable that cancels `beforeinput` like Draft.js or
Lexical, an input that cancels `keypress`, an input that cancels `keydown`) and
four focus patterns (refocus on keydown, node swap on the first key, focus
handed to a second input, type-anywhere). With `SPIKE_MODE=unstable-child`, one
blank local child webview sits beside them. **It is not an ad**, and no ads are
loaded.

| Mode | Build | Child webview |
|---|---|---|
| `stable` | `cargo build` | none |
| `unstable-nochild` | `cargo build --features unstable` | none |
| `unstable-child` | `cargo build --features unstable` | one, 300×600 at x=600 |

A driver thread clicks each field, types (ASCII with Shift, arrows including
presses at the field's edges, Backspace past empty, dead keys, Enter, Escape, a
function key, and on Windows Unicode packets too), reads back the value and the
DOM event log, and writes `SPIKE_OUT` (JSON).

## macOS (invisible lab)

```sh
T=/some/scratch/target
CARGO_TARGET_DIR=$T/stable   cargo build
CARGO_TARGET_DIR=$T/unstable cargo build --features unstable
./run-mac.sh $T/out $T/stable/debug/unstable-input-spike $T/unstable/debug/unstable-input-spike
```

The window is alpha 0 and click-through, the app uses the Accessory policy, and
`activate` / `makeKeyAndOrderFront:` are neutralised, so the app never shows and
is never frontmost. `run-mac.sh` proves both, using `window-monitor` from
`tools/parity-harness/lib` and `lsappinfo front`. Input never leaves the
process. Each key is a `CGEvent` from a private event source, never posted to
the window server, wrapped as an `NSEvent` and given to the app's own
`-[NSApplication sendEvent:]`. Because the app is never active, this process
alone is told that the spike window is key and the app is active, and
`sendEvent:` routes key events the way AppKit does for an active app's key
window. WebKit re-sends keys the page did not handle through that same
`sendEvent:`.

Knobs: `SPIKE_DELIVERY=window` (straight to `-[NSWindow sendEvent:]`),
`SPIKE_EVENT_SOURCE=ns`, `SPIKE_ARROW_STRINGS=function-keys`,
`SPIKE_TRACE_ALL=1` (logs which views receive `keyDown:`), and
`SPIKE_MITIGATE=native|js` (the two mitigations under test).

## Windows (CI)

`.github/workflows/spike-unstable.yml` (manual `workflow_dispatch` only) runs
the three modes on `windows-2025`. It sets the US-International layout for dead
keys, types with `SendInput`, adds an Alt-Tab-and-back check against a second
window, and uploads `out/` as the `spike-unstable-windows` artifact.
