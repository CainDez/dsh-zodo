//! A0 bridge probe.
//!
//! This is deliberately **not** the Zodo business kernel or a second implementation of its domain.
//! It proves the real Harness -> Host adapter -> versioned Rust process boundary, including
//! discovery, handshake, asynchronous work, cancellation and clean teardown. The business Core
//! replaces it in A1/A2 and keeps its own protocol and SQLite facts.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

const PROTOCOL: &str = "zodo-a0-bridge/1";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_SLEEP_MS: u64 = 60_000;
const AUTH_HEADER: &str = "authorization";

fn main() {
    if let Err(error) = run() {
        eprintln!("zodo-a0-probe: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let state_dir = parse_args()?;
    fs::create_dir_all(&state_dir)
        .map_err(|error| format!("create state directory {}: {error}", state_dir.display()))?;
    let lock_path = state_dir.join("probe.lock.json");

    if let Some(existing) = read_lock(&lock_path) {
        if health_of(&existing).is_ok() {
            println!(
                "zodo-a0-probe: existing instance {} is healthy; this instance exits",
                existing.instance_id
            );
            return Ok(());
        }
        // A dead lock must not prevent recovery forever. The Host also checks health before
        // spawning, so this is the repair path for an owner that disappeared without cleanup.
        thread::sleep(Duration::from_millis(150));
        if let Some(existing) = read_lock(&lock_path) {
            if health_of(&existing).is_ok() {
                println!(
                    "zodo-a0-probe: instance {} won the startup race; this instance exits",
                    existing.instance_id
                );
                return Ok(());
            }
        }
        match fs::remove_file(&lock_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "remove stale lock {}: {error}",
                    lock_path.display()
                ));
            }
        }
    }

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("bind loopback listener: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("set nonblocking listener: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("read listener address: {error}"))?
        .port();

    let state = Arc::new(ProbeState {
        protocol: PROTOCOL,
        version: VERSION,
        instance_id: Uuid::new_v4().to_string(),
        token: Uuid::new_v4().simple().to_string(),
        pid: process::id(),
        started: Instant::now(),
        shutdown: AtomicBool::new(false),
        operations: Mutex::new(HashMap::new()),
        calls: AtomicU64::new(0),
        cancellations: AtomicU64::new(0),
    });

    let lock = LockRecord {
        protocol: PROTOCOL.to_string(),
        version: VERSION.to_string(),
        instance_id: state.instance_id.clone(),
        pid: state.pid,
        port,
        token: state.token.clone(),
        started_at_unix_ms: unix_ms(),
    };
    write_lock_atomic(&lock_path, &lock)?;
    println!(
        "zodo-a0-probe: listening on 127.0.0.1:{port} instance={} pid={}",
        state.instance_id, state.pid
    );

    while !state.shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _peer)) => {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, &state) {
                        eprintln!("zodo-a0-probe: request error: {error}");
                    }
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(15));
            }
            Err(error) => return Err(format!("accept connection: {error}")),
        }
    }

    let _ = fs::remove_file(&lock_path);
    println!("zodo-a0-probe: stopped cleanly; lock removed");
    Ok(())
}

fn parse_args() -> Result<PathBuf, String> {
    let mut args = std::env::args().skip(1);
    let mut state_dir: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--state-dir" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--state-dir requires a path".to_string())?;
                state_dir = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                println!(
                    "zodo-a0-probe --state-dir <directory>\n\nA0 bridge probe only. Protocol {PROTOCOL}."
                );
                process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}; use --help")),
        }
    }
    state_dir.ok_or_else(|| "missing required --state-dir".to_string())
}

struct ProbeState {
    protocol: &'static str,
    version: &'static str,
    instance_id: String,
    token: String,
    pid: u32,
    started: Instant,
    shutdown: AtomicBool,
    operations: Mutex<HashMap<String, Arc<AtomicBool>>>,
    calls: AtomicU64,
    cancellations: AtomicU64,
}

#[derive(Clone, Deserialize, Serialize)]
struct LockRecord {
    #[serde(rename = "protocol")]
    protocol: String,
    #[serde(rename = "version")]
    version: String,
    #[serde(rename = "instance_id")]
    instance_id: String,
    #[serde(rename = "pid")]
    pid: u32,
    #[serde(rename = "port")]
    port: u16,
    #[serde(rename = "token")]
    token: String,
    #[serde(rename = "started_at_unix_ms")]
    started_at_unix_ms: u64,
}

fn read_lock(path: &Path) -> Option<LockRecord> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_lock_atomic(path: &Path, lock: &LockRecord) -> Result<(), String> {
    let temporary = path.with_extension("tmp");
    let body = serde_json::to_vec_pretty(lock).map_err(|error| format!("encode lock: {error}"))?;
    fs::write(&temporary, body).map_err(|error| format!("write lock temp: {error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("publish lock: {error}"))
}

fn health_of(lock: &LockRecord) -> Result<Value, String> {
    let body = http_request("GET", "127.0.0.1", lock.port, "/v1/health", None, None)?;
    if body.status != 200 {
        return Err(format!("health status {}", body.status));
    }
    let value: Value =
        serde_json::from_slice(&body.body).map_err(|error| format!("decode health: {error}"))?;
    if value.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
        return Err("protocol mismatch".to_string());
    }
    if value.get("instance").and_then(Value::as_str) != Some(lock.instance_id.as_str()) {
        return Err("instance mismatch".to_string());
    }
    Ok(value)
}

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

fn http_request(
    method: &str,
    host: &str,
    port: u16,
    path: &str,
    token: Option<&str>,
    body: Option<&[u8]>,
) -> Result<HttpResponse, String> {
    let mut stream = TcpStream::connect((host, port))
        .map_err(|error| format!("connect {host}:{port}: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| format!("set timeout: {error}"))?;
    let body = body.unwrap_or_default();
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("Content-Type: application/json\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.write_all(body))
        .map_err(|error| format!("write request: {error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("read response: {error}"))?;
    parse_http_response(&response)
}

fn parse_http_response(raw: &[u8]) -> Result<HttpResponse, String> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "malformed HTTP response: no header terminator".to_string())?;
    let header = std::str::from_utf8(&raw[..split])
        .map_err(|error| format!("decode response header: {error}"))?;
    let status = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "malformed HTTP response status".to_string())?;
    Ok(HttpResponse {
        status,
        body: raw[split + 4..].to_vec(),
    })
}

struct ParsedRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Result<ParsedRequest, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| format!("set read timeout: {error}"))?;
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > 64 * 1024 {
            return Err("request header too large".to_string());
        }
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("read request header: {error}"))?;
        if read == 0 {
            return Err("connection closed before headers".to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let header_text = std::str::from_utf8(&buffer[..header_end])
        .map_err(|error| format!("decode request header: {error}"))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| "missing request line".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut headers = HashMap::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > MAX_BODY_BYTES {
        return Err("request body too large".to_string());
    }
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < content_length {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("read request body: {error}"))?;
        if read == 0 {
            return Err("connection closed before body".to_string());
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);
    Ok(ParsedRequest {
        method,
        path,
        headers,
        body,
    })
}

fn handle_connection(mut stream: TcpStream, state: &ProbeState) -> Result<(), String> {
    let request = read_request(&mut stream)?;
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/health") => write_json(
            &mut stream,
            200,
            &json!({
                "schema": "zodo.a0.health/1",
                "protocol": state.protocol,
                "version": state.version,
                "instance": state.instance_id,
                "pid": state.pid,
                "uptime_ms": state.started.elapsed().as_millis() as u64,
                "calls": state.calls.load(Ordering::SeqCst),
            }),
        ),
        ("POST", "/v1/rpc") => {
            let expected = format!("Bearer {}", state.token);
            let supplied = request
                .headers
                .get(AUTH_HEADER)
                .map(String::as_str)
                .unwrap_or_default();
            if supplied != expected {
                return write_json(
                    &mut stream,
                    401,
                    &json!({"ok": false, "error": {"code": "UNAUTHORIZED", "message": "invalid probe token"}}),
                );
            }
            let envelope: RpcRequest = match serde_json::from_slice(&request.body) {
                Ok(value) => value,
                Err(error) => {
                    return write_json(
                        &mut stream,
                        400,
                        &json!({"ok": false, "error": {"code": "BAD_REQUEST", "message": format!("invalid JSON: {error}")}}),
                    );
                }
            };
            state.calls.fetch_add(1, Ordering::SeqCst);
            let response = dispatch(state, envelope);
            write_json(
                &mut stream,
                if response.ok { 200 } else { 422 },
                &response.value,
            )
        }
        ("POST", "/v1/shutdown") => {
            let expected = format!("Bearer {}", state.token);
            if request.headers.get(AUTH_HEADER).map(String::as_str) != Some(expected.as_str()) {
                return write_json(
                    &mut stream,
                    401,
                    &json!({"ok": false, "error": {"code": "UNAUTHORIZED", "message": "invalid probe token"}}),
                );
            }
            state.shutdown.store(true, Ordering::SeqCst);
            write_json(
                &mut stream,
                200,
                &json!({"ok": true, "stopping": true, "instance": state.instance_id}),
            )
        }
        _ => write_json(
            &mut stream,
            404,
            &json!({"ok": false, "error": {"code": "NOT_FOUND", "message": "unknown endpoint"}}),
        ),
    }
}

#[derive(Deserialize)]
struct RpcRequest {
    id: String,
    method: String,
    #[serde(default)]
    params: Value,
}

struct RpcResponse {
    ok: bool,
    value: Value,
}

fn dispatch(state: &ProbeState, request: RpcRequest) -> RpcResponse {
    if request.id.trim().is_empty() {
        return error_response(&request.id, "BAD_REQUEST", "request id is required");
    }
    match request.method.as_str() {
        "ping" => RpcResponse {
            ok: true,
            value: json!({
                "schema": "zodo.a0.result/1",
                "id": request.id,
                "ok": true,
                "protocol": state.protocol,
                "instance": state.instance_id,
                "result": {
                    "message": "pong",
                    "version": state.version,
                    "pid": state.pid,
                    "received_at_unix_ms": unix_ms(),
                }
            }),
        },
        "echo" => {
            let text = request
                .params
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            RpcResponse {
                ok: true,
                value: json!({
                    "schema": "zodo.a0.result/1",
                    "id": request.id,
                    "ok": true,
                    "protocol": state.protocol,
                    "instance": state.instance_id,
                    "result": {"message": text}
                }),
            }
        }
        "sleep" => {
            let delay_ms = request
                .params
                .get("delay_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(MAX_SLEEP_MS);
            let cancel = Arc::new(AtomicBool::new(false));
            state
                .operations
                .lock()
                .expect("operation map poisoned")
                .insert(request.id.clone(), Arc::clone(&cancel));
            let mut elapsed = 0_u64;
            while elapsed < delay_ms {
                if cancel.load(Ordering::SeqCst) {
                    state.cancellations.fetch_add(1, Ordering::SeqCst);
                    state
                        .operations
                        .lock()
                        .expect("operation map poisoned")
                        .remove(&request.id);
                    return error_response(&request.id, "CANCELLED", "operation cancelled");
                }
                let step = (delay_ms - elapsed).min(25);
                thread::sleep(Duration::from_millis(step));
                elapsed += step;
            }
            state
                .operations
                .lock()
                .expect("operation map poisoned")
                .remove(&request.id);
            RpcResponse {
                ok: true,
                value: json!({
                    "schema": "zodo.a0.result/1",
                    "id": request.id,
                    "ok": true,
                    "protocol": state.protocol,
                    "instance": state.instance_id,
                    "result": {"slept_ms": delay_ms}
                }),
            }
        }
        "cancel" => {
            let target = request
                .params
                .get("target_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let found = state
                .operations
                .lock()
                .expect("operation map poisoned")
                .get(target)
                .map(|flag| {
                    flag.store(true, Ordering::SeqCst);
                })
                .is_some();
            RpcResponse {
                ok: true,
                value: json!({
                    "schema": "zodo.a0.result/1",
                    "id": request.id,
                    "ok": true,
                    "protocol": state.protocol,
                    "instance": state.instance_id,
                    "result": {"cancelled": found, "target_id": target}
                }),
            }
        }
        "status" => RpcResponse {
            ok: true,
            value: json!({
                "schema": "zodo.a0.result/1",
                "id": request.id,
                "ok": true,
                "protocol": state.protocol,
                "instance": state.instance_id,
                "result": {
                    "calls": state.calls.load(Ordering::SeqCst),
                    "cancellations": state.cancellations.load(Ordering::SeqCst),
                    "active_operations": state.operations.lock().expect("operation map poisoned").len(),
                    "uptime_ms": state.started.elapsed().as_millis() as u64,
                }
            }),
        },
        other => error_response(
            &request.id,
            "UNKNOWN_METHOD",
            &format!("unknown method {other:?}"),
        ),
    }
}

fn error_response(id: &str, code: &str, message: &str) -> RpcResponse {
    RpcResponse {
        ok: false,
        value: json!({
            "schema": "zodo.a0.error/1",
            "id": id,
            "ok": false,
            "error": {"code": code, "message": message}
        }),
    }
}

fn write_json(stream: &mut TcpStream, status: u16, value: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(value).map_err(|error| format!("encode response: {error}"))?;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        422 => "Unprocessable Content",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|()| stream.write_all(&body))
        .map_err(|error| format!("write response: {error}"))
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ProbeState {
        ProbeState {
            protocol: PROTOCOL,
            version: VERSION,
            instance_id: "test-instance".to_string(),
            token: "test-token".to_string(),
            pid: process::id(),
            started: Instant::now(),
            shutdown: AtomicBool::new(false),
            operations: Mutex::new(HashMap::new()),
            calls: AtomicU64::new(0),
            cancellations: AtomicU64::new(0),
        }
    }

    fn request(id: &str, method: &str, params: Value) -> RpcRequest {
        RpcRequest {
            id: id.to_string(),
            method: method.to_string(),
            params,
        }
    }

    #[test]
    fn ping_reports_protocol_and_instance() {
        let response = dispatch(&state(), request("r1", "ping", json!({})));
        assert!(response.ok);
        assert_eq!(response.value["protocol"], PROTOCOL);
        assert_eq!(response.value["instance"], "test-instance");
        assert_eq!(response.value["result"]["message"], "pong");
    }

    #[test]
    fn echo_preserves_text() {
        let response = dispatch(
            &state(),
            request("r2", "echo", json!({"text": "你好 Zodo"})),
        );
        assert!(response.ok);
        assert_eq!(response.value["result"]["message"], "你好 Zodo");
    }

    #[test]
    fn cancel_marks_an_operation() {
        let state = state();
        state
            .operations
            .lock()
            .unwrap()
            .insert("slow".to_string(), Arc::new(AtomicBool::new(false)));
        let response = dispatch(
            &state,
            request("r3", "cancel", json!({"target_id": "slow"})),
        );
        assert!(response.ok);
        assert_eq!(response.value["result"]["cancelled"], true);
        assert!(state.operations.lock().unwrap()["slow"].load(Ordering::SeqCst));
    }

    #[test]
    fn unknown_method_is_a_domain_error_not_a_panic() {
        let response = dispatch(&state(), request("r4", "nope", json!({})));
        assert!(!response.ok);
        assert_eq!(response.value["error"]["code"], "UNKNOWN_METHOD");
    }

    #[test]
    fn blank_request_id_is_rejected() {
        let response = dispatch(&state(), request(" ", "ping", json!({})));
        assert!(!response.ok);
        assert_eq!(response.value["error"]["code"], "BAD_REQUEST");
    }
}
