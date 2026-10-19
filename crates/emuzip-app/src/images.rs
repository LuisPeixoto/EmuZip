//! Imagens instaladas, catálogo oficial e downloads com progresso.
use crate::ctx::{model, sync_model, Ctx, Download};
use crate::{Api, ImageItem, MainWindow, Store};
use emuzip_core::human_bytes;
use emuzip_repo::Phase;
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub fn publish_installed(ctx: &Ctx) {
    let items: Vec<ImageItem> = crate::devices::installed_images(ctx).into_iter().map(|(_, i)| i).collect();
    ctx.ui(move |ui| ui.global::<Store>().set_installed_images(model(items)));
}

fn matches_filter(img: &emuzip_repo::RemoteImage, f: i32, q: &str) -> bool {
    let ok = match f {
        1 => img.tag == "google_apis_playstore",
        2 => img.tag == "google_apis",
        3 => img.tag == "default" || img.tag.starts_with("aosp"),
        4 => img.tag.contains("tv"),
        5 => img.tag.contains("wear"),
        6 => !["google_apis_playstore", "google_apis", "default"].contains(&img.tag.as_str()) && !img.tag.contains("tv") && !img.tag.contains("wear"),
        _ => true,
    };
    ok && (q.is_empty() || {
        let hay = format!("{} {} {} {} {}", img.api, img.title(), img.tag, img.abi, img.display_name).to_lowercase();
        q.split_whitespace().all(|w| hay.contains(w))
    })
}

/// Arquiteturas que rodam bem nesta máquina primeiro.
fn host_abi_rank(abi: &str) -> u8 {
    let native = if cfg!(target_arch = "aarch64") { abi.starts_with("arm64") } else { abi == "x86_64" || abi == "x86" };
    if native {
        0
    } else {
        1
    }
}

pub fn publish_catalog(ctx: &Ctx) {
    let mut cat = ctx.catalog.lock().unwrap().clone();
    cat.sort_by_key(|i| (std::cmp::Reverse(i.api), host_abi_rank(&i.abi), i.tag.clone()));
    let downloads = ctx.downloads.lock().unwrap().clone();
    let installed: Vec<String> = ctx.avd().map(|a| a.images().into_iter().map(|i| i.id).collect()).unwrap_or_default();
    ctx.ui(move |ui| {
        let s = ui.global::<Store>();
        let (f, q) = (s.get_catalog_filter(), s.get_catalog_search().to_lowercase());
        let items: Vec<ImageItem> = cat
            .iter()
            .filter(|i| matches_filter(i, f, &q))
            .take(400)
            .map(|i| {
                let d = downloads.get(&i.id());
                ImageItem {
                    id: i.id().into(),
                    title: i.title().into(),
                    subtitle: format!("{} · {} · rev {}{}", i.abi, i.package, i.revision, if host_abi_rank(&i.abi) == 0 { " · nativa" } else { "" }).into(),
                    size: human_bytes(i.size).into(),
                    tag: i.tag.clone().into(),
                    installed: installed.contains(&i.id()),
                    downloading: d.is_some(),
                    progress: d.map(|d| d.progress).unwrap_or(0.0),
                    status: d.map(|d| d.status.clone()).unwrap_or_default().into(),
                    api: i.api as i32,
                }
            })
            .collect();
        sync_model(s.get_catalog(), items);
    });
}

pub fn load_catalog(ctx: &Ctx) {
    ctx.ui(|ui| {
        let s = ui.global::<Store>();
        s.set_catalog_loading(true);
        s.set_catalog_error("".into());
    });
    let c = ctx.clone();
    ctx.spawn(async move {
        let r = emuzip_repo::catalog().await;
        match r {
            Ok(v) if !v.is_empty() => {
                *c.catalog.lock().unwrap() = v;
                publish_catalog(&c);
                c.ui(|ui| ui.global::<Store>().set_catalog_loading(false));
            }
            Ok(_) => c.ui(|ui| {
                let s = ui.global::<Store>();
                s.set_catalog_loading(false);
                s.set_catalog_error(tr!("Empty catalog. Check the proxy or mirror in Settings.").into());
            }),
            Err(e) => {
                let m = format!("{e:#}");
                c.ui(move |ui| {
                    let s = ui.global::<Store>();
                    s.set_catalog_loading(false);
                    s.set_catalog_error(m.into());
                });
            }
        }
    });
}

pub fn download(ctx: &Ctx, id: &str) {
    let Some(img) = ctx.catalog.lock().unwrap().iter().find(|i| i.id() == id).cloned() else { return };
    let Some(sdk) = ctx.avd().map(|a| a.sdk) else {
        ctx.toast_err(tr!("Set up the Android SDK before downloading images"));
        return;
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let task = ctx.task(tr!("Downloading {}", img.title()), Some(cancel.clone()));
    ctx.downloads.lock().unwrap().insert(id.to_string(), Download { progress: 0.0, status: tr!("Connecting…").into(), cancel: cancel.clone() });
    publish_catalog(ctx);
    let c = ctx.clone();
    let id = id.to_string();
    ctx.spawn(async move {
        let c2 = c.clone();
        let id2 = id.clone();
        let task_ref = &task;
        let r = emuzip_repo::install(&img, &sdk, cancel, move |p| {
            let (prog, status) = match p {
                Phase::Downloading { done, total, bytes_per_sec } => {
                    let eta = if bytes_per_sec > 0.0 { (total.saturating_sub(done)) as f64 / bytes_per_sec } else { 0.0 };
                    (
                        done as f32 / total.max(1) as f32 * 0.9,
                        format!("{} de {} · {}/s · faltam {}", human_bytes(done), human_bytes(total), human_bytes(bytes_per_sec as u64), fmt_eta(eta)),
                    )
                }
                Phase::Verifying => (0.9, tr!("Verifying integrity (SHA-1)…").to_string()),
                Phase::Extracting { done, total } => (0.9 + done as f32 / total.max(1) as f32 * 0.1, tr!("Extracting files {}/{}", done, total)),
            };
            task_ref.progress(prog, status.clone());
            if let Some(d) = c2.downloads.lock().unwrap().get_mut(&id2) {
                d.progress = prog;
                d.status = status;
            }
            publish_catalog(&c2);
        })
        .await;
        c.downloads.lock().unwrap().remove(&id);
        task.finish(&r, &tr!("image" => "{} installed", img.title()));
        publish_catalog(&c);
        publish_installed(&c);
    });
}

fn fmt_eta(s: f64) -> String {
    let s = s as u64;
    if s >= 3600 {
        format!("{}h{:02}", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}min", s / 60)
    } else {
        format!("{s}s")
    }
}

pub fn cancel(ctx: &Ctx, id: &str) {
    if let Some(d) = ctx.downloads.lock().unwrap().get(id) {
        d.cancel.store(true, Ordering::Relaxed);
    }
}

pub fn remove(ctx: &Ctx, id: &str) {
    let Some(a) = ctx.avd() else { return };
    let Some(img) = a.images().into_iter().find(|i| i.id == id) else { return };
    let used: Vec<String> = ctx.devices.lock().unwrap().iter().filter(|d| AvdBackendCfg::sysdir(&d.id).contains(&img.id)).map(|d| d.name.clone()).collect();
    let warn = if used.is_empty() { String::new() } else { tr!(" Used by: {}.", used.join(", ")) };
    ctx.confirm(
        &format!("Remover {}?", img.title()),
        &tr!("The image will be deleted from disk ({}).{}", img.path.display(), warn),
        true,
        Box::new(move |c| {
            let task = c.task(tr!("Removing {}", img.title()), None);
            let r = std::fs::remove_dir_all(&img.path).map_err(anyhow::Error::from);
            task.finish(&r, &tr!("Image removed"));
            publish_installed(c);
            publish_catalog(c);
        }),
    );
}

struct AvdBackendCfg;
impl AvdBackendCfg {
    fn sysdir(id: &str) -> String {
        emuzip_backend::avd::AvdBackend::config(id).get("image.sysdir.1").cloned().unwrap_or_default()
    }
}

pub fn wire(ctx: &Ctx, ui: &MainWindow) {
    let api = ui.global::<Api>();
    let c = ctx.clone();
    api.on_load_catalog(move || load_catalog(&c));
    let c = ctx.clone();
    api.on_catalog_filter_changed(move || publish_catalog(&c));
    let c = ctx.clone();
    api.on_download_image(move |id| download(&c, &id));
    let c = ctx.clone();
    api.on_cancel_download(move |id| cancel(&c, &id));
    let c = ctx.clone();
    api.on_remove_image(move |id| remove(&c, &id));
    let c = ctx.clone();
    api.on_cancel_task(move |id| c.cancel_task(id));
    let c = ctx.clone();
    api.on_clear_tasks(move || c.clear_tasks());
    let c = ctx.clone();
    api.on_dismiss_toast(move |id| c.dismiss_toast(id));
    let c = ctx.clone();
    api.on_toast_action(move |id| c.toast_action(id));
}
