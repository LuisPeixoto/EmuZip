//! Transações capturadas (buffer circular).
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const MAX_ENTRIES: usize = 5000;
pub const MAX_BODY: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Entry {
    pub id: u64,
    /// Início (ms desde a época Unix).
    pub started_ms: u64,
    pub method: String,
    pub url: String,
    pub host: String,
    pub path: String,
    pub http_version: String,
    pub status: u16,
    pub req_headers: Vec<(String, String)>,
    pub resp_headers: Vec<(String, String)>,
    #[serde(skip)]
    pub req_body: Vec<u8>,
    /// Corpo da resposta já descomprimido (gzip/br/deflate).
    #[serde(skip)]
    pub resp_body: Vec<u8>,
    pub req_size: usize,
    pub resp_size: usize,
    pub req_truncated: bool,
    pub resp_truncated: bool,
    pub mime: String,
    pub ttfb_ms: f64,
    pub total_ms: f64,
    pub error: Option<String>,
    pub tls: bool,
    pub decrypted: bool,
    pub done: bool,
}

impl Entry {
    pub fn header<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
        h.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// Categoria simples para filtros e coluna "tipo".
    pub fn kind(&self) -> &'static str {
        let m = self.mime.to_ascii_lowercase();
        if self.method == "CONNECT" {
            "tunnel"
        } else if m.contains("json") {
            "json"
        } else if m.contains("html") {
            "doc"
        } else if m.starts_with("image/") {
            "img"
        } else if m.contains("javascript") {
            "js"
        } else if m.contains("css") {
            "css"
        } else if m.contains("font") {
            "font"
        } else if m.contains("xml") || m.contains("text") {
            "text"
        } else if m.is_empty() {
            ""
        } else {
            "bin"
        }
    }
}

#[derive(Clone, Default)]
pub struct Store {
    inner: Arc<Mutex<VecDeque<Entry>>>,
    seq: Arc<AtomicU64>,
    pub dirty: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
}

impl Store {
    pub fn push(&self, mut e: Entry) -> u64 {
        if self.paused.load(Ordering::Relaxed) {
            return 0;
        }
        e.id = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let id = e.id;
        let mut q = self.inner.lock().unwrap();
        q.push_back(e);
        while q.len() > MAX_ENTRIES {
            q.pop_front();
        }
        self.dirty.store(true, Ordering::Relaxed);
        id
    }

    pub fn update(&self, id: u64, f: impl FnOnce(&mut Entry)) {
        if id == 0 {
            return;
        }
        if let Some(e) = self.inner.lock().unwrap().iter_mut().rev().find(|e| e.id == id) {
            f(e);
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    pub fn get(&self, id: u64) -> Option<Entry> {
        self.inner.lock().unwrap().iter().find(|e| e.id == id).cloned()
    }

    pub fn snapshot(&self) -> Vec<Entry> {
        self.inner.lock().unwrap().iter().cloned().collect()
    }

    pub fn clear(&self) {
        self.inner.lock().unwrap().clear();
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
