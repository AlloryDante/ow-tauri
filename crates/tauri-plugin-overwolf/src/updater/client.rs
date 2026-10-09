//! The update client's HTTP layer (DESIGN §4.14, SEC-M10; ADR 0022).
//!
//! It stays on reqwest, while analytics and consent use the plain hyper
//! client (header order and the missing `accept: */*` are wire parity
//! there). Both sit on the same hyper and native-tls crates. reqwest gives
//! the updater what it relies on:
//!
//! - redirects followed by hand, at most [`MAX_REDIRECTS`], each one to
//!   `https` (or loopback `http` in debug builds);
//! - custom headers sent to the feed origin only, dropped for good after a
//!   cross-origin redirect, and never sent with a download;
//! - a connect timeout and an **idle** read timeout (between bytes, not
//!   for the whole body), so a slow, large installer still downloads;
//! - transparent decompression, with every size cap counted on the
//!   decompressed bytes;
//! - the system proxy.

use std::time::Duration;

use reqwest::header::LOCATION;
use url::Url;

use super::check_transport;
use crate::error::Error;

/// The most redirects one request follows (electron-updater and browsers
/// allow 10 too).
pub(crate) const MAX_REDIRECTS: usize = 10;

/// How a client reaches the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProxyChoice {
    /// The system proxy (environment and OS settings), as Chromium does.
    System,
    /// One proxy for every request (tests: a local proxy, a refusing one).
    #[cfg(test)]
    Fixed(String),
}

/// The client settings of one check or download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpSettings {
    /// The TCP and TLS connect timeout.
    pub(crate) connect_timeout: Duration,
    /// The idle read timeout: the longest wait for the next bytes.
    pub(crate) read_timeout: Duration,
    /// A debug build: loopback `http` is allowed.
    pub(crate) debug: bool,
    /// The proxy.
    pub(crate) proxy: ProxyChoice,
}

/// A reqwest client with the updater's policy.
#[derive(Debug, Clone)]
pub(crate) struct Http {
    client: reqwest::Client,
    debug: bool,
}

fn network(err: &reqwest::Error) -> Error {
    let what = if err.is_timeout() {
        "The update server did not answer in time."
    } else if err.is_decode() || err.is_body() {
        "The update server's answer broke off."
    } else {
        "The update server could not be reached."
    };
    Error::network(what, err.status().map(|s| s.as_u16()))
}

impl Http {
    /// A client for `settings`.
    ///
    /// # Errors
    ///
    /// `network` when the TLS stack cannot start.
    pub(crate) fn new(settings: &HttpSettings) -> Result<Self, Error> {
        let builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(settings.connect_timeout)
            .read_timeout(settings.read_timeout);
        let builder = match &settings.proxy {
            ProxyChoice::System => builder,
            #[cfg(test)]
            ProxyChoice::Fixed(proxy) => builder.proxy(
                reqwest::Proxy::all(proxy.as_str())
                    .map_err(|_| Error::invalid_argument("bad test proxy"))?,
            ),
        };
        let client = builder
            .build()
            .map_err(|_| Error::network("The HTTP client could not start.", None))?;
        Ok(Http {
            client,
            debug: settings.debug,
        })
    }

    /// `GET url` with the redirect and header rules: `headers` go to
    /// `url`'s origin only, and once a redirect leaves that origin they are
    /// never sent again. A non-2xx answer is a `network` error with its
    /// status.
    ///
    /// # Errors
    ///
    /// `network` for a transport failure, a refused or 11th redirect, or a
    /// non-2xx status.
    pub(crate) async fn get(
        &self,
        url: &Url,
        headers: &[(String, String)],
    ) -> Result<reqwest::Response, Error> {
        check_transport(url, self.debug)
            .map_err(|e| Error::network(e.to_string().replace("invalid argument: ", ""), None))?;
        let origin = url.origin();
        let mut current = url.clone();
        let mut send_headers = !headers.is_empty();
        let mut redirects = 0;
        loop {
            send_headers &= current.origin() == origin;
            let mut request = self.client.get(current.clone());
            if send_headers {
                for (name, value) in headers {
                    request = request.header(name.as_str(), value.as_str());
                }
            }
            let response = request.send().await.map_err(|e| network(&e))?;
            let status = response.status();
            if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
                let next = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|l| l.to_str().ok())
                    .and_then(|l| current.join(l).ok())
                    .ok_or_else(|| {
                        Error::network(
                            "The update server sent a redirect without a valid location.",
                            Some(status.as_u16()),
                        )
                    })?;
                redirects += 1;
                if redirects > MAX_REDIRECTS {
                    return Err(Error::network(
                        "The update server redirected too many times.",
                        Some(status.as_u16()),
                    ));
                }
                if check_transport(&next, self.debug).is_err() {
                    return Err(Error::network(
                        "The update server redirected to a URL that is not https.",
                        Some(status.as_u16()),
                    ));
                }
                current = next;
                continue;
            }
            if !status.is_success() {
                return Err(Error::network(
                    format!("The update server answered HTTP {}.", status.as_u16()),
                    Some(status.as_u16()),
                ));
            }
            return Ok(response);
        }
    }
}

/// Reads a small body (the feed, a `.sig`), refusing more than `cap`
/// decompressed bytes.
///
/// # Errors
///
/// `network` when the body breaks off, `invalid-argument` past `cap`.
pub(crate) async fn read_capped(
    mut response: reqwest::Response,
    cap: usize,
) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| network(&e))? {
        if out.len() + chunk.len() > cap {
            return Err(Error::invalid_argument(
                "The update server sent too much data.",
            ));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// The next body chunk, decompressed.
///
/// # Errors
///
/// `network` when the body breaks off or stalls past the idle timeout.
pub(crate) async fn next_chunk(
    response: &mut reqwest::Response,
) -> Result<Option<bytes::Bytes>, Error> {
    response.chunk().await.map_err(|e| network(&e))
}

#[cfg(test)]
pub(crate) mod tests {
    //! A tiny HTTP/1.1 server for the client tests and the engine tests.

    use std::fmt::Write as _;
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::error::ErrorCode;

    /// One recorded request: its request line and lower-cased headers.
    #[derive(Debug, Clone)]
    pub(crate) struct Seen {
        pub(crate) line: String,
        pub(crate) headers: Vec<(String, String)>,
    }

    impl Seen {
        pub(crate) fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        }

        pub(crate) fn path(&self) -> &str {
            self.line.split(' ').nth(1).unwrap_or_default()
        }
    }

    /// What the server writes back: it gets the connection and the request.
    pub(crate) type Handler = dyn Fn(&mut TcpStream, &Seen) + Send + Sync;

    /// A loopback server answering every connection with `handler`.
    pub(crate) struct Server {
        pub(crate) base: String,
        pub(crate) seen: Arc<Mutex<Vec<Seen>>>,
    }

    impl Server {
        pub(crate) fn start(handler: Arc<Handler>) -> Server {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let seen = Arc::new(Mutex::new(Vec::new()));
            let log = Arc::clone(&seen);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { return };
                    let handler = Arc::clone(&handler);
                    let log = Arc::clone(&log);
                    std::thread::spawn(move || {
                        let Some(req) = read_request(&mut stream) else {
                            return;
                        };
                        log.lock().unwrap().push(req.clone());
                        handler(&mut stream, &req);
                        let _ = stream.flush();
                    });
                }
            });
            Server { base, seen }
        }

        pub(crate) fn url(&self, path: &str) -> Url {
            Url::parse(&format!("{}{path}", self.base)).unwrap()
        }

        pub(crate) fn seen(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }
    }

    fn read_request(stream: &mut TcpStream) -> Option<Seen> {
        let mut reader = BufReader::new(stream.try_clone().ok()?);
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let mut headers = Vec::new();
        loop {
            let mut h = String::new();
            reader.read_line(&mut h).ok()?;
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                headers.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
            }
        }
        Some(Seen {
            line: line.trim_end().to_owned(),
            headers,
        })
    }

    /// Writes a complete response.
    pub(crate) fn respond(stream: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &[u8]) {
        let mut head = format!(
            "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n",
            body.len()
        );
        for (k, v) in headers {
            let _ = write!(head, "{k}: {v}\r\n");
        }
        head.push_str("\r\n");
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body);
    }

    pub(crate) fn settings(read_ms: u64) -> HttpSettings {
        HttpSettings {
            connect_timeout: Duration::from_secs(5),
            read_timeout: Duration::from_millis(read_ms),
            debug: true,
            proxy: ProxyChoice::System,
        }
    }

    fn block<T>(f: impl Future<Output = T>) -> T {
        tauri::async_runtime::block_on(f)
    }

    fn redirector(target: impl Fn(&Seen) -> String + Send + Sync + 'static) -> Arc<Handler> {
        Arc::new(move |s, req| {
            let to = target(req);
            respond(s, "302 Found", &[("location", &to)], b"");
        })
    }

    /// SEC-M10: at most 10 redirects, each to an allowed scheme.
    #[test]
    fn redirect_count_and_scheme() {
        let done = Server::start(Arc::new(|s, _| respond(s, "200 OK", &[], b"ok")));
        let done_base = done.base.clone();
        let hop = Server::start(redirector(move |req| {
            let n: usize = req.path().trim_start_matches("/r").parse().unwrap_or(0);
            if n == 0 {
                format!("{done_base}/ok")
            } else {
                format!("/r{}", n - 1)
            }
        }));
        let http = Http::new(&settings(5_000)).unwrap();
        // /r9 → … → /r0 → done: 10 redirects pass.
        let body = block(async {
            let r = http.get(&hop.url("/r9"), &[]).await?;
            read_capped(r, 10).await
        })
        .unwrap();
        assert_eq!(body, b"ok");
        // /r10: the 11th redirect fails.
        let err = block(http.get(&hop.url("/r10"), &[])).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Network);
        assert!(err.to_string().contains("too many"), "{err}");
        // A redirect to plain http on a non-loopback host, or to another
        // scheme, is refused without being followed.
        for to in [
            "http://example.invalid/x",
            "ftp://127.0.0.1/x",
            "file:///etc/hosts",
        ] {
            let bad = Server::start(redirector(move |_| to.to_owned()));
            let err = block(http.get(&bad.url("/"), &[])).unwrap_err();
            assert!(err.to_string().contains("not https"), "{to}: {err}");
        }
        // In a release build loopback http is refused before any request.
        let release = Http::new(&HttpSettings {
            debug: false,
            ..settings(5_000)
        })
        .unwrap();
        let n = done.seen().len();
        assert!(block(release.get(&done.url("/ok"), &[])).is_err());
        assert_eq!(done.seen().len(), n);
    }

    /// SEC-M10: custom headers reach the feed origin only, and never again
    /// once a redirect left it.
    #[test]
    fn headers_are_dropped_off_the_feed_origin() {
        let feed_base = Arc::new(Mutex::new(String::new()));
        let back = Arc::clone(&feed_base);
        let other = Server::start(Arc::new(move |s, req| {
            if req.path() == "/to-feed" {
                let to = format!("{}/final", back.lock().unwrap());
                respond(s, "302 Found", &[("location", &to)], b"");
            } else {
                respond(s, "200 OK", &[], b"other");
            }
        }));
        let other_base = other.base.clone();
        let feed = Server::start(Arc::new(move |s, req| match req.path() {
            "/away" => respond(
                s,
                "302 Found",
                &[("location", &format!("{other_base}/x"))],
                b"",
            ),
            "/bounce" => respond(
                s,
                "302 Found",
                &[("location", &format!("{other_base}/to-feed"))],
                b"",
            ),
            "/same" => respond(s, "302 Found", &[("location", "/final")], b""),
            _ => respond(s, "200 OK", &[], b"feed"),
        }));
        *feed_base.lock().unwrap() = feed.base.clone();
        let http = Http::new(&settings(5_000)).unwrap();
        let h = vec![("X-Token".to_owned(), "secret".to_owned())];
        // Same-origin redirect: both requests carry the header.
        block(http.get(&feed.url("/same"), &h)).unwrap();
        let seen = feed.seen();
        assert_eq!(seen.len(), 2);
        assert!(seen.iter().all(|r| r.header("x-token") == Some("secret")));
        // Cross-origin: the other origin never sees it.
        block(http.get(&feed.url("/away"), &h)).unwrap();
        assert_eq!(other.seen()[0].header("x-token"), None);
        // feed → other → feed: not sent on the way back either.
        block(http.get(&feed.url("/bounce"), &h)).unwrap();
        let seen = feed.seen();
        let last = seen.last().unwrap();
        assert_eq!(last.path(), "/final");
        assert_eq!(last.header("x-token"), None);
        assert!(other.seen().iter().all(|r| r.header("x-token").is_none()));
    }

    /// SEC-M10: the read timeout is idle, not total.
    #[test]
    fn idle_timeout_not_total() {
        const MB: usize = 1024 * 1024;
        let server = Server::start(Arc::new(|s, req| {
            let stall = req.path() == "/stall";
            let total = if stall { MB } else { 50 * MB };
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {total}\r\nconnection: close\r\n\r\n"
            );
            let _ = s.write_all(head.as_bytes());
            let chunk = vec![7_u8; MB / 2];
            for i in 0..(total / chunk.len()) {
                if stall && i == 1 {
                    std::thread::sleep(Duration::from_millis(1_500));
                } else {
                    std::thread::sleep(Duration::from_millis(30));
                }
                if s.write_all(&chunk).is_err() {
                    return;
                }
            }
        }));
        // 100 chunks 30 ms apart: about 3 s in all, far over the 400 ms
        // idle timeout, which never fires while bytes flow.
        let http = Http::new(&settings(400)).unwrap();
        let started = std::time::Instant::now();
        let got = block(async {
            let mut r = http.get(&server.url("/big"), &[]).await?;
            let mut n = 0;
            while let Some(c) = next_chunk(&mut r).await? {
                n += c.len();
            }
            Ok::<_, Error>(n)
        })
        .unwrap();
        assert_eq!(got, 50 * MB);
        assert!(started.elapsed() > Duration::from_millis(400));
        // A stall longer than the idle timeout fails.
        let err = block(async {
            let mut r = http.get(&server.url("/stall"), &[]).await?;
            while next_chunk(&mut r).await?.is_some() {}
            Ok::<_, Error>(())
        })
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::Network);
        assert!(err.to_string().contains("in time") || err.to_string().contains("broke off"), "{err}");
    }

    /// SEC-M10: size caps count decompressed bytes.
    #[test]
    fn size_cap_after_decompression() {
        use async_compression::tokio::write::GzipEncoder;
        use tokio::io::AsyncWriteExt as _;
        let plain = vec![b'a'; 2 * 1024 * 1024];
        let gz = block(async {
            let mut enc = GzipEncoder::new(Vec::new());
            enc.write_all(&plain).await.unwrap();
            enc.shutdown().await.unwrap();
            enc.into_inner()
        });
        assert!(gz.len() < 64 * 1024);
        let server = Server::start(Arc::new(move |s, _| {
            respond(s, "200 OK", &[("content-encoding", "gzip")], &gz);
        }));
        let http = Http::new(&settings(5_000)).unwrap();
        let err = block(async {
            let r = http.get(&server.url("/feed.yml"), &[]).await?;
            read_capped(r, 1024 * 1024).await
        })
        .unwrap_err();
        assert!(err.to_string().contains("too much"), "{err}");
        let ok = block(async {
            let r = http.get(&server.url("/feed.yml"), &[]).await?;
            read_capped(r, 4 * 1024 * 1024).await
        })
        .unwrap();
        assert_eq!(ok.len(), plain.len());
        // The client asked for compressed bodies.
        assert!(server.seen()[0].header("accept-encoding").is_some_and(|v| v.contains("gzip")));
    }

    /// SEC-M10, §7.4: requests go through the proxy; a refusing proxy is a
    /// `network` error, never a panic.
    #[test]
    fn proxy_is_used_and_refusal_is_an_error() {
        let proxy = Server::start(Arc::new(|s, _| respond(s, "200 OK", &[], b"via proxy")));
        let http = Http::new(&HttpSettings {
            proxy: ProxyChoice::Fixed(proxy.base.clone()),
            ..settings(5_000)
        })
        .unwrap();
        let body = block(async {
            let r = http.get(&Url::parse("http://localhost:9/feed.yml").unwrap(), &[]).await?;
            read_capped(r, 100).await
        })
        .unwrap();
        assert_eq!(body, b"via proxy");
        let seen = proxy.seen();
        assert!(seen[0].line.starts_with("GET http://localhost:9/feed.yml"), "{:?}", seen[0]);
        let refusing = Http::new(&HttpSettings {
            proxy: ProxyChoice::Fixed("http://127.0.0.1:9".into()),
            ..settings(5_000)
        })
        .unwrap();
        let err = block(refusing.get(&Url::parse("http://localhost:9/x").unwrap(), &[])).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Network);
    }

    /// Non-2xx answers carry their status.
    #[test]
    fn status_errors() {
        let server = Server::start(Arc::new(|s, _| respond(s, "404 Not Found", &[], b"")));
        let http = Http::new(&settings(5_000)).unwrap();
        match block(http.get(&server.url("/latest.yml"), &[])).unwrap_err() {
            Error::Network { status, .. } => assert_eq!(status, Some(404)),
            other => panic!("{other:?}"),
        }
    }
}
