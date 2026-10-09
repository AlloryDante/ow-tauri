# Spike: macOS mechanisms (W0c-A)

A minimal Tauri 2.12.1 app (`unstable`, guests as child webviews, as the
plugin builds them) that measures the macOS mechanisms the plugin design
relies on, on real `WKWebView`s. One item per process:

| `SPIKE_ITEM` | Question |
|---|---|
| `ua` | Guarded KVC `userAgent` vs `customUserAgent` vs async `navigator.userAgent` vs the template, with and without an app-set custom UA |
| `gesture` | `NSEvent` local monitor + `hitTest:` arming; a click on the app under a pass-through child; Return/Space with the guest first responder; script-only opens |
| `zoom` | `devicePixelRatio / scale_factor` under `pageZoom` 0.8 / 1.25 |
| `close` | After a non-prevented `CloseRequested`, does a check posted through the event-loop proxy run while the guests are alive and can still run `evaluateJavaScript`? Also prevented, close-to-tray, `destroy()` and a late check |
| `crash` | `kill -9` of a guest's WebContent process: `evaluateJavaScript("1")` result with (`SPIKE_HOOK=1`) and without Tauri's terminate hook; a throttled (hidden window) page; a hung page |
| `storage` | `sessionStorage` carry-over across a guest recreate: snapshot, recreate with the same label, restore before page scripts, one-shot |

Guests load only pages from a loopback fixture server inside the app
(`fixtures/`, 127.0.0.1, random port). The spike loads no ads and does not use the network.

## Run (macOS, invisible lab)

```sh
T=/some/scratch/target
CARGO_TARGET_DIR=$T cargo build -j 4
./run-mac.sh $T/out $T/debug/mac-mechanics-spike            # all items
./run-mac.sh $T/out $T/debug/mac-mechanics-spike gesture    # one item
```

Every window is alpha 0, shadowless and ignores mouse events at the window
server; the app uses the Accessory policy; `activate` /
`activateIgnoringOtherApps:` do nothing and `makeKeyAndOrderFront:` only
orders an invisible window in. Input never leaves the process: mouse and key
events are built here and given to this app's own
`-[NSApplication sendEvent:]` (the path that runs local event monitors).
Because the app is never active, this process alone is told which window is
key and that the app is active. `run-mac.sh` proves each run
`everVisible:false` (`tools/parity-harness/lib/window-monitor.swift`) and
`everFront:false` (`lsappinfo front`).

Knobs: `SPIKE_HOOK=1` (crash: install `Builder::on_web_content_process_terminate`;
`run-mac.sh` item `crash-hook`), `SPIKE_PROBE_DELAY_MS` (crash: probe delay
after the kill, default 300), `SPIKE_CLOSE_VARIANTS` (close: space-separated
`close`/`prevent`/`tray`/`destroy`/`late`/`gcd`; every non-prevented
variant also evaluates a hide beacon at `WindowEvent::Destroyed` through
retained guest handles), `SPIKE_DELAY_MS` (wait before the
driver starts), `LLDB=1` (attach lldb in `run-mac.sh`).

Lab-only measurements use WebKit SPI (`_webProcessIdentifier` to find the
WebContent pid); the plugin design does not.

## CI

`.github/workflows/spike-mac-mechanics.yml` (manual `workflow_dispatch`
only) runs every item on `macos-14` and `macos-15` and uploads `out/`.
