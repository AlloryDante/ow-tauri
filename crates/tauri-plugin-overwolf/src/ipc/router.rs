//! The IPC router (CONTRACT C): a pure state machine.
//!
//! The router knows peers by webview label. It never touches Tauri: commands
//! call into it, it queues [`HostMessage`]s per peer, and the host drains the
//! queues once per event-loop turn and sends each as one channel message. Time
//! is a millisecond counter passed in by the caller, so every rule is
//! testable.
//!
//! - Inbound (`ipc_invoke`, `ipc_send`) is reordered per sender and epoch
//!   (C.3), held in the startup queue until `ipc_main_ready` (C.4), then
//!   queued for `ow-main`.
//! - Outbound (`ipc_emit`, `ipc_reply`) is reordered per target window by the
//!   main runtime's shared per-target counter (C.5).
//! - Bounds: in-flight invokes per sender, queued messages per receiver, the
//!   encoded size of one message, the startup queue (C.4).
//!
//! ```
//! use serde_json::json;
//! use tauri_plugin_overwolf::config::IpcConfig;
//! use tauri_plugin_overwolf::ipc::router::{Router, SenderInfo, MAIN_LABEL};
//! let mut r = Router::new(&IpcConfig::default());
//! r.subscribe(MAIN_LABEL, "m1", 0);
//! r.main_ready(0);
//! r.subscribe("bw-1", "e1", 0);
//! let sender = SenderInfo { label: "bw-1".into(), window_id: 1, url: "tauri://localhost/".into() };
//! let id = r.invoke(&sender, "e1", 1, "get", vec![json!(1)], 10, 0).unwrap();
//! let out = r.drain();
//! assert_eq!(out[0].0, MAIN_LABEL);
//! r.reply(id, true, Some(json!("ok")), None, 1, 1);
//! let out = r.drain();
//! assert_eq!(out[0].0, "bw-1");
//! ```

use std::collections::{BTreeMap, HashMap, VecDeque};

use serde_json::Value;

use super::messages::{HostMessage, IpcMessage, IpcSender};
use super::reorder::{Reorder, ReorderError};
use crate::config::IpcConfig;
use crate::error::Error;

/// The main webview's label.
pub const MAIN_LABEL: &str = "ow-main";
/// Longest channel name, in UTF-16 code units (C.2).
pub const MAX_CHANNEL_UNITS: usize = 256;
/// The gap fallback: how long a reorder buffer waits for a missing number.
pub const GAP_MS: u64 = 1000;
/// Retired request ids remembered so a late reply still consumes its
/// sequence number.
pub const RETIRED_CAP: usize = 65_536;

/// The router's limits (C.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// 0 = no timeout.
    pub invoke_timeout_ms: u64,
    /// Startup queue length.
    pub startup_queue_max: usize,
    /// Startup queue entry lifetime.
    pub startup_timeout_ms: u64,
    /// Encoded size cap per message.
    pub max_message_bytes: usize,
    /// In-flight invokes per sender.
    pub max_in_flight_invokes: usize,
    /// Queued messages per receiver.
    pub max_queued_messages: usize,
}

impl From<&IpcConfig> for Limits {
    fn from(c: &IpcConfig) -> Self {
        Limits {
            invoke_timeout_ms: c.invoke_timeout_ms,
            startup_queue_max: c.startup_queue_max,
            startup_timeout_ms: c.startup_timeout_ms,
            max_message_bytes: c.max_message_bytes,
            max_in_flight_invokes: c.max_in_flight_invokes,
            max_queued_messages: c.max_queued_messages,
        }
    }
}

/// Who is calling `ipc_invoke` / `ipc_send`, taken from the calling webview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderInfo {
    /// Webview label (`bw-<id>`).
    pub label: String,
    /// Electron-style window id.
    pub window_id: u32,
    /// Current document URL.
    pub url: String,
}

/// A router log line; the host forwards these to the log file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterLog {
    /// `true` for warnings, `false` for debug lines.
    pub warn: bool,
    /// The message (no payloads, no user data).
    pub message: String,
}

#[derive(Debug, Clone)]
enum Inbound {
    Invoke {
        id: u64,
        channel: String,
        args: Vec<Value>,
        sender: SenderInfo,
    },
    Send {
        channel: String,
        args: Vec<Value>,
        sender: SenderInfo,
    },
}

#[derive(Debug, Clone)]
enum Outbound {
    Message {
        label: String,
        channel: String,
        args: Vec<Value>,
    },
    Result {
        label: String,
        id: u64,
        ok: bool,
        value: Option<Value>,
        error: Option<Value>,
    },
    /// A reply whose request was already answered (timeout, restart).
    Consumed,
}

#[derive(Debug)]
struct Peer {
    epoch: Option<String>,
    subscribed: bool,
    inbound: Reorder<Inbound>,
    in_flight: usize,
    outbox: Vec<HostMessage>,
}

impl Peer {
    fn new(window: u64) -> Self {
        Peer {
            epoch: None,
            subscribed: false,
            inbound: Reorder::new(window),
            in_flight: 0,
            outbox: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct Pending {
    label: String,
    window_id: u32,
    deadline: Option<u64>,
}

/// The router state.
#[derive(Debug)]
pub struct Router {
    limits: Limits,
    next_id: u64,
    peers: HashMap<String, Peer>,
    main_ready: bool,
    startup: VecDeque<(u64, Inbound)>,
    pending: HashMap<u64, Pending>,
    retired: HashMap<u64, u32>,
    retired_order: VecDeque<u64>,
    outbound: HashMap<u32, Reorder<Outbound>>,
    logs: Vec<RouterLog>,
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn channel_error(channel: &str) -> Option<Error> {
    let n = utf16_len(channel);
    if n == 0 || n > MAX_CHANNEL_UNITS {
        Some(Error::invalid_argument(
            "The channel must be 1 to 256 characters long.",
        ))
    } else {
        None
    }
}

fn reorder_error(e: ReorderError) -> Error {
    match e {
        ReorderError::Zero | ReorderError::Duplicate => {
            Error::invalid_argument("Duplicate or invalid sequence number.")
        }
        ReorderError::TooFarAhead => {
            Error::ipc_overloaded("Too many messages are waiting for an earlier one.")
        }
    }
}

impl Router {
    /// A router with the limits from `config`.
    #[must_use]
    pub fn new(config: &IpcConfig) -> Self {
        Router {
            limits: Limits::from(config),
            next_id: 1,
            peers: HashMap::new(),
            main_ready: false,
            startup: VecDeque::new(),
            pending: HashMap::new(),
            retired: HashMap::new(),
            retired_order: VecDeque::new(),
            outbound: HashMap::new(),
            logs: Vec::new(),
        }
    }

    /// The limits in force.
    #[must_use]
    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    fn window(&self) -> u64 {
        self.limits.max_queued_messages as u64
    }

    fn peer(&mut self, label: &str) -> &mut Peer {
        let window = self.window();
        self.peers
            .entry(label.to_owned())
            .or_insert_with(|| Peer::new(window))
    }

    fn warn(&mut self, message: String) {
        self.logs.push(RouterLog {
            warn: true,
            message,
        });
    }

    fn debug(&mut self, message: String) {
        self.logs.push(RouterLog {
            warn: false,
            message,
        });
    }

    /// Takes the accumulated log lines.
    pub fn take_logs(&mut self) -> Vec<RouterLog> {
        std::mem::take(&mut self.logs)
    }

    /// Whether `label` has a subscribed channel.
    #[must_use]
    pub fn is_subscribed(&self, label: &str) -> bool {
        self.peers.get(label).is_some_and(|p| p.subscribed)
    }

    /// The current epoch of `label`.
    #[must_use]
    pub fn epoch(&self, label: &str) -> Option<&str> {
        self.peers.get(label).and_then(|p| p.epoch.as_deref())
    }

    /// Whether `ipc_main_ready` has been received for the current `ow-main`
    /// document.
    #[must_use]
    pub fn is_main_ready(&self) -> bool {
        self.main_ready
    }

    /// Requests in flight from `label`.
    #[must_use]
    pub fn in_flight(&self, label: &str) -> usize {
        self.peers.get(label).map_or(0, |p| p.in_flight)
    }

    /// Messages queued for `label` and not yet drained.
    #[must_use]
    pub fn queued(&self, label: &str) -> usize {
        let base = self.peers.get(label).map_or(0, |p| p.outbox.len());
        if label == MAIN_LABEL {
            base + self.startup.len()
        } else {
            base
        }
    }

    /// The sizes of every buffer and table, for the lab's `core-stats.jsonl`
    /// (a growing number over an idle run is a leak).
    #[cfg_attr(
        not(feature = "plugin"),
        expect(dead_code, reason = "only the plugin's host records it")
    )]
    pub(crate) fn lab_stats(&self) -> Value {
        let peers: serde_json::Map<String, Value> = self
            .peers
            .iter()
            .map(|(label, p)| {
                (
                    label.clone(),
                    serde_json::json!({
                        "outbox": p.outbox.len(),
                        "inboundHeld": p.inbound.held_len(),
                        "inFlight": p.in_flight,
                    }),
                )
            })
            .collect();
        serde_json::json!({
            "peers": peers,
            "startup": self.startup.len(),
            "pending": self.pending.len(),
            "retired": self.retired.len(),
            "outbound": self.outbound.len(),
            "outboundHeld": self.outbound.values().map(Reorder::held_len).sum::<usize>(),
            "logs": self.logs.len(),
        })
    }

    // ----- retirement -------------------------------------------------------

    fn retire(&mut self, id: u64, window_id: u32) {
        self.retired.insert(id, window_id);
        self.retired_order.push_back(id);
        while self.retired_order.len() > RETIRED_CAP {
            if let Some(old) = self.retired_order.pop_front() {
                self.retired.remove(&old);
            }
        }
    }

    /// Removes a pending invoke, retires it and, when `error` is given,
    /// queues an `ipc-result` error for its sender.
    fn finish_pending(&mut self, id: u64, error: Option<&Error>) {
        let Some(p) = self.pending.remove(&id) else {
            return;
        };
        self.retire(id, p.window_id);
        if let Some(peer) = self.peers.get_mut(&p.label) {
            peer.in_flight = peer.in_flight.saturating_sub(1);
            if let Some(err) = error {
                peer.outbox.push(HostMessage::IpcResult {
                    id,
                    ok: false,
                    value: None,
                    error: serde_json::to_value(err).ok(),
                });
            }
        }
    }

    fn drop_pending_of(&mut self, label: &str) {
        let ids: Vec<u64> = self
            .pending
            .iter()
            .filter(|(_, p)| p.label == label)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.finish_pending(id, None);
        }
        self.startup.retain(|(_, item)| match item {
            Inbound::Invoke { sender, .. } => sender.label != label,
            Inbound::Send { .. } => true,
        });
    }

    // ----- subscription -----------------------------------------------------

    /// `ipc_subscribe`: registers `label`'s channel with a fresh `epoch`.
    ///
    /// For `ow-main`, a second subscription is a restart: every pending
    /// invoke is rejected with `not-ready`, the startup queue and the
    /// outbound order are reset and `ipc_main_ready` is required again. For a
    /// UI webview, a second subscription resets its inbound order and drops
    /// its pending invokes; messages buffered for it are kept and delivered
    /// on the new channel.
    pub fn subscribe(&mut self, label: &str, epoch: &str, now: u64) {
        let _ = now;
        let was_subscribed = self.peers.get(label).is_some_and(|p| p.epoch.is_some());
        if label == MAIN_LABEL {
            if was_subscribed {
                self.restart_main();
            }
        } else if was_subscribed {
            self.drop_pending_of(label);
        }
        let window = self.window();
        let peer = self.peer(label);
        peer.epoch = Some(epoch.to_owned());
        peer.subscribed = true;
        peer.inbound = Reorder::new(window);
    }

    /// A webview started loading a new document: its channel is gone. The
    /// old epoch becomes stale and messages buffered for it are dropped, as a
    /// renderer reload does in Electron; later messages buffer until the next
    /// `ipc_subscribe`.
    pub fn document_unloaded(&mut self, label: &str) {
        if label == MAIN_LABEL {
            self.restart_main();
            if let Some(main) = self.peers.get_mut(MAIN_LABEL) {
                main.subscribed = false;
                main.epoch = None;
            }
            return;
        }
        let window = self.window();
        if let Some(peer) = self.peers.get_mut(label) {
            peer.subscribed = false;
            peer.epoch = None;
            peer.outbox.clear();
            peer.inbound = Reorder::new(window);
        }
        self.drop_pending_of(label);
    }

    /// Rejects everything that involves the current `ow-main` document and
    /// resets the main-side state (A.6 soft restart, C.2).
    pub fn restart_main(&mut self) {
        let err = Error::not_ready("The main webview restarted.");
        let ids: Vec<u64> = self.pending.keys().copied().collect();
        for id in ids {
            self.finish_pending(id, Some(&err));
        }
        self.startup.clear();
        self.outbound.clear();
        self.retired.clear();
        self.retired_order.clear();
        self.main_ready = false;
        if let Some(main) = self.peers.get_mut(MAIN_LABEL) {
            main.outbox.clear();
        }
    }

    /// A webview was destroyed: drop its channel, buffers, order state and
    /// pending invokes (A.3).
    pub fn remove_peer(&mut self, label: &str, window_id: Option<u32>) {
        self.drop_pending_of(label);
        self.peers.remove(label);
        if let Some(id) = window_id {
            self.outbound.remove(&id);
        }
    }

    // ----- inbound ----------------------------------------------------------

    fn check_epoch(&self, label: &str, epoch: &str) -> Result<(), Error> {
        match self.peers.get(label) {
            Some(p) if p.subscribed && p.epoch.as_deref() == Some(epoch) => Ok(()),
            _ => Err(Error::not_ready(
                "The calling document is not subscribed (stale epoch).",
            )),
        }
    }

    fn check_size(&self, size: usize) -> Result<(), Error> {
        if size > self.limits.max_message_bytes {
            Err(Error::ipc_serialization(
                "The message is larger than ipc.maxMessageBytes.",
            ))
        } else {
            Ok(())
        }
    }

    /// `ipc_invoke`: accepts a request and returns its id (C.2).
    ///
    /// # Errors
    ///
    /// `not-ready` (stale epoch), `invalid-argument` (channel, duplicate
    /// sequence number), `ipc-serialization` (size), `ipc-overloaded`
    /// (in-flight cap, reorder window).
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors the command's wire arguments"
    )]
    pub fn invoke(
        &mut self,
        sender: &SenderInfo,
        epoch: &str,
        seq: u64,
        channel: &str,
        args: Vec<Value>,
        size: usize,
        now: u64,
    ) -> Result<u64, Error> {
        self.check_epoch(&sender.label, epoch)?;
        if let Some(e) = channel_error(channel) {
            return Err(e);
        }
        self.check_size(size)?;
        let max_in_flight = self.limits.max_in_flight_invokes;
        let peer = self.peer(&sender.label);
        peer.inbound.check(seq).map_err(reorder_error)?;
        if peer.in_flight >= max_in_flight {
            return Err(Error::ipc_overloaded(
                "Too many invokes are in flight from this window.",
            ));
        }
        let id = self.next_id;
        self.next_id += 1;
        let deadline =
            (self.limits.invoke_timeout_ms > 0).then(|| now + self.limits.invoke_timeout_ms);
        self.pending.insert(
            id,
            Pending {
                label: sender.label.clone(),
                window_id: sender.window_id,
                deadline,
            },
        );
        let item = Inbound::Invoke {
            id,
            channel: channel.to_owned(),
            args,
            sender: sender.clone(),
        };
        let peer = self.peer(&sender.label);
        peer.in_flight += 1;
        let released = peer.inbound.insert(seq, item, now).map_err(reorder_error)?;
        self.deliver_inbound(released, now);
        Ok(id)
    }

    /// `ipc_send`: accepts a message (C.3).
    ///
    /// # Errors
    ///
    /// As [`Router::invoke`], with `ipc-overloaded` when `ow-main` has too
    /// many messages queued.
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors the command's wire arguments"
    )]
    pub fn send(
        &mut self,
        sender: &SenderInfo,
        epoch: &str,
        seq: u64,
        channel: &str,
        args: Vec<Value>,
        size: usize,
        now: u64,
    ) -> Result<(), Error> {
        self.check_epoch(&sender.label, epoch)?;
        if let Some(e) = channel_error(channel) {
            return Err(e);
        }
        self.check_size(size)?;
        if self.queued(MAIN_LABEL) >= self.limits.max_queued_messages {
            return Err(Error::ipc_overloaded(
                "The main webview has too many messages queued.",
            ));
        }
        let item = Inbound::Send {
            channel: channel.to_owned(),
            args,
            sender: sender.clone(),
        };
        let released = self
            .peer(&sender.label)
            .inbound
            .insert(seq, item, now)
            .map_err(reorder_error)?;
        self.deliver_inbound(released, now);
        Ok(())
    }

    /// `ipc_skip`: the call carrying `seq` never reached the plugin (C.3).
    /// Stale epochs are ignored.
    pub fn skip(&mut self, label: &str, epoch: &str, seq: u64, now: u64) {
        if self.check_epoch(label, epoch).is_err() {
            return;
        }
        let released = self.peer(label).inbound.skip(seq, now);
        self.deliver_inbound(released, now);
    }

    fn to_message(item: Inbound) -> HostMessage {
        let stamp = |s: SenderInfo| IpcSender {
            window_id: s.window_id,
            label: s.label,
            url: s.url,
            frame_id: 0,
        };
        HostMessage::Ipc(match item {
            Inbound::Invoke {
                id,
                channel,
                args,
                sender,
            } => IpcMessage::Invoke {
                id,
                channel,
                args,
                sender: stamp(sender),
            },
            Inbound::Send {
                channel,
                args,
                sender,
            } => IpcMessage::Send {
                channel,
                args,
                sender: stamp(sender),
            },
        })
    }

    fn deliver_inbound(&mut self, items: Vec<Inbound>, now: u64) {
        for item in items {
            if self.main_ready {
                self.peer(MAIN_LABEL).outbox.push(Self::to_message(item));
            } else if self.startup.len() >= self.limits.startup_queue_max {
                self.reject_queued(item, "The startup queue is full.");
            } else {
                self.startup.push_back((now, item));
            }
        }
    }

    fn reject_queued(&mut self, item: Inbound, why: &str) {
        match item {
            Inbound::Invoke { id, channel, .. } => {
                let err =
                    Error::not_ready(why).with_data(serde_json::json!({ "channel": channel }));
                self.finish_pending(id, Some(&err));
            }
            Inbound::Send { channel, .. } => {
                self.warn(format!("dropped ipc send on '{channel}': {why}"));
            }
        }
    }

    /// `ipc_main_ready`: flushes the startup queue to `ow-main` (C.4).
    pub fn main_ready(&mut self, now: u64) {
        let _ = now;
        self.main_ready = true;
        let queued: Vec<Inbound> = self.startup.drain(..).map(|(_, item)| item).collect();
        let main = self.peer(MAIN_LABEL);
        main.outbox.extend(queued.into_iter().map(Self::to_message));
    }

    // ----- outbound ---------------------------------------------------------

    fn outbound_order(&mut self, window_id: u32) -> &mut Reorder<Outbound> {
        let window = self.window();
        self.outbound
            .entry(window_id)
            .or_insert_with(|| Reorder::new(window))
    }

    fn deliver_outbound(&mut self, items: Vec<Outbound>) {
        for item in items {
            match item {
                Outbound::Message {
                    label,
                    channel,
                    args,
                } => {
                    self.peer(&label)
                        .outbox
                        .push(HostMessage::Ipc(IpcMessage::Message { channel, args }));
                }
                Outbound::Result {
                    label,
                    id,
                    ok,
                    value,
                    error,
                } => {
                    if let Some(peer) = self.peers.get_mut(&label) {
                        peer.outbox.push(HostMessage::IpcResult {
                            id,
                            ok,
                            value,
                            error,
                        });
                    }
                }
                Outbound::Consumed => {}
            }
        }
    }

    /// `ipc_emit`: `webContents.send` to window `target_id` (C.5). `target`
    /// is the label of its `bw-*` webview, or `None` when the window is
    /// remote, destroyed or unknown; the message is then dropped with a
    /// warning and `Ok` is returned.
    ///
    /// A rejected message still uses up its `seq` (it is skipped), so later
    /// messages and replies to the window are not held back for the gap
    /// timeout.
    ///
    /// # Errors
    ///
    /// `ipc-serialization` (size), `ipc-overloaded` (target queue full or
    /// reorder window), `invalid-argument` (channel, duplicate `seq`).
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors the command's wire arguments"
    )]
    pub fn emit(
        &mut self,
        target_id: u32,
        target: Option<&str>,
        seq: u64,
        channel: &str,
        args: Vec<Value>,
        size: usize,
        now: u64,
    ) -> Result<(), Error> {
        let Some(label) = target else {
            self.warn(format!(
                "dropped webContents.send on '{channel}' to window {target_id}: not a local window"
            ));
            self.emit_skip(target_id, false, seq, now);
            return Ok(());
        };
        let checked = channel_error(channel)
            .map_or_else(|| self.check_size(size), Err)
            .and_then(|()| {
                let held = self.outbound.get(&target_id).map_or(0, Reorder::held_len);
                if self.queued(label) + held >= self.limits.max_queued_messages {
                    Err(Error::ipc_overloaded(
                        "The target window has too many messages queued.",
                    ))
                } else {
                    Ok(())
                }
            });
        if let Err(e) = checked {
            self.emit_skip(target_id, true, seq, now);
            return Err(e);
        }
        let item = Outbound::Message {
            label: label.to_owned(),
            channel: channel.to_owned(),
            args,
        };
        let released = self
            .outbound_order(target_id)
            .insert(seq, item, now)
            .map_err(reorder_error)?;
        self.deliver_outbound(released);
        Ok(())
    }

    /// `ipc_emit_skip`: outbound `seq` to window `target_id` will never be
    /// sent (the main runtime could not deliver the message or reply that
    /// carried it). `local` says whether the window is a local window the
    /// plugin knows; for other windows only an existing order is advanced.
    /// Numbers already used, skipped, or out of window are ignored.
    pub fn emit_skip(&mut self, target_id: u32, local: bool, seq: u64, now: u64) {
        let order = if local {
            Some(self.outbound_order(target_id))
        } else {
            self.outbound.get_mut(&target_id)
        };
        if let Some(order) = order {
            let released = order.skip(seq, now);
            self.deliver_outbound(released);
        }
    }

    /// `ipc_reply`: the answer to request `id` (C.2). Unknown ids are
    /// ignored; a reply to a request that already timed out still consumes
    /// its `seq`. When the reply cannot be ordered (duplicate `seq`, or too
    /// far ahead), the invoke is settled at once with `ipc-overloaded`, so
    /// it never hangs.
    pub fn reply(
        &mut self,
        id: u64,
        ok: bool,
        value: Option<Value>,
        error: Option<Value>,
        seq: u64,
        now: u64,
    ) {
        let (window_id, item) = if let Some(p) = self.pending.remove(&id) {
            if let Some(peer) = self.peers.get_mut(&p.label) {
                peer.in_flight = peer.in_flight.saturating_sub(1);
            }
            self.retire(id, p.window_id);
            (
                p.window_id,
                Outbound::Result {
                    label: p.label,
                    id,
                    ok,
                    value,
                    error,
                },
            )
        } else if let Some(window_id) = self.retired.remove(&id) {
            self.debug(format!("late or repeated ipc reply {id} dropped"));
            (window_id, Outbound::Consumed)
        } else {
            self.debug(format!("ipc reply {id} for unknown id ignored"));
            return;
        };
        let rejected = match self
            .outbound_order(window_id)
            .insert(seq, item.clone(), now)
        {
            Ok(released) => {
                self.deliver_outbound(released);
                return;
            }
            Err(e) => e,
        };
        self.warn(format!(
            "ipc reply {id} with sequence {seq} could not be ordered: {rejected:?}"
        ));
        if let Outbound::Result { label, id, .. } = item
            && let Some(peer) = self.peers.get_mut(&label)
        {
            let err = Error::ipc_overloaded("The reply could not be delivered in order.");
            peer.outbox.push(HostMessage::IpcResult {
                id,
                ok: false,
                value: None,
                error: serde_json::to_value(&err).ok(),
            });
        }
    }

    // ----- other messages ---------------------------------------------------

    /// Queues any host message for `label` (state, window, lifecycle,
    /// packages, ...). These are never dropped by the router.
    pub fn push(&mut self, label: &str, message: HostMessage) {
        self.peer(label).outbox.push(message);
    }

    /// Takes every non-empty queue of a subscribed peer, in label order.
    pub fn drain(&mut self) -> Vec<(String, Vec<HostMessage>)> {
        let mut out: BTreeMap<String, Vec<HostMessage>> = BTreeMap::new();
        for (label, peer) in &mut self.peers {
            if peer.subscribed && !peer.outbox.is_empty() {
                out.insert(label.clone(), std::mem::take(&mut peer.outbox));
            }
        }
        out.into_iter().collect()
    }

    /// Whether [`Router::drain`] would return anything.
    #[must_use]
    pub fn has_output(&self) -> bool {
        self.peers
            .values()
            .any(|p| p.subscribed && !p.outbox.is_empty())
    }

    /// Timers: reorder gaps, startup queue expiry, invoke timeouts.
    pub fn tick(&mut self, now: u64) {
        // Inbound gaps.
        let labels: Vec<String> = self.peers.keys().cloned().collect();
        for label in labels {
            let expired = match self.peers.get_mut(&label) {
                Some(p) => p.inbound.expire(now, GAP_MS),
                None => continue,
            };
            for (a, b) in &expired.gaps {
                self.warn(format!(
                    "ipc from {label}: sequence {a}..={b} never arrived; skipped"
                ));
            }
            self.deliver_inbound(expired.released, now);
        }
        // Outbound gaps.
        let targets: Vec<u32> = self.outbound.keys().copied().collect();
        for target in targets {
            let expired = match self.outbound.get_mut(&target) {
                Some(r) => r.expire(now, GAP_MS),
                None => continue,
            };
            for (a, b) in &expired.gaps {
                self.warn(format!(
                    "ipc to window {target}: sequence {a}..={b} never arrived; skipped"
                ));
            }
            self.deliver_outbound(expired.released);
        }
        // Startup queue expiry.
        let timeout = self.limits.startup_timeout_ms;
        while let Some((since, _)) = self.startup.front() {
            if now.saturating_sub(*since) < timeout {
                break;
            }
            if let Some((_, item)) = self.startup.pop_front() {
                self.reject_queued(item, "The main webview did not become ready in time.");
            }
        }
        // Invoke timeouts.
        let expired: Vec<u64> = self
            .pending
            .iter()
            .filter(|(_, p)| p.deadline.is_some_and(|d| now >= d))
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            let err = Error::ipc_timeout("The invoke timed out.");
            self.finish_pending(id, Some(&err));
        }
    }

    /// The error code a rejected invoke result carries; for tests.
    #[cfg(test)]
    fn result_code(msg: &HostMessage) -> Option<String> {
        match msg {
            HostMessage::IpcResult { error: Some(e), .. } => e["code"].as_str().map(str::to_owned),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use serde_json::json;

    fn sender(id: u32) -> SenderInfo {
        SenderInfo {
            label: format!("bw-{id}"),
            window_id: id,
            url: "tauri://localhost/index.html".into(),
        }
    }

    fn ready_router(cfg: &IpcConfig) -> Router {
        let mut r = Router::new(cfg);
        r.subscribe(MAIN_LABEL, "m", 0);
        r.main_ready(0);
        r
    }

    /// Channels of `ipc` messages queued for `label`, in order.
    fn channels(out: &[(String, Vec<HostMessage>)], label: &str) -> Vec<String> {
        out.iter()
            .filter(|(l, _)| l == label)
            .flat_map(|(_, msgs)| msgs.iter())
            .filter_map(|m| match m {
                HostMessage::Ipc(
                    IpcMessage::Invoke { channel, .. }
                    | IpcMessage::Send { channel, .. }
                    | IpcMessage::Message { channel, .. },
                ) => Some(channel.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn invoke_round_trip() {
        let mut r = ready_router(&IpcConfig::default());
        r.subscribe("bw-1", "e", 0);
        let id = r
            .invoke(&sender(1), "e", 1, "c", vec![json!(1)], 5, 0)
            .unwrap();
        assert_eq!(id, 1);
        assert_eq!(r.in_flight("bw-1"), 1);
        let out = r.drain();
        let HostMessage::Ipc(IpcMessage::Invoke { sender, .. }) = &out[0].1[0] else {
            panic!()
        };
        assert_eq!(sender.label, "bw-1");
        assert_eq!(sender.frame_id, 0);
        r.reply(id, true, None, None, 1, 0);
        assert_eq!(r.in_flight("bw-1"), 0);
        let out = r.drain();
        assert_eq!(out.len(), 1);
        assert_eq!(
            serde_json::to_value(&out[0].1[0]).unwrap(),
            json!({"type":"ipc-result","id":1,"ok":true})
        );
        // A second reply is ignored and consumes nothing new.
        r.reply(id, true, None, None, 2, 0);
        assert!(r.drain().is_empty());
    }

    #[test]
    fn validation_errors() {
        let cfg = IpcConfig {
            max_message_bytes: 100,
            max_in_flight_invokes: 2,
            ..IpcConfig::default()
        };
        let mut r = ready_router(&cfg);
        r.subscribe("bw-1", "e", 0);
        let s = sender(1);
        let code = |res: Result<u64, Error>| res.unwrap_err().code();
        assert_eq!(
            code(r.invoke(&s, "old", 1, "c", vec![], 1, 0)),
            ErrorCode::NotReady
        );
        assert_eq!(
            code(r.invoke(&s, "e", 1, "", vec![], 1, 0)),
            ErrorCode::InvalidArgument
        );
        let long: String = "x".repeat(257);
        assert_eq!(
            code(r.invoke(&s, "e", 1, &long, vec![], 1, 0)),
            ErrorCode::InvalidArgument
        );
        // 128 astral characters = 256 UTF-16 units: allowed.
        let astral: String = "\u{1F600}".repeat(128);
        assert!(r.invoke(&s, "e", 1, &astral, vec![], 1, 0).is_ok());
        assert_eq!(
            code(r.invoke(&s, "e", 2, "c", vec![], 101, 0)),
            ErrorCode::IpcSerialization
        );
        assert!(r.invoke(&s, "e", 2, "c", vec![], 100, 0).is_ok());
        assert_eq!(
            code(r.invoke(&s, "e", 3, "c", vec![], 1, 0)),
            ErrorCode::IpcOverloaded
        );
        assert_eq!(
            code(r.invoke(&s, "e", 2, "c", vec![], 1, 0)),
            ErrorCode::InvalidArgument
        );
        // Unsubscribed window.
        assert_eq!(
            code(r.invoke(&sender(2), "e", 1, "c", vec![], 1, 0)),
            ErrorCode::NotReady
        );
    }

    #[test]
    fn startup_queue_overflow_and_expiry() {
        let cfg = IpcConfig {
            startup_queue_max: 2,
            startup_timeout_ms: 1000,
            ..IpcConfig::default()
        };
        let mut r = Router::new(&cfg);
        r.subscribe(MAIN_LABEL, "m", 0);
        r.subscribe("bw-1", "e", 0);
        let s = sender(1);
        let a = r.invoke(&s, "e", 1, "a", vec![], 1, 0).unwrap();
        r.send(&s, "e", 2, "b", vec![], 1, 500).unwrap();
        let c = r.invoke(&s, "e", 3, "c", vec![], 1, 600).unwrap();
        // c overflowed: rejected right away with not-ready.
        let out = r.drain();
        let to_ui: Vec<_> = out
            .iter()
            .filter(|(l, _)| l == "bw-1")
            .flat_map(|(_, m)| m)
            .collect();
        assert_eq!(to_ui.len(), 1);
        assert!(matches!(to_ui[0], HostMessage::IpcResult { id, .. } if *id == c));
        assert_eq!(Router::result_code(to_ui[0]).as_deref(), Some("not-ready"));
        assert!(
            channels(&out, MAIN_LABEL).is_empty(),
            "nothing reaches main before ipc_main_ready"
        );
        // a expires at 1000, b at 1500.
        r.tick(1000);
        let out = r.drain();
        assert!(matches!(out[0].1[0], HostMessage::IpcResult { id, .. } if id == a));
        assert_eq!(r.in_flight("bw-1"), 0);
        r.main_ready(1200);
        assert_eq!(channels(&r.drain(), MAIN_LABEL), vec!["b"]);
    }

    #[test]
    fn main_ready_flushes_in_fifo_order() {
        let mut r = Router::new(&IpcConfig::default());
        r.subscribe(MAIN_LABEL, "m", 0);
        r.subscribe("bw-1", "e", 0);
        r.subscribe("bw-2", "f", 0);
        r.send(&sender(1), "e", 1, "a1", vec![], 1, 0).unwrap();
        r.send(&sender(2), "f", 1, "b1", vec![], 1, 0).unwrap();
        r.invoke(&sender(1), "e", 2, "a2", vec![], 1, 0).unwrap();
        assert!(r.drain().is_empty());
        r.main_ready(1);
        assert_eq!(channels(&r.drain(), MAIN_LABEL), vec!["a1", "b1", "a2"]);
    }

    #[test]
    fn invoke_timeout_and_late_reply_consumes_seq() {
        let cfg = IpcConfig {
            invoke_timeout_ms: 100,
            ..IpcConfig::default()
        };
        let mut r = ready_router(&cfg);
        r.subscribe("bw-1", "e", 0);
        let id = r.invoke(&sender(1), "e", 1, "slow", vec![], 1, 0).unwrap();
        r.drain();
        r.tick(100);
        let out = r.drain();
        assert_eq!(
            Router::result_code(&out[0].1[0]).as_deref(),
            Some("ipc-timeout")
        );
        // The handler sent a message (seq 1) and then replied late (seq 2);
        // an emit with seq 3 must not wait for anything.
        r.emit(1, Some("bw-1"), 1, "m1", vec![], 1, 150).unwrap();
        r.reply(id, true, Some(json!(1)), None, 2, 160);
        r.emit(1, Some("bw-1"), 3, "m3", vec![], 1, 170).unwrap();
        let out = r.drain();
        assert_eq!(channels(&out, "bw-1"), vec!["m1", "m3"]);
        assert!(
            out[0]
                .1
                .iter()
                .all(|m| !matches!(m, HostMessage::IpcResult { .. }))
        );
    }

    #[test]
    fn reply_is_ordered_after_earlier_emits() {
        let mut r = ready_router(&IpcConfig::default());
        r.subscribe("bw-1", "e", 0);
        let id = r.invoke(&sender(1), "e", 1, "get", vec![], 1, 0).unwrap();
        r.drain();
        // The reply (seq 2) arrives before the emit (seq 1) it follows.
        r.reply(id, true, Some(json!("v")), None, 2, 0);
        assert!(r.drain().is_empty());
        r.emit(1, Some("bw-1"), 1, "progress", vec![], 1, 0)
            .unwrap();
        let out = r.drain();
        assert!(
            matches!(&out[0].1[0], HostMessage::Ipc(IpcMessage::Message { channel, .. }) if channel == "progress")
        );
        assert!(matches!(&out[0].1[1], HostMessage::IpcResult { id: i, .. } if *i == id));
    }

    #[test]
    fn emit_buffers_until_subscribe_and_reload_clears() {
        let cfg = IpcConfig {
            max_queued_messages: 2,
            ..IpcConfig::default()
        };
        let mut r = ready_router(&cfg);
        r.emit(1, Some("bw-1"), 1, "a", vec![], 1, 0).unwrap();
        r.emit(1, Some("bw-1"), 2, "b", vec![], 1, 0).unwrap();
        assert_eq!(
            r.emit(1, Some("bw-1"), 3, "c", vec![], 1, 0)
                .unwrap_err()
                .code(),
            ErrorCode::IpcOverloaded
        );
        assert!(r.drain().is_empty(), "not subscribed yet");
        r.subscribe("bw-1", "e", 0);
        assert_eq!(channels(&r.drain(), "bw-1"), vec!["a", "b"]);
        // The rejected message used up 3: 4 is not held back.
        r.emit(1, Some("bw-1"), 4, "c", vec![], 1, 0).unwrap();
        assert_eq!(channels(&r.drain(), "bw-1"), vec!["c"]);
        r.emit(1, Some("bw-1"), 5, "d", vec![], 1, 0).unwrap();
        r.document_unloaded("bw-1");
        r.subscribe("bw-1", "e2", 0);
        assert!(r.drain().is_empty(), "the reload emptied the buffer");
        // Remote / destroyed targets drop silently.
        r.emit(9, None, 1, "x", vec![], 1, 0).unwrap();
        assert!(r.take_logs().iter().any(|l| l.warn));
    }

    #[test]
    fn rejected_emits_and_reported_skips_do_not_hold_later_messages() {
        let mut r = ready_router(&IpcConfig::default());
        r.subscribe("bw-1", "e", 0);
        // A channel error after the target is known uses up its number.
        assert_eq!(
            r.emit(1, Some("bw-1"), 1, "", vec![], 1, 0)
                .unwrap_err()
                .code(),
            ErrorCode::InvalidArgument
        );
        r.emit(1, Some("bw-1"), 2, "two", vec![], 1, 0).unwrap();
        assert_eq!(channels(&r.drain(), "bw-1"), vec!["two"]);
        // The main runtime reports a number it never sent.
        r.emit(1, Some("bw-1"), 4, "four", vec![], 1, 0).unwrap();
        assert!(r.drain().is_empty(), "4 waits for 3");
        r.emit_skip(1, true, 3, 0);
        assert_eq!(channels(&r.drain(), "bw-1"), vec!["four"]);
        // A repeated skip is ignored.
        r.emit_skip(1, true, 3, 0);
        // Unknown windows get no order state.
        r.emit_skip(77, false, 1, 0);
        assert!(!r.outbound.contains_key(&77));
    }

    #[test]
    fn a_reply_that_cannot_be_ordered_still_settles_the_invoke() {
        let mut r = ready_router(&IpcConfig::default());
        r.subscribe("bw-1", "e", 0);
        let id = r.invoke(&sender(1), "e", 1, "get", vec![], 1, 0).unwrap();
        r.emit(1, Some("bw-1"), 1, "m", vec![], 1, 0).unwrap();
        r.drain();
        // `seq` 1 was already used by the emit.
        r.reply(id, true, Some(json!(1)), None, 1, 0);
        let out = r.drain();
        let msgs = &out.iter().find(|(l, _)| l == "bw-1").unwrap().1;
        assert_eq!(
            Router::result_code(&msgs[0]).as_deref(),
            Some("ipc-overloaded")
        );
        assert_eq!(r.in_flight("bw-1"), 0);
    }

    #[test]
    fn send_back_pressure_on_main() {
        let cfg = IpcConfig {
            max_queued_messages: 3,
            ..IpcConfig::default()
        };
        let mut r = ready_router(&cfg);
        r.subscribe("bw-1", "e", 0);
        for seq in 1..=3 {
            r.send(&sender(1), "e", seq, "s", vec![], 1, 0).unwrap();
        }
        assert_eq!(
            r.send(&sender(1), "e", 4, "s", vec![], 1, 0)
                .unwrap_err()
                .code(),
            ErrorCode::IpcOverloaded
        );
        r.drain();
        r.send(&sender(1), "e", 4, "s", vec![], 1, 0).unwrap();
    }

    #[test]
    fn main_restart_rejects_pending() {
        let mut r = ready_router(&IpcConfig::default());
        r.subscribe("bw-1", "e", 0);
        let id = r.invoke(&sender(1), "e", 1, "c", vec![], 1, 0).unwrap();
        r.drain();
        r.subscribe(MAIN_LABEL, "m2", 0);
        assert!(!r.is_main_ready());
        let out = r.drain();
        assert!(
            matches!(&out[0].1[0], HostMessage::IpcResult { id: i, ok: false, .. } if *i == id)
        );
        // The old main's reply is ignored; the order restarts at 1.
        r.reply(id, true, None, None, 1, 0);
        assert!(r.drain().is_empty());
        r.main_ready(0);
        r.emit(1, Some("bw-1"), 1, "fresh", vec![], 1, 0).unwrap();
        assert_eq!(channels(&r.drain(), "bw-1"), vec!["fresh"]);
    }

    #[test]
    fn sender_reload_and_destroy_drop_pending() {
        let mut r = ready_router(&IpcConfig::default());
        r.subscribe("bw-1", "e", 0);
        let id = r.invoke(&sender(1), "e", 1, "c", vec![], 1, 0).unwrap();
        r.drain();
        r.document_unloaded("bw-1");
        assert_eq!(r.in_flight("bw-1"), 0);
        r.subscribe("bw-1", "e2", 0);
        assert_eq!(
            r.invoke(&sender(1), "e", 1, "c", vec![], 1, 0)
                .unwrap_err()
                .code(),
            ErrorCode::NotReady,
            "stale epoch"
        );
        // The reply to the old document's request is dropped but its seq is
        // consumed.
        r.reply(id, true, None, None, 1, 0);
        r.emit(1, Some("bw-1"), 2, "after", vec![], 1, 0).unwrap();
        let out = r.drain();
        assert_eq!(channels(&out, "bw-1"), vec!["after"]);
        assert_eq!(out[0].1.len(), 1);
        r.remove_peer("bw-1", Some(1));
        assert!(!r.is_subscribed("bw-1"));
    }

    // ----- property-style tests ----------------------------------------------

    /// xorshift64*: deterministic, dependency-free randomness.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
        fn shuffle<T>(&mut self, v: &mut [T]) {
            for i in (1..v.len()).rev() {
                let j = usize::try_from(self.below(i as u64 + 1)).unwrap();
                v.swap(i, j);
            }
        }
    }

    /// Random interleavings of several senders, with duplicates, explicit
    /// skips, silent gaps and stale-epoch calls: every message that was
    /// accepted reaches main exactly once, and per sender in increasing
    /// sequence order.
    #[test]
    fn property_inbound_order() {
        for case in 0..300_u64 {
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (case + 1));
            let mut r = ready_router(&IpcConfig::default());
            let senders = 1 + rng.below(3);
            let mut events: Vec<(u32, u64, u8)> = Vec::new(); // (window, seq, kind)
            for w in 1..=senders {
                let w = u32::try_from(w).unwrap();
                r.subscribe(&format!("bw-{w}"), "e", 0);
                let n = 1 + rng.below(25);
                for seq in 1..=n {
                    // 0 = send, 1 = invoke, 2 = rejected before the plugin
                    // (skip), 3 = lost (gap fallback)
                    let kind = match rng.below(10) {
                        0 => 2,
                        1 => 3,
                        2..=5 => 1,
                        _ => 0,
                    };
                    events.push((w, seq, kind));
                    if rng.below(8) == 0 {
                        events.push((w, seq, kind)); // duplicate delivery
                    }
                }
                if rng.below(4) == 0 {
                    events.push((w, 1, 9)); // stale-epoch call
                }
            }
            rng.shuffle(&mut events);
            let mut now = 0;
            let mut accepted: HashMap<u32, Vec<u64>> = HashMap::new();
            let mut seen: HashMap<u32, Vec<u64>> = HashMap::new();
            let collect = |out: Vec<(String, Vec<HostMessage>)>,
                           seen: &mut HashMap<u32, Vec<u64>>| {
                for (label, msgs) in out {
                    if label != MAIN_LABEL {
                        continue;
                    }
                    for m in msgs {
                        if let HostMessage::Ipc(
                            IpcMessage::Invoke {
                                channel, sender, ..
                            }
                            | IpcMessage::Send {
                                channel, sender, ..
                            },
                        ) = m
                        {
                            seen.entry(sender.window_id)
                                .or_default()
                                .push(channel.parse().unwrap());
                        }
                    }
                }
            };
            for (w, seq, kind) in events {
                now += rng.below(40);
                let s = sender(w);
                let ch = seq.to_string();
                let ok = match kind {
                    0 => r.send(&s, "e", seq, &ch, vec![], 1, now).is_ok(),
                    1 => r.invoke(&s, "e", seq, &ch, vec![], 1, now).is_ok(),
                    2 => {
                        r.skip(&s.label, "e", seq, now);
                        false
                    }
                    9 => {
                        assert!(r.send(&s, "stale", seq, "x", vec![], 1, now).is_err());
                        false
                    }
                    _ => false,
                };
                if ok {
                    accepted.entry(w).or_default().push(seq);
                }
                if rng.below(3) == 0 {
                    r.tick(now);
                }
                collect(r.drain(), &mut seen);
            }
            for k in 1..=80 {
                r.tick(now + k * GAP_MS);
            }
            collect(r.drain(), &mut seen);
            for (w, mut acc) in accepted {
                acc.sort_unstable();
                let got = seen.remove(&w).unwrap_or_default();
                assert_eq!(got, acc, "case {case}, window {w}");
            }
        }
    }

    /// Random interleavings of emits and replies to several targets: each
    /// target receives them in the main runtime's `seq` order, nothing
    /// twice, and every reply exactly once.
    #[test]
    fn property_outbound_order() {
        for case in 0..300_u64 {
            let mut rng = Rng(0xD1B5_4A32_D192_ED03 ^ (case + 7));
            let mut r = ready_router(&IpcConfig::default());
            let targets = 1 + u32::try_from(rng.below(3)).unwrap();
            // Per target: a script of (seq, Some(invoke id) | None).
            let mut ops: Vec<(u32, u64, Option<u64>)> = Vec::new();
            for t in 1..=targets {
                r.subscribe(&format!("bw-{t}"), "e", 0);
                let n = 1 + rng.below(30);
                let mut invoke_seq = 0;
                for seq in 1..=n {
                    if rng.below(3) == 0 {
                        invoke_seq += 1;
                        let id = r
                            .invoke(&sender(t), "e", invoke_seq, "req", vec![], 1, 0)
                            .unwrap();
                        ops.push((t, seq, Some(id)));
                    } else {
                        ops.push((t, seq, None));
                    }
                }
            }
            r.drain();
            rng.shuffle(&mut ops);
            let mut got: HashMap<u32, Vec<u64>> = HashMap::new();
            for (t, seq, id) in &ops {
                match id {
                    Some(id) => r.reply(*id, true, Some(json!(seq)), None, *seq, 0),
                    None => r
                        .emit(
                            *t,
                            Some(&format!("bw-{t}")),
                            *seq,
                            &seq.to_string(),
                            vec![],
                            1,
                            0,
                        )
                        .unwrap(),
                }
                for (label, msgs) in r.drain() {
                    let t: u32 = label.trim_start_matches("bw-").parse().unwrap();
                    for m in msgs {
                        let seq = match m {
                            HostMessage::Ipc(IpcMessage::Message { channel, .. }) => {
                                channel.parse().unwrap()
                            }
                            HostMessage::IpcResult { value: Some(v), .. } => v.as_u64().unwrap(),
                            other => panic!("unexpected {other:?}"),
                        };
                        got.entry(t).or_default().push(seq);
                    }
                }
            }
            for t in 1..=targets {
                let expect: Vec<u64> = {
                    let mut v: Vec<u64> = ops.iter().filter(|o| o.0 == t).map(|o| o.1).collect();
                    v.sort_unstable();
                    v
                };
                assert_eq!(
                    got.remove(&t).unwrap_or_default(),
                    expect,
                    "case {case}, target {t}"
                );
                assert_eq!(r.in_flight(&format!("bw-{t}")), 0);
            }
        }
    }

    /// The in-flight cap holds under any interleaving of invokes and replies.
    #[test]
    fn property_in_flight_cap() {
        let cfg = IpcConfig {
            max_in_flight_invokes: 4,
            ..IpcConfig::default()
        };
        let mut rng = Rng(42);
        let mut r = ready_router(&cfg);
        r.subscribe("bw-1", "e", 0);
        let mut open: Vec<u64> = Vec::new();
        let mut seq = 0;
        let mut reply_seq = 0;
        for _ in 0..2000 {
            if rng.below(2) == 0 {
                seq += 1;
                match r.invoke(&sender(1), "e", seq, "c", vec![], 1, 0) {
                    Ok(id) => open.push(id),
                    Err(e) => {
                        assert_eq!(e.code(), ErrorCode::IpcOverloaded);
                        assert_eq!(open.len(), 4);
                        r.skip("bw-1", "e", seq, 0);
                    }
                }
            } else if !open.is_empty() {
                let i = usize::try_from(rng.below(open.len() as u64)).unwrap();
                let id = open.swap_remove(i);
                reply_seq += 1;
                r.reply(id, true, None, None, reply_seq, 0);
            }
            assert!(r.in_flight("bw-1") <= 4);
            assert_eq!(r.in_flight("bw-1"), open.len());
            r.drain();
        }
    }
}
