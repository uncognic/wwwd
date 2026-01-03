use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;
use scopeguard;

static CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
const MAX_CONNECTIONS: usize = 100;
const MAX_HEADER_SIZE: usize = 8 * 1024;
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
const IO_TIMEOUT_SECS: u64 = 5;

fn respond_simple(mut stream: &TcpStream, status: &str, body: &str) {
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Length: {}\r\n\r\n",
        status,
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

fn safe_percent_decode(input: &str) -> Option<String> {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let a = chars.next()?; let b = chars.next()?;
            let hex = format!("{}{}", a, b);
            let val = u8::from_str_radix(&hex, 16).ok()?;
            out.push(val as char);
        } else {
            out.push(c);
        }
    }
    Some(out)
}

fn client(mut stream: TcpStream) {
    CONNECTIONS.fetch_add(1, Ordering::SeqCst);
    let _guard = scopeguard::guard((), |_| {
        CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
    });

    let _ = stream.set_read_timeout(Some(Duration::from_secs(IO_TIMEOUT_SECS)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(IO_TIMEOUT_SECS)));

    let mut buffer = Vec::with_capacity(1024);
    let mut tmp = [0u8; 512];

    loop {
        match stream.read(&mut tmp) {
            Ok(0) => return,
            Ok(n) => {
                buffer.extend_from_slice(&tmp[..n]);
                if buffer.len() > MAX_HEADER_SIZE {
                    respond_simple(&stream, "413 Payload Too Large", "Header too large");
                    return;
                }
                if buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => {
                return;
            }
        }
    }

    let request = match std::str::from_utf8(&buffer) {
        Ok(s) => s,
        Err(_) => {
            respond_simple(&stream, "400 Bad Request", "Invalid UTF-8 in request");
            return;
        }
    };

    let first_line = request.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let raw_path = parts.next().unwrap_or("");
    let _version = parts.next().unwrap_or("");

    if method != "GET" && method != "HEAD" {
        respond_simple(&stream, "405 Method Not Allowed", "Only GET/HEAD allowed");
        return;
    }
    let decoded = match safe_percent_decode(raw_path) {
        Some(p) => p,
        None => {
            respond_simple(&stream, "400 Bad Request", "Invalid percent-encoding");
            return;
        }
    };

    let path_only = decoded.split(|c| c == '?' || c == '#').next().unwrap_or("/");
    if path_only.contains("..") {
        respond_simple(&stream, "400 Bad Request", "Invalid path");
        return;
    }

    let rel = if path_only == "/" { "index.html" } else { &path_only[1..] };
    let fs_path = Path::new(rel);
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(_) => {
            respond_simple(&stream, "500 Internal Server Error", "Server error");
            return;
        }
    };
    let target = match fs::canonicalize(&fs_path) {
        Ok(p) => p,
        Err(_) => {
            respond_simple(&stream, "404 Not Found", "Not found");
            return;
        }
    };
    if !target.starts_with(&cwd) {
        respond_simple(&stream, "403 Forbidden", "Forbidden");
        return;
    }

    if let Ok(meta) = fs::metadata(&target) {
        if meta.is_dir() {
            respond_simple(&stream, "403 Forbidden", "Not a file");
            return;
        }
        if meta.len() > MAX_FILE_SIZE {
            respond_simple(&stream, "413 Payload Too Large", "File too large");
            return;
        }
    }

    match fs::read(&target) {
        Ok(body) => {
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            if method == "GET" {
                let _ = stream.write_all(&body);
            }
            let _ = stream.flush();
        }
        Err(_) => {
            respond_simple(&stream, "404 Not Found", "Not found");
        }
    }
}

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080")?;
    eprintln!("Server listening on 127.0.0.1:8080");

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                if CONNECTIONS.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    let _ = respond_simple(&s, "503 Service Unavailable", "Server busy");
                    continue;
                }
                thread::spawn(|| {
                    client(s);
                });
            }
            Err(e) => {
                eprintln!("Connection failed: {}", e);
            }
        }
    }
    Ok(())
}