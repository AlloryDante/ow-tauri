//! A tiny loopback HTTP server (127.0.0.1) that serves the guest fixture page
//! so the ad guest has a real `http://` origin, as a production ad guest does,
//! without ever touching an ad server. It answers exactly two paths and
//! nothing else. No external network.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const GUEST_HTML: &str = include_str!("../web/guest-fixture.html");

/// Starts the server on an ephemeral loopback port. Returns the port. The
/// server thread runs until `stop` is set.
pub fn start(stop: Arc<AtomicBool>) -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    std::thread::spawn(move || {
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => handle(stream),
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });
    Ok(port)
}

fn handle(mut stream: TcpStream) {
    let mut buf = [0u8; 2048];
    let Ok(n) = stream.read(&mut buf) else { return };
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("/");
    let body = if path.starts_with("/guest") || path == "/" {
        GUEST_HTML
    } else {
        "not found"
    };
    let status = if body == "not found" { "404 Not Found" } else { "200 OK" };
    // The guest is an http origin; no COOP/COEP so the local-origin-frame
    // attack (B5) is not blocked by headers — only by the Rust frame guard.
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}
