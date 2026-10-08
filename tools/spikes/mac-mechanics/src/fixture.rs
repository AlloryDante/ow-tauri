//! Loopback fixture server (127.0.0.1, random port). Serves the fixture
//! pages and records every request (path, `User-Agent`, time). Guests in this
//! spike load only these pages: never an ad, never the network.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

const GUEST: &str = include_str!("../fixtures/guest.html");
const STORAGE: &str = include_str!("../fixtures/storage.html");
const POPUP: &str = include_str!("../fixtures/popup.html");

/// A running fixture server.
#[derive(Clone)]
pub struct Fixture {
    pub port: u16,
    log: Arc<Mutex<Vec<Value>>>,
}

impl Fixture {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("addr").port();
        let fixture = Self {
            port,
            log: Arc::new(Mutex::new(Vec::new())),
        };
        let f = fixture.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let f = f.clone();
                std::thread::spawn(move || f.serve(stream));
            }
        });
        fixture
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// Every recorded request so far.
    pub fn requests(&self) -> Vec<Value> {
        self.log.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// Recorded beacons whose `k` query value equals `k`.
    pub fn beacons(&self, k: &str) -> Vec<Value> {
        self.requests().into_iter().filter(|r| r["beacon"]["k"] == k).collect()
    }

    fn serve(&self, stream: TcpStream) {
        let mut reader = BufReader::new(match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        });
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        let mut ua = String::new();
        let mut origin = String::new();
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                break;
            }
            let lower = h.to_ascii_lowercase();
            if lower.starts_with("user-agent:") {
                ua = h[11..].trim().to_owned();
            } else if lower.starts_with("origin:") {
                origin = h[7..].trim().to_owned();
            }
        }
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or("").to_owned();
        let target = parts.next().unwrap_or("/").to_owned();
        let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
        let beacon: serde_json::Map<String, Value> = query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.to_owned(), Value::String(decode(v))))
            .collect();
        if let Ok(mut l) = self.log.lock() {
            l.push(json!({ "t": crate::t(), "method": method, "path": path, "query": query, "userAgent": ua, "origin": origin, "beacon": if path == "/beacon" { Value::Object(beacon) } else { Value::Null } }));
        }
        let (status, ctype, body) = match path {
            "/guest.html" => ("200 OK", "text/html; charset=utf-8", GUEST),
            "/storage.html" => ("200 OK", "text/html; charset=utf-8", STORAGE),
            "/popup.html" => ("200 OK", "text/html; charset=utf-8", POPUP),
            "/beacon" => ("204 No Content", "text/plain", ""),
            _ => ("404 Not Found", "text/plain", "not found"),
        };
        let mut s = stream;
        let _ = write!(
            s,
            "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
