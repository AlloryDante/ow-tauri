//! SEC-m10 (DESIGN §4.2, §7.2): the exit drain is bounded even when the
//! transport blocks the request lane's thread outright, and it never needs
//! the main thread, which is busy running the exit.
//!
//! This is the only test in its binary: the request lane is one thread per
//! process, and this test blocks it until the app has exited.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test fails on any unexpected error"
)]

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{RunEvent, WebviewUrl};
use tauri_plugin_overwolf::Builder;
use tauri_plugin_overwolf::analytics::{
    BoxFuture, DRAIN_LIMIT, HostRequest, HostResponse, Transport,
};

use common::{lock, wait_until};

/// Blocks the calling thread in `send` until released.
#[derive(Default)]
struct Blocking {
    calls: AtomicUsize,
    released: Mutex<bool>,
    wake: Condvar,
}

impl Blocking {
    fn release(&self) {
        *lock(&self.released) = true;
        self.wake.notify_all();
    }
}

impl Transport for Blocking {
    fn send(&self, _request: HostRequest) -> BoxFuture<Result<HostResponse, String>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut released = lock(&self.released);
        while !*released {
            released = self
                .wake
                .wait(released)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        Box::pin(async { Err("released".to_owned()) })
    }
}

#[test]
fn the_exit_drain_is_bounded_with_a_blocking_transport() {
    let transport = Arc::new(Blocking::default());
    let (context, _dir) = common::context(
        "drain-blocking",
        &json!({ "analytics": { "muidStrategy": "per-install" } }),
        &[],
    );
    let app = common::build(
        context,
        Builder::new().analytics_transport(transport.clone()),
        None,
    );
    common::window(&app, "main", WebviewUrl::default());
    let exit_requested_at = Arc::new(Mutex::new(None));
    let at = Arc::clone(&exit_requested_at);
    let blocked = Arc::clone(&transport);
    common::run_with(
        app,
        move |_| {
            assert!(
                wait_until(Duration::from_secs(15), || blocked
                    .calls
                    .load(Ordering::SeqCst)
                    > 0),
                "the transport was called and blocks the request lane"
            );
        },
        move |_, event| {
            if let RunEvent::ExitRequested { .. } = event {
                *lock(&at) = Some(Instant::now());
            }
        },
    );
    let took = lock(&exit_requested_at)
        .expect("the exit was requested")
        .elapsed();
    transport.release();
    assert!(
        took >= DRAIN_LIMIT,
        "the exit waited for the drain: {took:?}"
    );
    assert!(
        took < DRAIN_LIMIT + Duration::from_secs(1),
        "the drain is bounded and needed no main-thread hop: {took:?}"
    );
}
