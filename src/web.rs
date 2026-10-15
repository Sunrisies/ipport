use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, ensure};
use serde::Serialize;

use crate::scanner::{self, Filter};

const WORKERS: usize = 4;
const QUEUE_SIZE: usize = 16;
const MAX_REQUEST_SIZE: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

type ScanGate = Arc<Mutex<()>>;

pub fn serve(address: SocketAddr) -> Result<()> {
    ensure!(
        address.ip().is_loopback(),
        "The local dashboard must bind to a loopback address (127.0.0.1 or ::1)"
    );
    let listener = TcpListener::bind(address).context("Could not bind the local web server")?;
    listener.set_nonblocking(true)?;
    let gate = Arc::new(Mutex::new(()));
    let (sender, receiver) = mpsc::sync_channel::<TcpStream>(QUEUE_SIZE);
    let receiver = Arc::new(Mutex::new(receiver));
    let mut workers = Vec::with_capacity(WORKERS);

    for _ in 0..WORKERS {
        let receiver = Arc::clone(&receiver);
        let gate = Arc::clone(&gate);
        workers.push(thread::spawn(move || {
            loop {
                let stream = {
                    let channel = match receiver.lock() {
                        Ok(channel) => channel,
                        Err(_) => break,
                    };
                    match channel.recv() {
                        Ok(stream) => stream,
                        Err(_) => break,
                    }
                };
                let _ = handle_connection(stream, &gate);
            }
        }));
    }
    drop(receiver);

    let shutdown = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&shutdown);
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))
        .context("Could not install the Ctrl+C handler")?;

    println!(
        "Open http://{} in your browser. Press Ctrl+C to stop.",
        listener.local_addr()?
    );
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if sender.send(stream).is_err() {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error).context("Could not accept a browser connection"),
        }
    }

    drop(sender);
    for worker in workers {
        worker
            .join()
            .map_err(|_| anyhow!("Web request worker stopped unexpectedly"))?;
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream, gate: &ScanGate) -> Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let mut bytes = Vec::with_capacity(2048);
    let mut chunk = [0; 2048];
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = match stream.read(&mut chunk) {
            Ok(0) => return send_error(&mut stream, 400, "Bad Request", "Incomplete HTTP request"),
            Ok(count) => count,
            Err(error) => return Err(error).context("Could not read HTTP request"),
        };
        if bytes.len() + count > MAX_REQUEST_SIZE {
            return send_error(
                &mut stream,
                431,
                "Request Header Fields Too Large",
                "Request headers exceed 16 KiB",
            );
        }
        bytes.extend_from_slice(&chunk[..count]);
    }

    let request = std::str::from_utf8(&bytes).context("HTTP request is not valid UTF-8")?;
    let mut lines = request.split("\r\n");
    let request_line = lines.next().context("HTTP request has no request line")?;
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if parts.next().is_some() || !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return send_error(
            &mut stream,
            400,
            "Bad Request",
            "Malformed HTTP request line",
        );
    }
    if method != "GET" {
        return send_error(
            &mut stream,
            405,
            "Method Not Allowed",
            "Only GET requests are supported",
        );
    }
    for line in lines.take_while(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return send_error(&mut stream, 400, "Bad Request", "Malformed HTTP header");
        };
        if name.eq_ignore_ascii_case("transfer-encoding")
            || name.eq_ignore_ascii_case("content-length") && value.trim() != "0"
        {
            return send_error(
                &mut stream,
                400,
                "Bad Request",
                "GET request bodies are not supported",
            );
        }
    }

    if !target.starts_with('/') || target.contains('#') {
        return send_error(&mut stream, 400, "Bad Request", "Invalid request path");
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match path {
        "/" => send_bytes(
            &mut stream,
            200,
            "OK",
            "text/html; charset=utf-8",
            include_bytes!("../assets/index.html"),
        ),
        "/api/ports" => {
            let filter: Filter = match serde_urlencoded::from_str(query) {
                Ok(filter) => filter,
                Err(error) => {
                    return send_error(
                        &mut stream,
                        400,
                        "Bad Request",
                        &format!("Invalid query: {error}"),
                    );
                }
            };
            let _permit = match gate.try_lock() {
                Ok(permit) => permit,
                Err(TryLockError::WouldBlock) => {
                    return send_json_error(
                        &mut stream,
                        503,
                        "Service Unavailable",
                        "A scan is already running. Please try again.",
                    );
                }
                Err(TryLockError::Poisoned(_)) => {
                    return send_json_error(
                        &mut stream,
                        500,
                        "Internal Server Error",
                        "The port scanner is unavailable.",
                    );
                }
            };
            match scanner::scan(&filter) {
                Ok(snapshot) => send_json(&mut stream, 200, "OK", &snapshot),
                Err(error) => send_json_error(
                    &mut stream,
                    500,
                    "Internal Server Error",
                    &format!("{error:#}"),
                ),
            }
        }
        _ => send_error(&mut stream, 404, "Not Found", "Not found"),
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
}

fn send_json_error(stream: &mut TcpStream, code: u16, reason: &str, message: &str) -> Result<()> {
    send_json(stream, code, reason, &ErrorBody { error: message })
}

fn send_json<T: Serialize>(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    data: &T,
) -> Result<()> {
    let body = serde_json::to_vec(data).context("Could not encode JSON response")?;
    send_bytes(
        stream,
        code,
        reason,
        "application/json; charset=utf-8",
        &body,
    )
}

fn send_error(stream: &mut TcpStream, code: u16, reason: &str, message: &str) -> Result<()> {
    send_json_error(stream, code, reason, message)
}

fn send_bytes(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n",
        body.len()
    )?;
    if content_type.starts_with("text/html") {
        stream.write_all(b"Content-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    stream.write_all(body)?;
    stream.flush().context("Could not send HTTP response")
}
