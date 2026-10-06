//! Sending host requests: the [`Transport`] trait, the default `reqwest`
//! transport, and the dispatcher that starts requests in call order and
//! lets an exit drain them (CONTRACT E.1, A.6).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::{HostRequest, HostResponse, Method};

/// A boxed, sendable future.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// Sends host requests. The default is an HTTP client; tests pass their own
/// with `Builder::analytics_transport` to capture requests instead.
///
/// `send` is called synchronously, in the order the host makes requests;
/// the returned future performs the request.
///
/// ```
/// use std::sync::{Arc, Mutex};
/// use tauri_plugin_overwolf::analytics::{BoxFuture, HostRequest, HostResponse, Transport};
/// #[derive(Default)]
/// struct Capture(Mutex<Vec<HostRequest>>);
/// impl Transport for Capture {
///     fn send(&self, request: HostRequest) -> BoxFuture<Result<HostResponse, String>> {
///         self.0.lock().unwrap().push(request);
///         Box::pin(async { Ok(HostResponse { status: 200, body: b"{}".to_vec() }) })
///     }
/// }
/// let _transport: Arc<dyn Transport> = Arc::new(Capture::default());
/// ```
pub trait Transport: Send + Sync + 'static {
    /// Starts `request`; the future resolves with the response or an error
    /// text (never retried).
    fn send(&self, request: HostRequest) -> BoxFuture<Result<HostResponse, String>>;
}

/// The default transport: `reqwest` with the system proxy, decoding gzip,
/// deflate, brotli and zstd. In test builds (`cfg(test)` or the
/// `test-util` feature) it refuses every host that is not loopback.
#[derive(Debug, Clone)]
pub(crate) struct ReqwestTransport {
    client: Option<reqwest::Client>,
}

impl ReqwestTransport {
    pub(crate) fn new() -> Self {
        ReqwestTransport {
            client: reqwest::Client::builder().build().ok(),
        }
    }
}

/// Whether the default transport may contact `url` in this build.
pub(crate) fn allowed_host(url: &str) -> bool {
    if cfg!(any(test, feature = "test-util")) {
        url::Url::parse(url).is_ok_and(|u| match u.host() {
            Some(url::Host::Domain(d)) => d == "localhost",
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        })
    } else {
        true
    }
}

impl Transport for ReqwestTransport {
    fn send(&self, request: HostRequest) -> BoxFuture<Result<HostResponse, String>> {
        let Some(client) = self.client.clone() else {
            return Box::pin(async { Err("HTTP client unavailable".to_owned()) });
        };
        if !allowed_host(&request.url) {
            return Box::pin(async { Err("non-loopback host refused in a test build".to_owned()) });
        }
        let method = match request.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
        };
        let mut builder = client.request(method, &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(*name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        if let Some(timeout) = request.timeout {
            builder = builder.timeout(timeout);
        }
        Box::pin(async move {
            let response = builder.send().await.map_err(|e| e.to_string())?;
            let status = response.status().as_u16();
            let body = response.bytes().await.map_err(|e| e.to_string())?;
            Ok(HostResponse {
                status,
                body: body.to_vec(),
            })
        })
    }
}

/// Starts requests in call order and tracks the ones in flight.
#[derive(Clone)]
pub(crate) struct Dispatcher {
    transport: Arc<dyn Transport>,
    in_flight: Arc<AtomicUsize>,
    idle: Arc<tokio::sync::Notify>,
}

impl std::fmt::Debug for Dispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatcher")
            .field("in_flight", &self.in_flight.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

struct InFlight {
    count: Arc<AtomicUsize>,
    idle: Arc<tokio::sync::Notify>,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if self.count.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.idle.notify_waiters();
        }
    }
}

impl Dispatcher {
    pub(crate) fn new(transport: Arc<dyn Transport>) -> Self {
        Dispatcher {
            transport,
            in_flight: Arc::new(AtomicUsize::new(0)),
            idle: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Starts `request` now. `tracked` requests count for [`Self::drain`]
    /// (analytics); the consent request does not.
    pub(crate) fn send(
        &self,
        request: HostRequest,
        tracked: bool,
    ) -> tauri::async_runtime::JoinHandle<Result<HostResponse, String>> {
        let guard = tracked.then(|| {
            self.in_flight.fetch_add(1, Ordering::SeqCst);
            InFlight {
                count: self.in_flight.clone(),
                idle: self.idle.clone(),
            }
        });
        let fut = self.transport.send(request);
        tauri::async_runtime::spawn(async move {
            let out = fut.await;
            drop(guard);
            out
        })
    }

    /// Requests in flight.
    pub(crate) fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// Waits until no tracked request is in flight, at most `limit`.
    pub(crate) async fn drain(&self, limit: Duration) {
        let wait = async {
            loop {
                let notified = self.idle.notified();
                if self.in_flight() == 0 {
                    return;
                }
                notified.await;
            }
        };
        let _ = tokio::time::timeout(limit, wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builds_refuse_remote_hosts() {
        assert!(allowed_host("http://127.0.0.1:9/x"));
        assert!(allowed_host("http://localhost/x"));
        assert!(allowed_host("http://[::1]/x"));
        assert!(!allowed_host(
            "https://analyticsnew.overwolf.com/analytics/Counter"
        ));
        let t = ReqwestTransport::new();
        let req = HostRequest {
            method: Method::Get,
            url: "https://tracking.overwolf.com/".into(),
            headers: vec![],
            body: None,
            timeout: None,
        };
        let out = tauri::async_runtime::block_on(t.send(req));
        assert!(out.unwrap_err().contains("refused"));
    }

    struct Slow;
    impl Transport for Slow {
        fn send(&self, _r: HostRequest) -> BoxFuture<Result<HostResponse, String>> {
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                Ok(HostResponse::default())
            })
        }
    }

    #[test]
    fn drain_waits_for_tracked_requests_only() {
        tauri::async_runtime::block_on(async {
            let d = Dispatcher::new(Arc::new(Slow));
            let req = HostRequest {
                method: Method::Get,
                url: String::new(),
                headers: vec![],
                body: None,
                timeout: None,
            };
            let _untracked = d.send(req.clone(), false);
            let _tracked = d.send(req, true);
            assert_eq!(d.in_flight(), 1);
            d.drain(Duration::from_secs(2)).await;
            assert_eq!(d.in_flight(), 0);
            // Nothing in flight: returns at once.
            d.drain(Duration::from_millis(1)).await;
        });
    }
}
