//! Exportação: HAR 1.2, JSON, CSV, Postman v2.1 e cURL.
use crate::store::Entry;
use base64::Engine;
use serde_json::{json, Value};

/// Data ISO 8601 (UTC) a partir de ms Unix, sem dependências.
pub fn iso8601(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Algoritmo civil_from_days (Howard Hinnant).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, (rem % 3600) / 60, rem % 60, ms % 1000)
}

pub fn is_text(mime: &str, body: &[u8]) -> bool {
    let m = mime.to_ascii_lowercase();
    (m.starts_with("text/") || m.contains("json") || m.contains("xml") || m.contains("javascript") || m.contains("x-www-form-urlencoded") || m.is_empty())
        && std::str::from_utf8(body).is_ok()
}

fn hdrs(h: &[(String, String)]) -> Value {
    Value::Array(h.iter().map(|(k, v)| json!({ "name": k, "value": v })).collect())
}

fn query(url: &str) -> Value {
    let q = url.split_once('?').map(|(_, q)| q).unwrap_or("");
    Value::Array(
        q.split('&')
            .filter(|s| !s.is_empty())
            .map(|kv| {
                let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                json!({ "name": k, "value": v })
            })
            .collect(),
    )
}

fn status_text(s: u16) -> &'static str {
    match s {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "",
    }
}

pub fn har(entries: &[Entry], version: &str) -> String {
    let items: Vec<Value> = entries
        .iter()
        .filter(|e| e.method != "CONNECT")
        .map(|e| {
            let req_mime = Entry::header(&e.req_headers, "content-type").unwrap_or("").to_string();
            let mut request = json!({
                "method": e.method, "url": e.url, "httpVersion": e.http_version,
                "headers": hdrs(&e.req_headers), "queryString": query(&e.url), "cookies": [],
                "headersSize": -1, "bodySize": e.req_size,
            });
            if !e.req_body.is_empty() {
                request["postData"] = json!({ "mimeType": req_mime, "text": String::from_utf8_lossy(&e.req_body) });
            }
            let mut content = json!({ "size": e.resp_body.len(), "mimeType": e.mime });
            if is_text(&e.mime, &e.resp_body) {
                content["text"] = json!(String::from_utf8_lossy(&e.resp_body));
            } else if !e.resp_body.is_empty() {
                content["text"] = json!(base64::engine::general_purpose::STANDARD.encode(&e.resp_body));
                content["encoding"] = json!("base64");
            }
            let wait = e.ttfb_ms.max(0.0);
            json!({
                "startedDateTime": iso8601(e.started_ms),
                "time": e.total_ms,
                "request": request,
                "response": {
                    "status": e.status, "statusText": status_text(e.status), "httpVersion": e.http_version,
                    "headers": hdrs(&e.resp_headers), "cookies": [], "content": content,
                    "redirectURL": Entry::header(&e.resp_headers, "location").unwrap_or(""),
                    "headersSize": -1, "bodySize": e.resp_size,
                    "_error": e.error,
                },
                "cache": {},
                "timings": { "send": 0, "wait": wait, "receive": (e.total_ms - wait).max(0.0) },
            })
        })
        .collect();
    serde_json::to_string_pretty(&json!({
        "log": { "version": "1.2", "creator": { "name": "EmuZip", "version": version }, "entries": items }
    }))
    .unwrap_or_default()
}

pub fn json_export(entries: &[Entry]) -> String {
    let v: Vec<Value> = entries
        .iter()
        .map(|e| {
            let mut j = serde_json::to_value(e).unwrap_or(Value::Null);
            j["started"] = json!(iso8601(e.started_ms));
            j["request_body"] = json!(String::from_utf8_lossy(&e.req_body));
            j["response_body"] = if is_text(&e.mime, &e.resp_body) {
                json!(String::from_utf8_lossy(&e.resp_body))
            } else {
                json!(base64::engine::general_purpose::STANDARD.encode(&e.resp_body))
            };
            j
        })
        .collect();
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn csv(entries: &[Entry]) -> String {
    let mut out = String::from("id,start,method,status,url,type,req_bytes,resp_bytes,ttfb_ms,total_ms,tls,error\n");
    for e in entries {
        out += &format!(
            "{},{},{},{},{},{},{},{},{:.1},{:.1},{},{}\n",
            e.id,
            iso8601(e.started_ms),
            e.method,
            e.status,
            csv_field(&e.url),
            csv_field(&e.mime),
            e.req_size,
            e.resp_size,
            e.ttfb_ms,
            e.total_ms,
            e.tls,
            csv_field(e.error.as_deref().unwrap_or(""))
        );
    }
    out
}

pub fn postman(entries: &[Entry]) -> String {
    let items: Vec<Value> = entries
        .iter()
        .filter(|e| e.method != "CONNECT")
        .map(|e| {
            let mut req = json!({
                "method": e.method,
                "header": e.req_headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("content-length")).map(|(k, v)| json!({"key": k, "value": v})).collect::<Vec<_>>(),
                "url": { "raw": e.url },
            });
            if !e.req_body.is_empty() {
                req["body"] = json!({ "mode": "raw", "raw": String::from_utf8_lossy(&e.req_body) });
            }
            json!({ "name": format!("{} {}", e.method, e.path), "request": req })
        })
        .collect();
    serde_json::to_string_pretty(&json!({
        "info": { "name": "EmuZip", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
        "item": items
    }))
    .unwrap_or_default()
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn curl_one(e: &Entry) -> String {
    let mut s = format!("curl -X {} {}", e.method, sh_quote(&e.url));
    for (k, v) in &e.req_headers {
        if k.eq_ignore_ascii_case("content-length") || k.eq_ignore_ascii_case("host") {
            continue;
        }
        s += &format!(" \\\n  -H {}", sh_quote(&format!("{k}: {v}")));
    }
    if !e.req_body.is_empty() {
        s += &format!(" \\\n  --data-binary {}", sh_quote(&String::from_utf8_lossy(&e.req_body)));
    }
    s
}

pub fn curl(entries: &[Entry]) -> String {
    let mut out = String::from("#!/bin/sh\n# Gerado pelo EmuZip\n\n");
    for e in entries.iter().filter(|e| e.method != "CONNECT") {
        out += &curl_one(e);
        out += "\n\n";
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Entry {
        Entry {
            id: 1,
            started_ms: 1_760_000_000_123,
            method: "POST".into(),
            url: "https://api.example.com/v1/login?x=1".into(),
            host: "api.example.com".into(),
            path: "/v1/login?x=1".into(),
            http_version: "HTTP/1.1".into(),
            status: 200,
            req_headers: vec![("content-type".into(), "application/json".into())],
            resp_headers: vec![("content-type".into(), "application/json".into())],
            req_body: br#"{"user":"o'neil"}"#.to_vec(),
            resp_body: br#"{"ok":true}"#.to_vec(),
            mime: "application/json".into(),
            ttfb_ms: 40.0,
            total_ms: 55.0,
            ..Default::default()
        }
    }

    #[test]
    fn iso() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso8601(1_760_000_000_123), "2025-10-09T08:53:20.123Z");
    }

    #[test]
    fn har_valid() {
        let v: Value = serde_json::from_str(&har(&[sample()], "0.3.0")).unwrap();
        let e = &v["log"]["entries"][0];
        assert_eq!(v["log"]["version"], "1.2");
        assert_eq!(e["request"]["method"], "POST");
        assert_eq!(e["request"]["queryString"][0]["name"], "x");
        assert_eq!(e["response"]["content"]["text"], r#"{"ok":true}"#);
        assert_eq!(e["timings"]["wait"], 40.0);
    }

    #[test]
    fn other_formats() {
        assert!(csv(&[sample()]).lines().nth(1).unwrap().starts_with("1,2025-10-09T"));
        let p: Value = serde_json::from_str(&postman(&[sample()])).unwrap();
        assert_eq!(p["item"][0]["request"]["url"]["raw"], "https://api.example.com/v1/login?x=1");
        let c = curl_one(&sample());
        assert!(c.contains(r#"--data-binary '{"user":"o'\''neil"}'"#));
        let j: Value = serde_json::from_str(&json_export(&[sample()])).unwrap();
        assert_eq!(j[0]["response_body"], r#"{"ok":true}"#);
    }
}
