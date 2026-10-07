//! Sending host requests: the [`Transport`] trait, the default `hyper`
//! transport, and the dispatcher that starts analytics requests in call
//! order, adds the final `<UA>`, and lets an exit drain them (CONTRACT
//! E.1, A.6).

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

/// The default transport: a `hyper` client over native TLS with ALPN
/// (HTTP/2 when the server offers it, as Chromium negotiates it), the
/// system proxy (HTTPS through an HTTP `CONNECT` tunnel), no cookie jar, and
/// decoding of gzip, deflate, brotli and zstd responses.
///
/// It puts exactly the request's headers on the wire, in order, and adds
/// none (E.1): a higher-level client such as `reqwest` always adds
/// `accept: */*`, which ow-electron does not send (observed). Over HTTP/1.1
/// hyper appends `host`; over HTTP/2 the authority travels as `:authority`.
/// In test builds (`cfg(test)` or the `test-util` feature) it refuses every
/// host that is not loopback.
#[derive(Clone)]
pub(crate) struct HyperTransport {
    client: Option<HyperClient>,
}

type HyperClient = hyper_util::client::legacy::Client<
    hyper_tls::HttpsConnector<ProxyConnector>,
    http_body_util::Full<bytes::Bytes>,
>;

impl std::fmt::Debug for HyperTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HyperTransport")
            .field("ready", &self.client.is_some())
            .finish()
    }
}

impl HyperTransport {
    pub(crate) fn new() -> Self {
        let tls = native_tls::TlsConnector::builder()
            .request_alpns(&["h2", "http/1.1"])
            .build()
            .ok();
        let client = tls.map(|tls| {
            let mut http = hyper_util::client::legacy::connect::HttpConnector::new();
            http.enforce_http(false);
            let proxy = ProxyConnector {
                http,
                matcher: Arc::new(hyper_util::client::proxy::matcher::Matcher::from_system()),
            };
            let https = hyper_tls::HttpsConnector::from((proxy, tls.into()));
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .build(https)
        });
        HyperTransport { client }
    }
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// TCP connections for the client: direct, or through the system proxy's
/// `CONNECT` tunnel for HTTPS destinations it intercepts (the proxies
/// `reqwest`'s `system-proxy` used, from the same matcher).
#[derive(Clone)]
struct ProxyConnector {
    http: hyper_util::client::legacy::connect::HttpConnector,
    matcher: Arc<hyper_util::client::proxy::matcher::Matcher>,
}

impl tower_service::Service<http::Uri> for ProxyConnector {
    type Response = hyper_util::rt::TokioIo<tokio::net::TcpStream>;
    type Error = BoxError;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.http.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, dst: http::Uri) -> Self::Future {
        let tunnel = (dst.scheme_str() == Some("https"))
            .then(|| self.matcher.intercept(&dst))
            .flatten()
            .filter(|i| i.uri().scheme_str() == Some("http"));
        let mut http = self.http.clone();
        match tunnel {
            Some(intercept) => {
                let mut tunnel = hyper_util::client::legacy::connect::proxy::Tunnel::new(
                    intercept.uri().clone(),
                    http,
                );
                if let Some(auth) = intercept.basic_auth() {
                    tunnel = tunnel.with_auth(auth.clone());
                }
                Box::pin(async move { tunnel.call(dst).await.map_err(Into::into) })
            }
            None => Box::pin(async move { http.call(dst).await.map_err(Into::into) }),
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

/// Decodes a response body by its `content-encoding` (the encodings the
/// request's `accept-encoding` offers, E.1); others pass through.
async fn decode_body(encoding: Option<&str>, body: &[u8]) -> Result<Vec<u8>, String> {
    use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZlibDecoder, ZstdDecoder};
    use tokio::io::AsyncReadExt as _;
    let mut out = Vec::new();
    let read = match encoding.map(|e| e.trim().to_ascii_lowercase()).as_deref() {
        Some("gzip" | "x-gzip") => GzipDecoder::new(body).read_to_end(&mut out).await,
        Some("deflate") => ZlibDecoder::new(body).read_to_end(&mut out).await,
        Some("br") => BrotliDecoder::new(body).read_to_end(&mut out).await,
        Some("zstd") => ZstdDecoder::new(body).read_to_end(&mut out).await,
        _ => return Ok(body.to_vec()),
    };
    read.map(|_| out)
        .map_err(|e| format!("decoding the response failed: {e}"))
}

/// Builds the `http` request: the method, the URL, the headers in order,
/// the body.
fn http_request(
    request: &HostRequest,
) -> Result<http::Request<http_body_util::Full<bytes::Bytes>>, String> {
    let mut builder = http::Request::builder()
        .method(match request.method {
            Method::Get => http::Method::GET,
            Method::Post => http::Method::POST,
        })
        .uri(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(*name, value);
    }
    builder
        .body(http_body_util::Full::new(bytes::Bytes::from(
            request.body.clone().unwrap_or_default(),
        )))
        .map_err(|e| e.to_string())
}

impl Transport for HyperTransport {
    fn send(&self, request: HostRequest) -> BoxFuture<Result<HostResponse, String>> {
        let Some(client) = self.client.clone() else {
            return Box::pin(async { Err("HTTP client unavailable".to_owned()) });
        };
        if !allowed_host(&request.url) {
            return Box::pin(async { Err("non-loopback host refused in a test build".to_owned()) });
        }
        let lab_id = lab_request_start(&request);
        let timeout = request.timeout;
        Box::pin(async move {
            let out = async {
                use http_body_util::BodyExt as _;
                let req = http_request(&request)?;
                let exchange = async {
                    let response = client.request(req).await.map_err(|e| e.to_string())?;
                    let (parts, body) = response.into_parts();
                    let body = body.collect().await.map_err(|e| e.to_string())?.to_bytes();
                    Ok::<_, String>((parts, body))
                };
                let (parts, body) = match timeout {
                    Some(limit) => tokio::time::timeout(limit, exchange)
                        .await
                        .map_err(|_| "the request timed out".to_owned())??,
                    None => exchange.await?,
                };
                let encoding = parts
                    .headers
                    .get(http::header::CONTENT_ENCODING)
                    .and_then(|v| v.to_str().ok());
                let body = decode_body(encoding, &body).await?;
                let set_cookies = parts
                    .headers
                    .get_all(http::header::SET_COOKIE)
                    .iter()
                    .filter_map(|v| v.to_str().ok().map(str::to_owned))
                    .collect();
                Ok((
                    HostResponse {
                        status: parts.status.as_u16(),
                        body,
                        set_cookies,
                    },
                    format!("{:?}", parts.version),
                ))
            }
            .await;
            lab_request_end(lab_id, &out);
            out.map(|(response, _)| response)
        })
    }
}

/// The header fields of `request` in the order this transport puts them on
/// the wire: exactly the request's own (`HTTP/2` pseudo-headers precede
/// them; over HTTP/1.1 hyper appends `host`).
pub(crate) fn wire_headers(request: &HostRequest) -> Vec<(String, String)> {
    request
        .headers
        .iter()
        .map(|(n, v)| ((*n).to_owned(), v.clone()))
        .collect()
}

/// Lab trace: a host request leaves (`host-requests.jsonl`).
fn lab_request_start(request: &HostRequest) -> u64 {
    if !crate::lab::trace_on() {
        return 0;
    }
    let id = crate::lab::next_id();
    crate::lab::record("host-requests.jsonl", || {
        let headers = wire_headers(request);
        let cookies = headers
            .iter()
            .find(|(n, _)| n == "cookie")
            .map(|(_, v)| crate::lab::cookie_names(v))
            .unwrap_or_default();
        serde_json::json!({
            "phase": "start",
            "id": id,
            "method": match request.method { Method::Get => "GET", Method::Post => "POST" },
            "url": request.url,
            "sentHeaders": headers.iter().map(|(n, v)| format!("{n}: {v}")).collect::<Vec<_>>(),
            "cookiesSent": cookies,
            "uploadBody": request.body.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()),
            "timeoutMs": request.timeout.map(|t| t.as_millis()),
        })
    });
    id
}

/// Lab trace: the outcome of the host request `id`.
fn lab_request_end(id: u64, out: &Result<(HostResponse, String), String>) {
    if id == 0 {
        return;
    }
    crate::lab::record("host-requests.jsonl", || match out {
        Ok((r, version)) => serde_json::json!({
            "phase": "end",
            "id": id,
            "status": r.status,
            "protocol": version,
            "setCookies": r.set_cookies.iter().map(|c| c.split('=').next().unwrap_or_default().to_owned()).collect::<Vec<_>>(),
            "responseBody": String::from_utf8_lossy(&r.body[..r.body.len().min(2048)]),
        }),
        Err(e) => serde_json::json!({ "phase": "end", "id": id, "error": e }),
    });
}

/// How long the dispatcher lets one analytics request run before it starts
/// the next one, unless the request finishes sooner. Requests start in call
/// order and the launch sequence leaves within about 100 ms, as in
/// ow-electron (E.2, observed: six requests within 5 ms of each other); a
/// fast response still finishes before the next request starts.
pub(crate) const ORDER_WINDOW: Duration = Duration::from_millis(2);

/// What the host adds to each request just before it goes out (E.1): the
/// final `<UA>`. Host requests carry no cookies and store none, as
/// ow-electron's do not (observed: every cookie excluded by the request's
/// credentials mode, no `Set-Cookie` stored).
pub(crate) trait RequestHooks: Send + Sync + 'static {
    /// Resolves once `<UA>` is final, or when waiting for it is over.
    fn wait_user_agent(&self) -> BoxFuture<()>;
    /// `<UA>` as currently known.
    fn user_agent(&self) -> Option<String>;
}

/// Applies `hooks` to `request`: the final `<UA>` replaces the one it was
/// built with.
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
}

/// Sends `request`.
async fn perform(
    transport: Arc<dyn Transport>,
    request: HostRequest,
) -> Result<HostResponse, String> {
    transport.send(request).await
}

/// One analytics request waiting for its turn.
struct Job {
    request: HostRequest,
    guard: Option<InFlight>,
    reply: tokio::sync::oneshot::Sender<Result<HostResponse, String>>,
}

/// Sends host requests: analytics requests in call order (each starts when
/// the previous one finished, or [`ORDER_WINDOW`] after it started); the
/// consent request at once, in parallel with them (E.2 #2). Tracks the analytics
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
                        let out = perform(transport, request).await;
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
                let _ = reply.send(perform(transport, request).await);
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
        let t = HyperTransport::new();
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
                // The first request is slow (a cold connection); the others
                // answer at once.
                if r.url.ends_with("/1") {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
                rec.log.lock().unwrap().push(format!("end {}", r.url));
                Ok(HostResponse {
                    status: 200,
                    body: Vec::new(),
                    set_cookies: vec!["fresh=1; Domain=.example.test; Path=/".into()],
                })
            })
        }
    }

    struct HookRef;
    impl RequestHooks for HookRef {
        fn wait_user_agent(&self) -> BoxFuture<()> {
            Box::pin(async {})
        }
        fn user_agent(&self) -> Option<String> {
            Some("Final UA".into())
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

    /// Regression (lab diff): the launch sequence waited up to 1 s per
    /// response and spread over seconds; ow-electron starts it within a few
    /// milliseconds (E.2).
    #[test]
    fn analytics_requests_start_in_call_order_without_waiting_for_slow_responses() {
        tauri::async_runtime::block_on(async {
            let rec = Arc::new(Recorder::default());
            let d = Dispatcher::new(Arc::new(Recording(Arc::clone(&rec))));
            let started = std::time::Instant::now();
            let pending: Vec<_> = (1..=3)
                .map(|i| d.send(request(&format!("http://127.0.0.1/{i}")), true))
                .collect();
            for p in pending {
                assert_eq!(p.await.unwrap().status, 200);
            }
            assert!(started.elapsed() < Duration::from_millis(1000));
            let log = rec.log.lock().unwrap().clone();
            let at = |entry: &str| log.iter().position(|l| l == entry).unwrap();
            // Started in call order...
            assert!(at("start http://127.0.0.1/1") < at("start http://127.0.0.1/2"));
            assert!(at("start http://127.0.0.1/2") < at("start http://127.0.0.1/3"));
            // ...all of them while the slow first one was still running...
            assert_eq!(log.last().unwrap(), "end http://127.0.0.1/1");
            // ...and a fast response finishes before the next request starts.
            assert!(at("end http://127.0.0.1/2") < at("start http://127.0.0.1/3"));
            assert_eq!(d.in_flight(), 0);
        });
    }

    /// Regression (lab diff): host requests carried the ads data store's
    /// cookies; ow-electron's send none and store none (observed).
    #[test]
    fn hooks_set_the_final_ua_and_requests_carry_no_cookies() {
        tauri::async_runtime::block_on(async {
            let rec = Arc::new(Recorder::default());
            let d = Dispatcher::new(Arc::new(Recording(Arc::clone(&rec))));
            d.set_hooks(Arc::new(HookRef));
            d.send(request("http://127.0.0.1/1"), true).await.unwrap();
            // The consent request (untracked) gets the same treatment.
            d.send(request("http://127.0.0.1/2"), false).await.unwrap();
            let sent = rec.requests.lock().unwrap().clone();
            let want = vec![
                ("user-agent", "Final UA".to_owned()),
                ("priority", "u=4, i".to_owned()),
            ];
            assert_eq!(sent[0].headers, want);
            assert_eq!(sent[1].headers, want);
        });
    }

    /// One HTTP/1.1 exchange on a loopback socket: returns the request
    /// head and body the server read, and answers with `response`.
    fn serve_once(response: Vec<u8>) -> (String, std::thread::JoinHandle<(String, Vec<u8>)>) {
        use std::io::{BufRead as _, BufReader, Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/tracking/InsertStats?Stats=true",
            listener.local_addr().unwrap()
        );
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                head.push_str(&line);
            }
            let length = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length: "))
                .map_or(0, |v| v.trim().parse::<usize>().unwrap());
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let mut stream = stream;
            stream.write_all(&response).unwrap();
            (head, body)
        });
        (url, handle)
    }

    /// Regression (lab diff): the wire carried `accept: */*` (added by the
    /// previous HTTP client) and `content-length` last; ow-electron sends
    /// exactly the E.1 fields, `content-length` first.
    #[test]
    fn the_transport_sends_exactly_the_request_headers_and_decodes_gzip() {
        // gzip of `{"ok":1}`.
        const GZIP: [u8; 28] = [
            0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff, 0xab, 0x56, 0xca, 0xcf,
            0x56, 0xb2, 0x32, 0xac, 0x05, 0x00, 0x09, 0x8c, 0x54, 0x10, 0x08, 0x00, 0x00, 0x00,
        ];
        let mut response = b"HTTP/1.1 200 OK\r\ncontent-encoding: gzip\r\nset-cookie: a=1\r\ncontent-length: 28\r\n\r\n".to_vec();
        response.extend_from_slice(&GZIP);
        let (url, server) = serve_once(response);
        let body = br#"{"Kind":400022}"#.to_vec();
        let request = HostRequest {
            method: Method::Post,
            url: url.clone(),
            headers: vec![
                ("content-length", body.len().to_string()),
                ("content-type", "application/json".to_owned()),
                ("sec-fetch-site", "none".to_owned()),
                ("user-agent", "UA".to_owned()),
                ("accept-encoding", "gzip, deflate, br, zstd".to_owned()),
                ("priority", "u=4, i".to_owned()),
            ],
            body: Some(body.clone()),
            timeout: Some(Duration::from_secs(10)),
        };
        let out = tauri::async_runtime::block_on(HyperTransport::new().send(request)).unwrap();
        assert_eq!(out.status, 200);
        assert_eq!(out.body, br#"{"ok":1}"#);
        assert_eq!(out.set_cookies, vec!["a=1".to_owned()]);
        let (head, sent) = server.join().unwrap();
        let host = url
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .to_owned();
        let want = [
            "POST /tracking/InsertStats?Stats=true HTTP/1.1".to_owned(),
            "content-length: 15".to_owned(),
            "content-type: application/json".to_owned(),
            "sec-fetch-site: none".to_owned(),
            "user-agent: UA".to_owned(),
            "accept-encoding: gzip, deflate, br, zstd".to_owned(),
            "priority: u=4, i".to_owned(),
            format!("host: {host}"),
        ];
        assert_eq!(head.lines().collect::<Vec<_>>(), want);
        assert_eq!(sent, body);
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
