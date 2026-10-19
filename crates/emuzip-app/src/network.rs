//! Inspetor de rede: captura via proxy MITM, lista, detalhe e exportação.
use crate::ctx::{sync_model, Ctx};
use crate::{Api, MainWindow, Net, NetEntry, Player};
use emuzip_backend::avd::AvdBackend;
use emuzip_core::human_bytes;
use emuzip_netcap::{export, Ca, Capture, Entry, Store};
use slint::ComponentHandle;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[derive(Default)]
pub struct NetState {
    pub store: Store,
    pub capture: Option<Capture>,
    pub serial: Option<String>,
    pub selected: Option<u64>,
    pub filtered: Vec<u64>,
}

/// Marcador do proxy configurado pelo EmuZip em cada device (para limpar só o que é nosso).
fn marker(serial: &str) -> std::path::PathBuf {
    emuzip_core::data_dir().join("netcap").join(format!("active-{serial}"))
}

/// Se o device ficou apontando para um proxy de captura do EmuZip que não existe mais, restaura.
pub fn cleanup_orphan(serial: &str) {
    let Ok(port) = std::fs::read_to_string(marker(serial)) else { return };
    let cur = emuzip_adb::shell(serial, "settings get global http_proxy").unwrap_or_default();
    if cur.trim() == format!("10.0.2.2:{}", port.trim()) {
        let _ = emuzip_adb::shell(serial, "settings put global http_proxy :0");
    }
    let _ = std::fs::remove_file(marker(serial));
}

fn ca() -> anyhow::Result<Ca> {
    Ca::load_or_create(&emuzip_core::data_dir().join("netcap"))
}

fn serial(ctx: &Ctx) -> Option<String> {
    let id = ctx.player.lock().unwrap().as_ref()?.id.clone();
    AvdBackend::find_running(&id).map(|r| r.serial())
}

/// Tenta instalar o CA no repositório do sistema (requer imagem com root).
fn install_system_ca(serial: &str, ca: &Ca) -> anyhow::Result<bool> {
    let r = emuzip_adb::root(serial).unwrap_or_default();
    if r.contains("cannot run as root") || r.contains("production") {
        return Ok(false);
    }
    std::thread::sleep(Duration::from_secs(2));
    if emuzip_adb::shell(serial, "id -u").map(|s| s.trim() != "0").unwrap_or(true) {
        return Ok(false);
    }
    let name = ca.android_name()?;
    let local = ca.dir.join(&name);
    std::fs::write(&local, &ca.cert_pem)?;
    emuzip_adb::push(serial, &local, &format!("/data/local/tmp/{name}"), |_, _| {})?;
    let out = emuzip_adb::shell(serial, &emuzip_netcap::ca::android_install_script(&name))?;
    Ok(out.contains("EMUZIP_CA_OK"))
}

pub fn start(ctx: &Ctx) {
    if ctx.net.lock().unwrap().capture.is_some() {
        return;
    }
    let Some(serial) = serial(ctx) else {
        ctx.toast_err(tr!("The device isn't ready yet"));
        return;
    };
    ctx.ui(|ui| ui.global::<Net>().set_starting(true));
    let task = ctx.task(tr!("Starting network capture"), None);
    let c = ctx.clone();
    ctx.spawn(async move {
        let r: anyhow::Result<(u16, bool)> = async {
            let ca = ca()?;
            task.progress(-1.0, tr!("Installing the EmuZip certificate…"));
            let s2 = serial.clone();
            let ca2 = Ca::load_or_create(&ca.dir)?;
            let system = tokio::task::spawn_blocking(move || install_system_ca(&s2, &ca2).unwrap_or(false)).await.unwrap_or(false);
            let port = emuzip_netcap::proxy::free_port(8890);
            let store = c.net.lock().unwrap().store.clone();
            let cap = emuzip_netcap::proxy::start(&ca, store, system, port).await?;
            task.progress(-1.0, tr!("Pointing the Android proxy to EmuZip…"));
            let s3 = serial.clone();
            let _ = std::fs::write(marker(&serial), port.to_string());
            tokio::task::spawn_blocking(move || emuzip_adb::shell(&s3, &format!("settings put global http_proxy 10.0.2.2:{port}"))).await??;
            let mut st = c.net.lock().unwrap();
            st.capture = Some(cap);
            st.serial = Some(serial.clone());
            Ok((port, system))
        }
        .await;
        match r {
            Ok((port, system)) => {
                task.done(tr!("Capturing on 10.0.2.2:{}", port));
                let status = if system {
                    tr!("EmuZip certificate installed in the system: HTTPS from all apps is decrypted.").to_string()
                } else {
                    tr!("Non-root image: install the certificate as a user to decrypt HTTPS from apps that trust user CAs (e.g. Chrome, debug apps). Otherwise, HTTPS shows up as a tunnel.").to_string()
                };
                c.toast_ok(if system { tr!("Capture started (HTTPS decrypted)") } else { tr!("Capture started") });
                c.ui(move |ui| {
                    let n = ui.global::<Net>();
                    n.set_capturing(true);
                    n.set_starting(false);
                    n.set_port(port as i32);
                    n.set_ca_system(system);
                    n.set_decrypt(system);
                    n.set_ca_status(status.into());
                });
            }
            Err(e) => {
                task.fail(format!("{e:#}"));
                c.toast_err(tr!("Couldn't start capture: {}", format!("{:#}", e)));
                c.ui(|ui| ui.global::<Net>().set_starting(false));
            }
        }
    });
}

/// Para a captura e devolve a internet direta ao Android. Seguro chamar várias vezes.
pub fn stop(ctx: &Ctx) {
    let (cap, serial) = {
        let mut st = ctx.net.lock().unwrap();
        (st.capture.take(), st.serial.take())
    };
    let had = cap.is_some();
    if let Some(c) = cap {
        c.stop();
    }
    if let Some(s) = serial {
        let _ = emuzip_adb::shell(&s, "settings put global http_proxy :0");
        let _ = std::fs::remove_file(marker(&s));
    }
    if had {
        ctx.ui(|ui| ui.global::<Net>().set_capturing(false));
    }
}

fn matches(e: &Entry, q: &str, kind: i32) -> bool {
    let k = match kind {
        1 => e.kind() == "json" || Entry::header(&e.req_headers, "x-requested-with").is_some(),
        2 => e.kind() == "doc",
        3 => e.kind() == "img",
        4 => e.status >= 400 || e.error.is_some(),
        5 => e.tls,
        _ => true,
    };
    k && (q.is_empty() || e.url.to_lowercase().contains(q) || e.status.to_string() == q || e.method.to_lowercase() == q)
}

fn fmt_ms(ms: f64) -> String {
    if ms <= 0.0 {
        "—".into()
    } else if ms < 1000.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

pub fn publish(ctx: &Ctx) {
    let c = ctx.clone();
    ctx.ui(move |ui| {
        let n = ui.global::<Net>();
        let (q, kind) = (n.get_filter().to_lowercase(), n.get_kind_filter());
        let store = c.net.lock().unwrap().store.clone();
        let all = store.snapshot();
        let filtered: Vec<&Entry> = all.iter().filter(|e| matches(e, &q, kind)).collect();
        let shown: Vec<NetEntry> = filtered
            .iter()
            .rev()
            .take(2000)
            .map(|e| NetEntry {
                id: e.id as i32,
                method: e.method.clone().into(),
                status: e.status as i32,
                host: e.host.clone().into(),
                path: e.path.clone().into(),
                kind: e.kind().into(),
                size: if e.method == "CONNECT" { "—".into() } else { human_bytes(e.resp_size.max(e.resp_body.len()) as u64).into() },
                time: fmt_ms(e.total_ms).into(),
                error: e.error.is_some(),
                tls: e.tls,
                decrypted: e.decrypted,
            })
            .collect();
        let bytes: usize = all.iter().map(|e| e.resp_size).sum();
        let errors = all.iter().filter(|e| e.status >= 400 || e.error.is_some()).count();
        n.set_total(all.len() as i32);
        n.set_stats(tr!("{} requests · {} shown · {} received · {} failed", all.len(), filtered.len(), human_bytes(bytes as u64), errors).into());
        c.net.lock().unwrap().filtered = filtered.iter().map(|e| e.id).collect();
        sync_model(n.get_entries(), shown);
    });
}

fn pretty_body(mime: &str, body: &[u8], truncated: bool) -> String {
    if body.is_empty() {
        return if truncated { tr!("(large body or streaming: not captured)").into() } else { tr!("(empty)") };
    }
    if !export::is_text(mime, body) {
        return tr!("(binary: {}, {})", if mime.is_empty() { tr!("unknown") } else { mime.to_string() }, human_bytes(body.len() as u64));
    }
    let text = String::from_utf8_lossy(body);
    if mime.contains("json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            return serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.to_string());
        }
    }
    let mut s = text.to_string();
    if s.len() > 200_000 {
        s.truncate(200_000);
        s.push_str(&tr!("\n… (truncated in preview)"));
    }
    s
}

fn select(ctx: &Ctx, ui: &MainWindow, id: i32) {
    let Some(e) = ctx.net.lock().unwrap().store.get(id as u64) else { return };
    ctx.net.lock().unwrap().selected = Some(e.id);
    let n = ui.global::<Net>();
    n.set_selected(id);
    n.set_d_url(e.url.clone().into());
    let status = if let Some(err) = &e.error { tr!("ERROR: {}", err) } else { e.status.to_string() };
    n.set_d_summary(
        format!("{} · {} · {} · {}", status, if e.mime.is_empty() { "—" } else { &e.mime }, human_bytes(e.resp_size as u64), fmt_ms(e.total_ms)).into(),
    );
    let hdr = |h: &[(String, String)]| h.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("\n");
    n.set_d_req_headers(format!("{} {} {}\n{}", e.method, e.path, e.http_version, hdr(&e.req_headers)).into());
    n.set_d_resp_headers(format!("{} {}\n{}", e.http_version, e.status, hdr(&e.resp_headers)).into());
    let req_mime = Entry::header(&e.req_headers, "content-type").unwrap_or("").to_string();
    n.set_d_req_body(pretty_body(&req_mime, &e.req_body, e.req_truncated).into());
    n.set_d_resp_body(pretty_body(&e.mime, &e.resp_body, e.resp_truncated).into());
    n.set_d_timing(
        tr!(
            "Start: {}\nWaiting for response (TTFB): {}\nTotal: {}\nSent: {}\nReceived: {}\nTLS: {}{}",
            export::iso8601(e.started_ms),
            fmt_ms(e.ttfb_ms),
            fmt_ms(e.total_ms),
            human_bytes(e.req_size as u64),
            human_bytes(e.resp_size as u64),
            if e.tls { tr!("yes") } else { tr!("no") },
            if e.tls && !e.decrypted { tr!(" (not decrypted)") } else { String::new() }
        )
        .into(),
    );
    // Preview de imagem via arquivo temporário.
    let mut has_img = false;
    if e.mime.starts_with("image/") && !e.resp_body.is_empty() {
        let ext = e.mime.trim_start_matches("image/").replace("svg+xml", "svg");
        let p = emuzip_core::data_dir().join("netcap").join(format!("preview.{ext}"));
        if std::fs::write(&p, &e.resp_body).is_ok() {
            if let Ok(img) = slint::Image::load_from_path(&p) {
                n.set_d_image(img);
                has_img = true;
            }
        }
    }
    n.set_d_has_image(has_img);
    if has_img && n.get_detail_tab() == 0 {
        n.set_detail_tab(1);
    }
}

fn export_entries(ctx: &Ctx, ui: &MainWindow) {
    let n = ui.global::<Net>();
    let st = ctx.net.lock().unwrap();
    let all = st.store.snapshot();
    let list: Vec<Entry> = match n.get_export_scope() {
        1 => all.into_iter().filter(|e| st.filtered.contains(&e.id)).collect(),
        2 => all.into_iter().filter(|e| Some(e.id) == st.selected).collect(),
        _ => all,
    };
    drop(st);
    if list.is_empty() {
        ctx.toast(0, tr!("Nothing to export"));
        return;
    }
    let (ext, name, content) = match n.get_export_format() {
        0 => ("har", "emuzip.har", export::har(&list, env!("CARGO_PKG_VERSION"))),
        1 => ("json", "emuzip-rede.json", export::json_export(&list)),
        2 => ("csv", "emuzip-rede.csv", export::csv(&list)),
        3 => ("json", "emuzip.postman_collection.json", export::postman(&list)),
        _ => ("sh", "emuzip-curl.sh", export::curl(&list)),
    };
    let Some(dest) = rfd::FileDialog::new().set_file_name(name).add_filter(ext.to_uppercase(), &[ext]).save_file() else { return };
    match std::fs::write(&dest, content) {
        Ok(_) => {
            let d = dest.clone();
            ctx.toast_with(1, tr!("{} requests exported", list.len()), tr!("Show"), Some(Box::new(move |_| crate::devices::open_path(d.parent().unwrap()))));
        }
        Err(e) => ctx.toast_err(e.to_string()),
    }
}

fn install_user_ca(ctx: &Ctx) {
    let Some(serial) = serial(ctx) else { return };
    let task = ctx.task(tr!("Installing EmuZip certificate"), None);
    ctx.blocking(move |c| {
        let r = (|| -> anyhow::Result<bool> {
            let ca = ca()?;
            if install_system_ca(&serial, &ca)? {
                return Ok(true);
            }
            let local = ca.dir.join("EmuZip-CA.crt");
            std::fs::write(&local, &ca.cert_pem)?;
            emuzip_adb::push(&serial, &local, "/sdcard/Download/EmuZip-CA.crt", |_, _| {})?;
            let _ = emuzip_adb::shell(&serial, "am start -a android.settings.SECURITY_SETTINGS");
            Ok(false)
        })();
        match r {
            Ok(true) => {
                task.done(tr!("Certificate installed in the system"));
                c.toast_ok(tr!("Certificate installed in the system: HTTPS will be decrypted"));
                c.ui(|ui| {
                    let n = ui.global::<Net>();
                    n.set_ca_system(true);
                    n.set_decrypt(true);
                    n.set_ca_status(tr!("EmuZip certificate installed in the system: HTTPS from all apps is decrypted.").into());
                });
                let c2 = c.clone();
                c.ui(move |_| set_decrypt(&c2, true));
            }
            Ok(false) => {
                task.done(tr!("EmuZip-CA.crt sent to Download"));
                c.toast_with(0, tr!("On Android: Settings > Security > More > Encryption & credentials > Install a certificate > CA certificate > Download/EmuZip-CA.crt. Then turn on \"Decrypt HTTPS\"."), "", None);
            }
            Err(e) => {
                task.fail(format!("{e:#}"));
                c.toast_err(format!("{e:#}"));
            }
        }
    });
}

fn set_decrypt(ctx: &Ctx, on: bool) {
    if let Some(c) = ctx.net.lock().unwrap().capture.as_ref() {
        c.decrypt.store(on, Ordering::Relaxed);
    }
}

pub fn wire(ctx: &Ctx, ui: &MainWindow) {
    let api = ui.global::<Api>();
    ui.global::<Net>().set_entries(crate::ctx::model(vec![]));
    ui.global::<Net>().set_ca_status(tr!("EmuZip becomes Android's proxy (10.0.2.2) to see every Request and Response.").into());
    let c = ctx.clone();
    api.on_net_start(move || start(&c));
    let c = ctx.clone();
    api.on_net_stop(move || {
        let c2 = c.clone();
        c.blocking(move |_| stop(&c2));
        c.toast(0, tr!("Capture stopped and Android internet restored"));
    });
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_net_clear(move || {
        c.net.lock().unwrap().store.clear();
        w.unwrap().global::<Net>().set_selected(-1);
        publish(&c);
    });
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_net_select(move |id| select(&c, &w.unwrap(), id));
    let c = ctx.clone();
    api.on_net_filter_changed(move || publish(&c));
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_net_export(move || export_entries(&c, &w.unwrap()));
    let c = ctx.clone();
    api.on_net_copy_curl(move || {
        let sel = c.net.lock().unwrap().selected;
        if let Some(e) = sel.and_then(|id| c.net.lock().unwrap().store.get(id)) {
            crate::prefs::copy_to_clipboard(&export::curl_one(&e));
            c.toast(0, tr!("cURL copied"));
        }
    });
    let c = ctx.clone();
    api.on_net_save_body(move |_| {
        let sel = c.net.lock().unwrap().selected;
        let Some(e) = sel.and_then(|id| c.net.lock().unwrap().store.get(id)) else { return };
        let name = e.path.split('?').next().unwrap_or("").rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("resposta").to_string();
        let Some(dest) = rfd::FileDialog::new().set_file_name(&name).save_file() else { return };
        match std::fs::write(&dest, &e.resp_body) {
            Ok(_) => c.toast_ok(tr!("Body saved ({})", human_bytes(e.resp_body.len() as u64))),
            Err(err) => c.toast_err(err.to_string()),
        }
    });
    let c = ctx.clone();
    api.on_net_install_ca(move || install_user_ca(&c));
    let c = ctx.clone();
    api.on_net_set_decrypt(move |on| set_decrypt(&c, on));

    // Atualiza a lista quando há novidades (no máx. ~3×/s) e respeita "Pausar".
    let (c, w) = (ctx.clone(), ui.as_weak());
    let timer = Box::leak(Box::new(slint::Timer::default()));
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(350), move || {
        let Some(ui) = w.upgrade() else { return };
        let store = c.net.lock().unwrap().store.clone();
        store.paused.store(ui.global::<Net>().get_paused(), Ordering::Relaxed);
        if store.dirty.swap(false, Ordering::Relaxed) && ui.global::<Player>().get_panel() == "network" {
            publish(&c);
        }
    });
}
