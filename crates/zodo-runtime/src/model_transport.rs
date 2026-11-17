//! Cancellable custom-model HTTP transport, with a fresh authorization gate at body polling.
use crate::ai::{MAX_MODEL_BYTES, endpoint_uri, unavailable};
use futures_core::Stream;
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use zodo_kernel::error::Result;

pub(crate) type DispatchGuard = Arc<dyn Fn() -> Result<()> + Send + Sync>;

/// Hyper asks for the body when it is ready to transmit, after DNS/connect/TLS. Check the same
/// current model/policy/run facts again then, not just when constructing an HTTP request.
struct GuardedBody {
    bytes: Option<Vec<u8>>,
    guard: DispatchGuard,
}
impl Stream for GuardedBody {
    type Item = std::io::Result<Vec<u8>>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.bytes.is_none() {
            return Poll::Ready(None);
        }
        if (self.guard)().is_err() {
            self.bytes = None;
            return Poll::Ready(Some(Err(std::io::Error::other(
                "model dispatch authorization revoked",
            ))));
        }
        Poll::Ready(self.bytes.take().map(Ok))
    }
}

pub(crate) fn post_json(
    url: &str,
    api_key: Option<String>,
    body: Value,
    guard: DispatchGuard,
) -> Result<Value> {
    guard()?;
    let uri = endpoint_uri(url)?;
    let bytes =
        serde_json::to_vec(&body).map_err(|_| unavailable("model_request_encode_failed"))?;
    let length = bytes.len();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| unavailable("custom_transport_unavailable"))?;
    let result = runtime.block_on(async {
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(60));
        // No DNS is consulted for localhost; other approved device destinations are literal IPs.
        if uri
            .host()
            .is_some_and(|host| host.eq_ignore_ascii_case("localhost"))
        {
            let port = uri
                .port_u16()
                .unwrap_or(if uri.scheme_str() == Some("https") {
                    443
                } else {
                    80
                });
            builder = builder.resolve(
                "localhost",
                std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            );
        }
        let client = builder
            .build()
            .map_err(|_| unavailable("custom_transport_unavailable"))?;
        let mut request = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Content-Length", length)
            .body(reqwest::Body::wrap_stream(GuardedBody {
                bytes: Some(bytes),
                guard: guard.clone(),
            }));
        if let Some(key) = api_key {
            request = request.bearer_auth(key);
        }
        let send = async {
            guard()?;
            let mut response = request
                .send()
                .await
                .map_err(|_| unavailable("custom_model_request_failed"))?;
            if response.status() != reqwest::StatusCode::OK {
                return Err(unavailable("custom_model_request_failed"));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| unavailable("model_response_unreadable"))?
            {
                if bytes.len() + chunk.len() > MAX_MODEL_BYTES {
                    return Err(unavailable("model_response_too_large"));
                }
                bytes.extend_from_slice(&chunk);
            }
            guard()?;
            serde_json::from_slice(&bytes).map_err(|_| unavailable("model_response_invalid_json"))
        };
        tokio::pin!(send);
        let mut checks = tokio::time::interval(Duration::from_millis(50));
        let deadline = tokio::time::sleep(Duration::from_secs(60));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                biased;
                _ = &mut deadline => { return Err(unavailable("custom_model_timeout")); }
                _ = checks.tick() => { guard()?; checks.reset(); }
                result = &mut send => { return result; }
            }
        }
        // Dropping the request/response futures and private client aborts transport I/O.
    });
    // System DNS may use a blocking OS resolver. Bound shutdown rather than waiting indefinitely;
    // a lingering DNS lookup has only a hostname, never the prompt/body or provider credential.
    runtime.shutdown_timeout(Duration::from_millis(200));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::Instant,
    };

    fn receive(stream: &mut std::net::TcpStream) -> Vec<u8> {
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut data = Vec::new();
        let mut byte = [0];
        while !data.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            data.push(byte[0]);
        }
        let headers = String::from_utf8_lossy(&data);
        let length: usize = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|value| value.trim().parse().unwrap())
            })
            .unwrap();
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        body
    }
    fn reply(stream: &mut std::net::TcpStream, status: &str, body: &str, extra: &str) {
        let text = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
            body.len()
        );
        stream.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn slow_authorization_checks_do_not_starve_http_dispatch() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!(
            "http://localhost:{}/v1/chat/completions",
            listener.local_addr().unwrap().port()
        );
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            assert_eq!(receive(&mut stream), br#"{"fixture":"allowed"}"#);
            reply(&mut stream, "200 OK", "{}", "");
        });
        let guard = Arc::new(|| {
            std::thread::sleep(Duration::from_millis(60));
            Ok(())
        });
        let start = Instant::now();
        assert_eq!(
            post_json(&endpoint, None, json!({"fixture":"allowed"}), guard).unwrap(),
            json!({})
        );
        assert!(start.elapsed() < Duration::from_secs(4));
        server.join().unwrap();
    }

    #[test]
    fn revocation_closes_actual_inflight_socket_promptly() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let (sent, ready) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            receive(&mut stream);
            sent.send(()).unwrap();
            let mut byte = [0];
            assert!(matches!(stream.read(&mut byte), Ok(0) | Err(_)));
        });
        let allowed = Arc::new(AtomicBool::new(true));
        let flag = allowed.clone();
        let guard = Arc::new(move || {
            if flag.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(crate::ai::denied("revoked"))
            }
        });
        let caller = std::thread::spawn(move || {
            post_json(&endpoint, None, json!({"fixture":"allowed"}), guard)
        });
        ready.recv_timeout(Duration::from_secs(4)).unwrap();
        let start = Instant::now();
        allowed.store(false, Ordering::SeqCst);
        assert!(caller.join().unwrap().is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        server.join().unwrap();
    }

    #[test]
    fn guarded_body_refuses_revocation_after_preflight_before_bytes_are_released() {
        let allowed = Arc::new(AtomicBool::new(true));
        let flag = allowed.clone();
        let guard: DispatchGuard = Arc::new(move || {
            if flag.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(crate::ai::denied("revoked"))
            }
        });
        guard().unwrap();
        let mut body = GuardedBody {
            bytes: Some(b"must not dispatch".to_vec()),
            guard,
        };
        allowed.store(false, Ordering::SeqCst);
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert!(matches!(
            Pin::new(&mut body).poll_next(&mut cx),
            Poll::Ready(Some(Err(_)))
        ));
        assert!(body.bytes.is_none());
    }

    #[test]
    fn redirects_are_not_followed_and_provider_errors_are_redacted() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let escaped = TcpListener::bind("127.0.0.1:0").unwrap();
        escaped.set_nonblocking(true).unwrap();
        let endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let target = escaped.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            receive(&mut stream);
            reply(
                &mut stream,
                "307 Temporary Redirect",
                "SYNTHETIC_SECRET_MUST_NOT_LEAK",
                &format!("Location: http://{target}/escape\r\n"),
            );
        });
        let error = post_json(
            &endpoint,
            None,
            json!({"fixture":"allowed"}),
            Arc::new(|| Ok(())),
        )
        .unwrap_err();
        assert!(!error.message.contains("SYNTHETIC_SECRET"));
        assert!(escaped.accept().is_err());
        server.join().unwrap();
    }
}
