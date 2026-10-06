//! Sending host requests: the [`Transport`] trait, the default `reqwest`
//! transport, and the dispatcher that sends analytics requests in call
//! order, adds the final `<UA>` and the ads data store's cookies, and lets
//! an exit drain them (CONTRACT E.1, A.6).

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
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
///         Box::pin(async { Ok(HostResponse { status: 200, body: b"{}".to_vec(), ..HostResponse::default() }) })
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
/// deflate, brotli and zstd, without a cookie jar of its own (the host adds
/// the ads data store's cookies, E.1). `reqwest` always adds
/// `accept: */*`, a header ow-electron does not send (a known deviation:
/// the client has no way to leave it out). In test builds (`cfg(test)` or the
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
            let set_cookies = response
                .headers()
                .get_all(reqwest::header::SET_COOKIE)
                .iter()
                .filter_map(|v| v.to_str().ok().map(str::to_owned))
                .collect();
            let body = response.bytes().await.map_err(|e| e.to_string())?;
            Ok(HostResponse {
                status,
                body: body.to_vec(),
                set_cookies,
            })
        })
    }
}

/// The longest the dispatcher waits for one analytics request to finish
/// before it starts the next one. Requests start strictly in call order and
/// normally finish in that order too (E.2, E.3); a slow response delays the
/// next request by at most this long.
pub(crate) const ORDER_WINDOW: Duration = Duration::from_millis(1000);

/// What the host adds to each request just before it goes out (E.1): the
/// final `<UA>` and the ads data store's cookies, and where `Set-Cookie`
/// response headers go.
pub(crate) trait RequestHooks: Send + Sync + 'static {
    /// Resolves once `<UA>` is final, or when waiting for it is over.
    fn wait_user_agent(&self) -> BoxFuture<()>;
    /// `<UA>` as currently known.
    fn user_agent(&self) -> Option<String>;
    /// The `cookie` header value for `url`, if the store has cookies for it.
    fn cookie_header(&self, url: &str) -> BoxFuture<Option<String>>;
    /// Writes the `Set-Cookie` values of a response from `url` to the store.
    fn store_cookies(&self, url: &str, set_cookies: Vec<String>) -> BoxFuture<()>;
}

/// Applies `hooks` to `request`: the final `<UA>` replaces the one it was
/// built with, and the store's cookies go last (E.1 header order).
async fn prepare(hooks: Option<&Arc<dyn RequestHooks>>, request: &mut HostRequest) {
    let Some(hooks) = hooks else { return };
    hooks.wait_user_agent().await;
    if let Some(ua) = hooks.user_agent() {
        for (name, value) in &mut request.headers {
            if *name == "user-agent" {
                value.clone_from(&ua);
            }
        }
    }
    if let Some(cookie) = hooks.cookie_header(&request.url).await {
        request.headers.push(("cookie", cookie));
    }
}

/// Sends `request` and writes its `Set-Cookie` headers back.
async fn perform(
    transport: Arc<dyn Transport>,
    hooks: Option<Arc<dyn RequestHooks>>,
    request: HostRequest,
) -> Result<HostResponse, String> {
    let url = request.url.clone();
    let out = transport.send(request).await;
    if let (Ok(response), Some(hooks)) = (&out, hooks)
        && !response.set_cookies.is_empty()
    {
        hooks
            .store_cookies(&url, response.set_cookies.clone())
            .await;
    }
    out
}

/// One analytics request waiting for its turn.
struct Job {
    request: HostRequest,
    guard: Option<InFlight>,
    reply: tokio::sync::oneshot::Sender<Result<HostResponse, String>>,
}

/// Sends host requests: analytics requests one after another in call order
/// (each waits for the previous one, at most [`ORDER_WINDOW`]); the consent
/// request at once, in parallel with them (E.2 #2). Tracks the analytics
/// requests in flight for the exit drain.
#[derive(Clone)]
pub(crate) struct Dispatcher {
    transport: Arc<dyn Transport>,
    hooks: Arc<OnceLock<Arc<dyn RequestHooks>>>,
    queue: Arc<OnceLock<tokio::sync::mpsc::UnboundedSender<Job>>>,
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

/// The result of [`Dispatcher::send`].
pub(crate) type Pending = BoxFuture<Result<HostResponse, String>>;

impl Dispatcher {
    pub(crate) fn new(transport: Arc<dyn Transport>) -> Self {
        Dispatcher {
            transport,
            hooks: Arc::new(OnceLock::new()),
            queue: Arc::new(OnceLock::new()),
            in_flight: Arc::new(AtomicUsize::new(0)),
            idle: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Installs the host's request hooks (once; later calls are ignored).
    pub(crate) fn set_hooks(&self, hooks: Arc<dyn RequestHooks>) {
        let _ = self.hooks.set(hooks);
    }

    fn worker(&self) -> &tokio::sync::mpsc::UnboundedSender<Job> {
        self.queue.get_or_init(|| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Job>();
            let transport = Arc::clone(&self.transport);
            let hooks = Arc::clone(&self.hooks);
            tauri::async_runtime::spawn(async move {
                while let Some(job) = rx.recv().await {
                    let Job {
                        mut request,
                        guard,
                        reply,
                    } = job;
                    let hooks = hooks.get().cloned();
                    prepare(hooks.as_ref(), &mut request).await;
                    let transport = Arc::clone(&transport);
                    let mut handle = tauri::async_runtime::spawn(async move {
                        let out = perform(transport, hooks, request).await;
                        drop(guard);
                        let _ = reply.send(out);
                    });
                    let _ = tokio::time::timeout(ORDER_WINDOW, &mut handle).await;
                }
            });
            tx
        })
    }

    /// Sends `request`. `tracked` requests (analytics) take their turn in
    /// call order and count for [`Self::drain`]; an untracked one (the
    /// consent request) starts at once. The returned future resolves with
    /// the response; dropping it does not cancel the request.
    pub(crate) fn send(&self, request: HostRequest, tracked: bool) -> Pending {
        let (reply, rx) = tokio::sync::oneshot::channel();
        if tracked {
            self.in_flight.fetch_add(1, Ordering::SeqCst);
            let guard = InFlight {
                count: self.in_flight.clone(),
                idle: self.idle.clone(),
            };
            let job = Job {
                request,
                guard: Some(guard),
                reply,
            };
            if let Err(err) = self.worker().send(job) {
                // The worker is gone (runtime shutting down): fail at once.
                let Job { reply, .. } = err.0;
                let _ = reply.send(Err("the request queue is closed".to_owned()));
            }
        } else {
            let transport = Arc::clone(&self.transport);
            let hooks = self.hooks.get().cloned();
            tauri::async_runtime::spawn(async move {
                let mut request = request;
                prepare(hooks.as_ref(), &mut request).await;
                let _ = reply.send(perform(transport, hooks, request).await);
            });
        }
        Box::pin(async move {
            rx.await
                .unwrap_or_else(|_| Err("the request was dropped".to_owned()))
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

    /// Records each request when it starts and when it finishes.
    #[derive(Default)]
    struct Recorder {
        log: std::sync::Mutex<Vec<String>>,
        requests: std::sync::Mutex<Vec<HostRequest>>,
    }

    struct Recording(Arc<Recorder>);
    impl Transport for Recording {
        fn send(&self, r: HostRequest) -> BoxFuture<Result<HostResponse, String>> {
            let rec = Arc::clone(&self.0);
            rec.log.lock().unwrap().push(format!("start {}", r.url));
            rec.requests.lock().unwrap().push(r.clone());
            Box::pin(async move {
                // The first request is the slowest: a concurrent dispatcher
                // would finish it last.
                let ms = if r.url.ends_with("/1") { 40 } else { 5 };
                tokio::time::sleep(Duration::from_millis(ms)).await;
                rec.log.lock().unwrap().push(format!("end {}", r.url));
                Ok(HostResponse {
                    status: 200,
                    body: Vec::new(),
                    set_cookies: vec!["fresh=1; Domain=.example.test; Path=/".into()],
                })
            })
        }
    }

    #[derive(Default)]
    struct Hooks {
        stored: std::sync::Mutex<Vec<(String, Vec<String>)>>,
    }
    struct HookRef(Arc<Hooks>);
    impl RequestHooks for HookRef {
        fn wait_user_agent(&self) -> BoxFuture<()> {
            Box::pin(async {})
        }
        fn user_agent(&self) -> Option<String> {
            Some("Final UA".into())
        }
        fn cookie_header(&self, url: &str) -> BoxFuture<Option<String>> {
            let found = url
                .ends_with("/1")
                .then(|| "euconsent-v2=CQ; _pubcid=p".to_owned());
            Box::pin(async move { found })
        }
        fn store_cookies(&self, url: &str, set_cookies: Vec<String>) -> BoxFuture<()> {
            self.0
                .stored
                .lock()
                .unwrap()
                .push((url.to_owned(), set_cookies));
            Box::pin(async {})
        }
    }

    fn request(url: &str) -> HostRequest {
        HostRequest {
            method: Method::Get,
            url: url.to_owned(),
            headers: vec![
                ("user-agent", "Fallback UA".to_owned()),
                ("priority", "u=4, i".to_owned()),
            ],
            body: None,
            timeout: None,
        }
    }

    #[test]
    fn analytics_requests_go_one_after_another_in_call_order() {
        tauri::async_runtime::block_on(async {
            let rec = Arc::new(Recorder::default());
            let d = Dispatcher::new(Arc::new(Recording(Arc::clone(&rec))));
            let pending: Vec<_> = (1..=3)
                .map(|i| d.send(request(&format!("http://127.0.0.1/{i}")), true))
                .collect();
            for p in pending {
                assert_eq!(p.await.unwrap().status, 200);
            }
            let log = rec.log.lock().unwrap().clone();
            let want: Vec<String> = (1..=3)
                .flat_map(|i| {
                    [
                        format!("start http://127.0.0.1/{i}"),
                        format!("end http://127.0.0.1/{i}"),
                    ]
                })
                .collect();
            assert_eq!(log, want);
            assert_eq!(d.in_flight(), 0);
        });
    }

    #[test]
    fn hooks_set_the_final_ua_add_cookies_last_and_store_set_cookie() {
        tauri::async_runtime::block_on(async {
            let rec = Arc::new(Recorder::default());
            let hooks = Arc::new(Hooks::default());
            let d = Dispatcher::new(Arc::new(Recording(Arc::clone(&rec))));
            d.set_hooks(Arc::new(HookRef(Arc::clone(&hooks))));
            d.send(request("http://127.0.0.1/1"), true).await.unwrap();
            // The consent request (untracked) gets the same treatment.
            d.send(request("http://127.0.0.1/2"), false).await.unwrap();
            let sent = rec.requests.lock().unwrap().clone();
            assert_eq!(
                sent[0].headers,
                vec![
                    ("user-agent", "Final UA".to_owned()),
                    ("priority", "u=4, i".to_owned()),
                    ("cookie", "euconsent-v2=CQ; _pubcid=p".to_owned()),
                ]
            );
            // No cookies for the URL: no cookie header at all.
            assert!(!sent[1].headers.iter().any(|(n, _)| *n == "cookie"));
            let stored = hooks.stored.lock().unwrap().clone();
            assert_eq!(stored.len(), 2);
            assert_eq!(stored[0].0, "http://127.0.0.1/1");
            assert_eq!(
                stored[0].1,
                vec!["fresh=1; Domain=.example.test; Path=/".to_owned()]
            );
        });
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
