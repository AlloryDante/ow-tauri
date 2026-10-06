//! IPC between webviews and Rust (CONTRACT A.3, C).
//!
//! - [`messages`]: the `HostMessage` shapes Rust sends to a webview.
//! - [`reorder`]: the sequence-number reorder buffer.
//! - [`router`]: the pure routing state machine (epochs, ordering, startup
//!   queue, back-pressure, timeouts).

pub mod messages;
pub mod reorder;
pub mod router;
