//! Host messages: everything Rust sends to a webview (CONTRACT A.3).
//!
//! A webview receives `HostMessage[]` over the channel it registered with
//! `ipc_subscribe`; each element has a kebab-case `type`.
//!
//! ```
//! use tauri_plugin_overwolf::ipc::messages::{HostMessage, IpcMessage};
//! let msg = HostMessage::Ipc(IpcMessage::Message { channel: "ping".into(), args: vec![serde_json::json!(1)] });
//! assert_eq!(
//!     serde_json::to_value(&msg).unwrap(),
//!     serde_json::json!({ "type": "ipc", "kind": "message", "channel": "ping", "args": [1] })
//! );
//! ```

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

/// The sender of an IPC request, stamped by Rust from the calling webview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcSender {
    /// Electron-style window id.
    pub window_id: u32,
    /// Webview label.
    pub label: String,
    /// Document URL.
    pub url: String,
    /// Always 0 (main frame).
    pub frame_id: u32,
}

/// The `ipc` host message.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum IpcMessage {
    /// An `ipcRenderer.invoke` request, to `ow-main`.
    Invoke {
        /// Request id.
        id: u64,
        /// Channel name.
        channel: String,
        /// OTJ-encoded arguments.
        args: Vec<Value>,
        /// The sending webview.
        sender: IpcSender,
    },
    /// An `ipcRenderer.send` message, to `ow-main`.
    Send {
        /// Channel name.
        channel: String,
        /// OTJ-encoded arguments.
        args: Vec<Value>,
        /// The sending webview.
        sender: IpcSender,
    },
    /// A `webContents.send` message, to a UI webview.
    Message {
        /// Channel name.
        channel: String,
        /// OTJ-encoded arguments.
        args: Vec<Value>,
    },
}

/// One state patch (B.1.6).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Patch {
    /// Dot path into `HostSnapshot`.
    pub path: String,
    /// New value.
    pub value: Value,
}

/// `WindowEventName` (A.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
#[expect(
    missing_docs,
    reason = "each variant is the Electron event of the same name"
)]
pub enum WindowEventName {
    Created,
    Close,
    Closed,
    Focus,
    Blur,
    Show,
    Hide,
    Minimize,
    Maximize,
    Unmaximize,
    Restore,
    Resize,
    Move,
    EnterFullScreen,
    LeaveFullScreen,
    ReadyToShow,
    DidFinishLoad,
    DomReady,
    DidFailLoad,
    RenderProcessGone,
    /// A top-level navigation the A.2.3.1 policy cancelled; `data.url`.
    WillNavigate,
    /// A `window.open` / `target=_blank` request, always denied natively;
    /// `data.url`. The main runtime runs `setWindowOpenHandler`.
    NewWindow,
}

/// Everything Rust sends to a webview.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum HostMessage {
    /// IPC traffic (C).
    Ipc(IpcMessage),
    /// The reply to an `ipc_invoke` (C.2).
    #[serde(rename_all = "camelCase")]
    IpcResult {
        /// Request id.
        id: u64,
        /// Whether the handler succeeded.
        ok: bool,
        /// OTJ value; absent decodes to `undefined`.
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
        /// `IpcErrorWire` when `ok` is false.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<Value>,
    },
    /// Sync-cache update (B.1.6).
    State {
        /// State sequence number (C.6).
        seq: u64,
        /// Patches, applied in order.
        patches: Vec<Patch>,
    },
    /// Window lifecycle (B.2).
    #[serde(rename_all = "camelCase")]
    Window {
        /// Electron-style window id.
        id: u32,
        /// The event.
        event: WindowEventName,
        /// On `close`: answer with `window_close_reply`.
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
        /// Event details.
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<Value>,
    },
    /// The quit sequence (A.6) and app-level events (`second-instance`,
    /// `activate`).
    #[serde(rename_all = "camelCase")]
    Lifecycle {
        /// `before-quit`, `will-quit`, `quit`, `second-instance`, `activate`.
        event: String,
        /// On `before-quit` / `will-quit`: answer with `app_quit_reply`.
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
        /// On `quit`.
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        /// Further fields (`argv`, `cwd` for `second-instance`).
        #[serde(flatten)]
        extra: Map<String, Value>,
    },
    /// Package manager lifecycle (B.1.3). The contract's inner `type` field is
    /// carried as `event`, because `type` is the message tag.
    Packages {
        /// Fields: `event` plus `name`, `version`, `details`, ...
        #[serde(flatten)]
        body: Map<String, Value>,
    },
    /// An event emitted by a package object (B.1.4).
    #[serde(rename_all = "camelCase")]
    PackageEvent {
        /// Package name.
        package: String,
        /// Event name.
        event: String,
        /// Remote-value encoded arguments.
        args: Vec<Value>,
        /// Actionable event id.
        #[serde(skip_serializing_if = "Option::is_none")]
        event_id: Option<u64>,
        /// Allowed actions.
        #[serde(skip_serializing_if = "Option::is_none")]
        actions: Option<Vec<String>>,
    },
    /// The runtime invokes a callback reference (H.2.1).
    #[serde(rename_all = "camelCase")]
    PackageCallback {
        /// Callback id.
        cb_id: u64,
        /// Arguments.
        args: Vec<Value>,
    },
    /// Callbacks the runtime will never invoke again.
    #[serde(rename_all = "camelCase")]
    PackageCallbackRelease {
        /// Callback ids.
        cb_ids: Vec<u64>,
    },
    /// An `<owadview>` event for its embedder (B.3.5).
    #[serde(rename_all = "camelCase")]
    AdviewEvent {
        /// Element id.
        element_id: String,
        /// Event name.
        name: String,
        /// Event data.
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<Value>,
        /// `guest` or `host`.
        source: String,
    },
    /// Update client events (I.3). The contract's inner `type` field is
    /// carried as `event`, as for [`HostMessage::Packages`].
    Updater {
        /// Fields: `event` plus `info`, `progress`, `error`.
        #[serde(flatten)]
        body: Map<String, Value>,
    },
    /// A registered global shortcut was pressed or released.
    GlobalShortcut {
        /// The id passed to `global_shortcut_register`.
        id: u64,
        /// The accelerator.
        accelerator: String,
        /// `pressed` or `released`.
        state: &'static str,
    },
}

impl HostMessage {
    /// A `window` message without request id.
    #[must_use]
    pub fn window(id: u32, event: WindowEventName, data: Option<Value>) -> Self {
        HostMessage::Window {
            id,
            event,
            request_id: None,
            data,
        }
    }

    /// A `lifecycle` message.
    #[must_use]
    pub fn lifecycle(event: &str, request_id: Option<u64>, exit_code: Option<i32>) -> Self {
        HostMessage::Lifecycle {
            event: event.to_owned(),
            request_id,
            exit_code,
            extra: Map::new(),
        }
    }
}

/// Deserialises a present field as `Some`, including an explicit `null`, so
/// that "absent" (`undefined`) and `null` stay distinct.
///
/// # Errors
///
/// When the field is not valid JSON.
pub fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shapes() {
        let sender = IpcSender {
            window_id: 3,
            label: "bw-3".into(),
            url: "tauri://localhost/index.html".into(),
            frame_id: 0,
        };
        assert_eq!(
            serde_json::to_value(HostMessage::Ipc(IpcMessage::Invoke {
                id: 9,
                channel: "c".into(),
                args: vec![],
                sender
            }))
            .unwrap(),
            json!({"type":"ipc","kind":"invoke","id":9,"channel":"c","args":[],"sender":{"windowId":3,"label":"bw-3","url":"tauri://localhost/index.html","frameId":0}})
        );
        assert_eq!(
            serde_json::to_value(HostMessage::IpcResult {
                id: 1,
                ok: true,
                value: None,
                error: None
            })
            .unwrap(),
            json!({"type":"ipc-result","id":1,"ok":true})
        );
        assert_eq!(
            serde_json::to_value(HostMessage::IpcResult {
                id: 1,
                ok: true,
                value: Some(Value::Null),
                error: None
            })
            .unwrap(),
            json!({"type":"ipc-result","id":1,"ok":true,"value":null})
        );
        assert_eq!(
            serde_json::to_value(HostMessage::Window {
                id: 2,
                event: WindowEventName::EnterFullScreen,
                request_id: Some(5),
                data: None
            })
            .unwrap(),
            json!({"type":"window","id":2,"event":"enter-full-screen","requestId":5})
        );
        let mut extra = Map::new();
        extra.insert("argv".into(), json!(["a"]));
        assert_eq!(
            serde_json::to_value(HostMessage::Lifecycle {
                event: "second-instance".into(),
                request_id: None,
                exit_code: None,
                extra
            })
            .unwrap(),
            json!({"type":"lifecycle","event":"second-instance","argv":["a"]})
        );
        assert_eq!(
            serde_json::to_value(HostMessage::lifecycle("quit", None, Some(0))).unwrap(),
            json!({"type":"lifecycle","event":"quit","exitCode":0})
        );
        let mut body = Map::new();
        body.insert("event".into(), json!("ready"));
        body.insert("name".into(), json!("gep"));
        assert_eq!(
            serde_json::to_value(HostMessage::Packages { body }).unwrap(),
            json!({"type":"packages","event":"ready","name":"gep"})
        );
        assert_eq!(
            serde_json::to_value(HostMessage::GlobalShortcut {
                id: 1,
                accelerator: "Ctrl+K".into(),
                state: "pressed"
            })
            .unwrap(),
            json!({"type":"global-shortcut","id":1,"accelerator":"Ctrl+K","state":"pressed"})
        );
        assert_eq!(
            serde_json::to_value(HostMessage::State {
                seq: 4,
                patches: vec![Patch {
                    path: "displays".into(),
                    value: json!([])
                }]
            })
            .unwrap(),
            json!({"type":"state","seq":4,"patches":[{"path":"displays","value":[]}]})
        );
    }

    #[test]
    fn present_keeps_null_distinct_from_absent() {
        #[derive(Deserialize)]
        struct R {
            #[serde(default, deserialize_with = "present")]
            value: Option<Value>,
        }
        let absent: R = serde_json::from_str("{}").unwrap();
        let null: R = serde_json::from_str(r#"{"value":null}"#).unwrap();
        assert_eq!(absent.value, None);
        assert_eq!(null.value, Some(Value::Null));
    }
}
