//! Allow-list HTTPS proxy for GPU QA (spec-narrate-test-strategy.md, Requirement: Test kit;
//! spec-narrate-setup.md, Source allow-list).
//!
//! `narrate-test-proxy --listen 127.0.0.1:<port> --log <file>`
//!
//! Tunnels `CONNECT host:port` when the host is on the allow-list, else answers 403. Any request
//! that is not `CONNECT` also gets 403. Every request appends `<unix ms>\t<ALLOW or DENY>\t<host>:<port>`
//! to the log file (the target as the client wrote it). An allowed host that cannot be reached
//! gets 502. Connections that send no request line are dropped without a log entry.
//!
//! Allow-list: `pypi.org`, `files.pythonhosted.org`, `download.pytorch.org`, `github.com`,
//! `objects.githubusercontent.com`, `release-assets.githubusercontent.com`, `huggingface.co`,
//! and any host ending in `.hf.co` or `.huggingface.co`.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::process::exit;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const EXACT_HOSTS: [&str; 7] = [
    "pypi.org",
    "files.pythonhosted.org",
    "download.pytorch.org",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "huggingface.co",
];
const SUFFIXES: [&str; 2] = [".hf.co", ".huggingface.co"];

const MAX_HEAD: usize = 16 * 1024;
const FORBIDDEN: &str = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const BAD_GATEWAY: &str = "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const ESTABLISHED: &str = "HTTP/1.1 200 Connection established\r\n\r\n";

struct Log(Mutex<File>);

impl Log {
    fn write(&self, verdict: &str, target: &str) {
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let line = format!("{ms}\t{verdict}\t{target}\n");
        if let Ok(mut f) = self.0.lock() {
            let _ = f.write_all(line.as_bytes());
            let _ = f.flush();
        }
    }
}

fn host_allowed(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    if host.is_empty() || host.contains("..") {
        return false;
    }
    if !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.') {
        return false;
    }
    if EXACT_HOSTS.contains(&host) {
        return true;
    }
    SUFFIXES.iter().any(|s| host.len() > s.len() && host.ends_with(s) && !host.starts_with('.'))
}

/// Splits `host:port` (bracketed IPv6 allowed); `None` when the port is missing or not a number.
fn split_authority(target: &str) -> Option<(String, u16)> {
    let (host, port) = target.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    Some((host.to_string(), port))
}

/// Reads up to the end of the request head. Returns the bytes read (head and anything after it).
fn read_head(client: &mut TcpStream) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() >= MAX_HEAD {
            break;
        }
        match client.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    buf
}

/// The authority of a non-CONNECT request target, for the log (`http://host/path` gives `host:80`).
fn plain_target(target: &str) -> String {
    let (rest, default_port) = if let Some(r) = target.strip_prefix("http://") {
        (r, 80)
    } else if let Some(r) = target.strip_prefix("https://") {
        (r, 443)
    } else {
        return target.to_string();
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.contains(':') { authority.to_string() } else { format!("{authority}:{default_port}") }
}

fn deny(client: &mut TcpStream, log: &Log, target: &str) {
    log.write("DENY", target);
    let _ = client.write_all(FORBIDDEN.as_bytes());
    let _ = client.shutdown(Shutdown::Both);
}

fn handle(mut client: TcpStream, log: Arc<Log>) {
    let _ = client.set_read_timeout(Some(Duration::from_secs(30)));
    let buf = read_head(&mut client);
    if buf.is_empty() {
        return;
    }
    let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4).unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let line = head.lines().next().unwrap_or("").trim().to_string();
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");

    if !method.eq_ignore_ascii_case("CONNECT") {
        let shown = if target.is_empty() { line.as_str().to_string() } else { plain_target(target) };
        deny(&mut client, &log, &shown);
        return;
    }
    let Some((host, port)) = split_authority(target) else {
        deny(&mut client, &log, target);
        return;
    };
    if !host_allowed(&host) {
        deny(&mut client, &log, target);
        return;
    }
    log.write("ALLOW", target);

    let upstream = (host.as_str(), port).to_socket_addrs().ok().and_then(|addrs| {
        addrs.into_iter().find_map(|a| TcpStream::connect_timeout(&a, Duration::from_secs(10)).ok())
    });
    let Some(mut upstream) = upstream else {
        let _ = client.write_all(BAD_GATEWAY.as_bytes());
        let _ = client.shutdown(Shutdown::Both);
        return;
    };
    if client.write_all(ESTABLISHED.as_bytes()).is_err() {
        return;
    }
    // Bytes the client sent after the head belong to the tunnel.
    if buf.len() > head_end && upstream.write_all(&buf[head_end..]).is_err() {
        return;
    }
    let _ = client.set_read_timeout(None);
    tunnel(client, upstream);
}

fn tunnel(client: TcpStream, upstream: TcpStream) {
    let (Ok(mut client_r), Ok(mut upstream_w)) = (client.try_clone(), upstream.try_clone()) else {
        return;
    };
    let (mut upstream_r, mut client_w) = (upstream, client);
    let down = std::thread::spawn(move || {
        let _ = std::io::copy(&mut upstream_r, &mut client_w);
        let _ = client_w.shutdown(Shutdown::Both);
        let _ = upstream_r.shutdown(Shutdown::Both);
    });
    let _ = std::io::copy(&mut client_r, &mut upstream_w);
    let _ = upstream_w.shutdown(Shutdown::Both);
    let _ = client_r.shutdown(Shutdown::Both);
    let _ = down.join();
}

fn usage() -> ! {
    eprintln!("usage: narrate-test-proxy --listen 127.0.0.1:<port> --log <file>");
    exit(2);
}

fn main() {
    let mut listen: Option<String> = None;
    let mut log_path: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--listen" => listen = args.next(),
            "--log" => log_path = args.next(),
            _ => usage(),
        }
    }
    let (Some(listen), Some(log_path)) = (listen, log_path) else { usage() };
    let file = OpenOptions::new().create(true).append(true).open(&log_path).unwrap_or_else(|e| {
        eprintln!("narrate-test-proxy: cannot open {log_path}: {e}");
        exit(1);
    });
    let log = Arc::new(Log(Mutex::new(file)));
    let listener = TcpListener::bind(&listen).unwrap_or_else(|e| {
        eprintln!("narrate-test-proxy: cannot listen on {listen}: {e}");
        exit(1);
    });
    for stream in listener.incoming().flatten() {
        let log = Arc::clone(&log);
        std::thread::spawn(move || handle(stream, log));
    }
}
