# ADR 0010: Deliver host messages over one IPC channel per webview

- Status: Superseded by ADR 0017 (2026-10-08)
- Date: 2026-10-06

## Context

The first contract draft sent everything from Rust to webviews as Tauri
events with `emit_to`: IPC requests to `ow-main`, `webContents.send` messages
to UI windows, state patches, package events and eval requests.

Reading Tauri 2.12.1 shows that `emit_to` does not isolate webviews. The
manager loops over every webview, and a JS listener registered with the
default target (`listen()` from `@tauri-apps/api/event`) matches every emit,
whatever its target. UI windows needed `core:event:allow-listen`, so any script
in any UI window could read every other window's IPC traffic, the main
webview's invoke requests and its state. Event payloads are also evaluated as
script on the receiving side, which is slow for large messages.

Two ordering problems sat on top: an invoke's reply came back as the command
response while `webContents.send` messages came as events, so Electron's rule
that messages sent inside a handler arrive before its reply did not hold; and
nothing distinguished one document's sequence numbers from the next after a
reload.

## Decision

- Each runtime calls `ipc_subscribe(onMessage: Channel<HostMessage[]>)` once
  per document. Rust stores the channel by the calling webview's label and
  sends everything for that webview through it. Tauri events are not used,
  and no capability grants `core:event:*`.
- `ipc_invoke` only acknowledges (`{ id }`); the result arrives as an
  `ipc-result` message on the sender's channel, ordered with the main
  runtime's `webContents.send` calls to that window by one per-target
  sequence.
- `ipc_subscribe` returns a random epoch; every request carries
  `{ epoch, seq }`. Stale epochs are rejected; gaps caused by calls Tauri
  rejected before the command ran are reported with `ipc_skip`.
- Messages queued for a webview during one event-loop turn go out as one
  array.
- Bounds: invokes in flight per sender, messages queued per receiver, and
  encoded size, each with a defined error (`ipc-overloaded`,
  `ipc-serialization`).
- Destroyed webviews release their channel, buffers and pending requests.

## Consequences

- A webview can receive only its own messages; a mock-runtime test asserts
  that webview B never sees a message for webview A.
- Large payloads use Tauri's channel transport instead of script evaluation.
  Since the GHSA-w28w-mhc8-qvjv fix (Tauri 2.11.6 / 2.12.0), channel data
  fetches are bound to the webview that owns the channel.
- Electron's reply-after-messages ordering holds.
- The renderer and main runtimes need a little more code (pending maps keyed
  by request id, epoch handling).

## Alternatives considered

- **Keep events, register listeners with an explicit webview target.** The
  plugin's own listeners would be targeted, but nothing stops another script
  in the page from registering an `Any` listener, so isolation would still
  depend on page code. Rejected.
- **Return invoke results as command responses.** Simple, but breaks the
  ordering guarantee. Rejected.

## Amendments

- 2026-10-08, Tauri-native rewrite: superseded by [ADR 0017](0017-tauri-native-pivot.md). There is no IPC router and there are no host messages. App webviews call Tauri commands. Each `<owadview>` mount gets its own Tauri `Channel` for its element events (CONTRACT B.3.5).
