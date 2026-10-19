//! Proxy MITM baseado no hudsucker. O emulador aponta para 10.0.2.2:<porta> (o host).
use crate::store::{now_ms, Entry, Store, MAX_BODY};
use crate::Ca;
use anyhow::Result;
use http_body_util::BodyExt;
use hudsucker::certificate_authority::RcgenAuthority;
use hudsucker::hyper::{header, Request, Response, StatusCode};
use hudsucker::rustls::crypto::aws_lc_rs;
use hudsucker::{Body, HttpContext, HttpHandler, Proxy, RequestOrResponse};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
struct Handler {
    store: Store,
    decrypt: Arc<AtomicBool>,
    cur: Option<(u64, Instant)>,
}

fn headers_vec(h: &hudsucker::hyper::HeaderMap) -> Vec<(String, String)> {
    h.iter().map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned())).collect()
}

fn too_big(h: &hudsucker::hyper::HeaderMap) -> bool {
    let len = h.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    let ct = h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    len > MAX_BODY || ct.contains("event-stream") || ct.contains("video/") || ct.contains("audio/")
}

impl HttpHandler for Handler {
    async fn handle_request(&mut self, _ctx: &HttpContext, req: Request<Body>) -> RequestOrResponse {
        let uri = req.uri().clone();
        if req.method() == hudsucker::hyper::Method::CONNECT {
            let auth = uri.authority().map(|a| a.to_string()).unwrap_or_default();
            self.store.push(Entry {
                started_ms: now_ms(),
                method: "CONNECT".into(),
                url: format!("https://{}", auth.trim_end_matches(":443")),
                host: auth.split(':').next().unwrap_or("").into(),
                path: if self.decrypt.load(Ordering::Relaxed) { "(TLS tunnel, decrypted)".into() } else { "(TLS tunnel, not decrypted)".into() },
                status: 200,
                tls: true,
                decrypted: self.decrypt.load(Ordering::Relaxed),
                done: true,
                ..Default::default()
            });
            return req.into();
        }
        let started = Instant::now();
        let (parts, body) = req.into_parts();
        let host = uri.host().map(String::from).or_else(|| parts.headers.get(header::HOST).and_then(|v| v.to_str().ok()).map(String::from)).unwrap_or_default();
        let tls = uri.scheme_str() == Some("https");
        let mut e = Entry {
            started_ms: now_ms(),
            method: parts.method.to_string(),
            url: if uri.scheme().is_some() { uri.to_string().replacen(":443/", "/", 1) } else { format!("http://{host}{uri}") },
            path: uri.path_and_query().map(|p| p.to_string()).unwrap_or_else(|| "/".into()),
            host,
            http_version: format!("{:?}", parts.version),
            req_headers: headers_vec(&parts.headers),
            tls,
            decrypted: tls,
            ..Default::default()
        };
        let body = if too_big(&parts.headers) {
            e.req_truncated = true;
            body
        } else {
            match body.collect().await {
                Ok(c) => {
                    let b = c.to_bytes();
                    e.req_size = b.len();
                    e.req_body = b.to_vec();
                    Body::from(b)
                }
                Err(_) => Body::empty(),
            }
        };
        let id = self.store.push(e);
        self.cur = Some((id, started));
        Request::from_parts(parts, body).into()
    }

    async fn handle_response(&mut self, _ctx: &HttpContext, res: Response<Body>) -> Response<Body> {
        let Some((id, started)) = self.cur else { return res };
        let ttfb = started.elapsed().as_secs_f64() * 1000.0;
        let (parts, body) = res.into_parts();
        let status = parts.status.as_u16();
        let hv = headers_vec(&parts.headers);
        let mime = parts.headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").trim().to_string();
        if too_big(&parts.headers) {
            self.store.update(id, |e| {
                e.status = status;
                e.resp_headers = hv;
                e.mime = mime;
                e.ttfb_ms = ttfb;
                e.total_ms = ttfb;
                e.resp_truncated = true;
                e.done = true;
            });
            return Response::from_parts(parts, body);
        }
        let raw = match body.collect().await {
            Ok(c) => c.to_bytes(),
            Err(err) => {
                let msg = err.to_string();
                self.store.update(id, |e| {
                    e.status = status;
                    e.error = Some(msg);
                    e.done = true;
                });
                return Response::from_parts(parts, Body::empty());
            }
        };
        // Cópia descomprimida só para exibição; o cliente recebe os bytes originais.
        let mut b = Response::builder().status(parts.status);
        for (k, v) in parts.headers.iter() {
            b = b.header(k, v);
        }
        let decoded = match b.body(Body::from(raw.clone())).map(hudsucker::decode_response) {
            Ok(Ok(r)) => r.into_body().collect().await.map(|c| c.to_bytes().to_vec()).unwrap_or_else(|_| raw.to_vec()),
            _ => raw.to_vec(),
        };
        let total = started.elapsed().as_secs_f64() * 1000.0;
        let size = raw.len();
        self.store.update(id, |e| {
            e.status = status;
            e.resp_headers = hv;
            e.mime = mime;
            e.ttfb_ms = ttfb;
            e.total_ms = total;
            e.resp_size = size;
            e.resp_truncated = decoded.len() > MAX_BODY;
            e.resp_body = if decoded.len() > MAX_BODY { decoded[..MAX_BODY].to_vec() } else { decoded };
            e.done = true;
        });
        Response::from_parts(parts, Body::from(raw))
    }

    async fn handle_error(&mut self, _ctx: &HttpContext, err: hudsucker::hyper_util::client::legacy::Error) -> Response<Body> {
        if let Some((id, started)) = self.cur {
            let msg = format!("{err}");
            self.store.update(id, |e| {
                e.error = Some(msg);
                e.total_ms = started.elapsed().as_secs_f64() * 1000.0;
                e.done = true;
            });
        }
        Response::builder().status(StatusCode::BAD_GATEWAY).body(Body::empty()).expect("502")
    }

    async fn should_intercept_tls(&mut self, _ctx: &HttpContext, _hello: hudsucker::rustls::server::ClientHello<'_>) -> bool {
        self.decrypt.load(Ordering::Relaxed)
    }
}

/// Captura em andamento. Ao ser descartada, encerra o proxy.
pub struct Capture {
    pub port: u16,
    pub decrypt: Arc<AtomicBool>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Capture {
    pub fn stop(mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
    }
}

/// Porta livre a partir de `from`.
pub fn free_port(from: u16) -> u16 {
    (from..from + 200).find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok()).unwrap_or(from)
}

/// Inicia o proxy (precisa rodar dentro de um runtime tokio).
pub async fn start(ca: &Ca, store: Store, decrypt: bool, port: u16) -> Result<Capture> {
    let authority = RcgenAuthority::new(ca.issuer()?, 1000, aws_lc_rs::default_provider());
    let decrypt = Arc::new(AtomicBool::new(decrypt));
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let proxy = Proxy::builder()
        .with_addr(SocketAddr::from(([127, 0, 0, 1], port)))
        .with_ca(authority)
        .with_rustls_connector(aws_lc_rs::default_provider())
        .with_http_handler(Handler { store, decrypt: decrypt.clone(), cur: None })
        .with_graceful_shutdown(async {
            let _ = rx.await;
        })
        .build()?;
    tokio::spawn(async move {
        let _ = proxy.start().await;
    });
    // Aguarda o bind.
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    }
    Ok(Capture { port, decrypt, stop: Some(tx) })
}
