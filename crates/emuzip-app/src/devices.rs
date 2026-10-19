//! Lista de dispositivos, ações e assistente de criação/edição.
use crate::ctx::{model, sync_model, Ctx};
use crate::{Api, DeviceItem, ImageItem, MainWindow, ProfileItem, Store, Wizard};
use emuzip_backend::avd::AvdBackend;
use emuzip_backend::{list_all, CreateSpec, StartOpts};
use emuzip_core::{profiles, BackendKind, Device, State};
use slint::{ComponentHandle, SharedString};
use std::time::{Duration, Instant};

const GPU: [&str; 4] = ["auto", "host", "swiftshader_indirect", "angle_indirect"];
const CAM_BACK: [&str; 4] = ["virtualscene", "emulated", "webcam0", "none"];
const CAM_FRONT: [&str; 3] = ["emulated", "webcam0", "none"];

fn kind_of(d: &Device) -> &'static str {
    let id = d.id.to_lowercase();
    if id.contains("tv") || d.tag.contains("TV") {
        "tv"
    } else if id.contains("wear") || d.tag.contains("Wear") {
        "wear"
    } else if d.width.max(d.height) > 0 && d.width.min(d.height) as f32 / d.width.max(d.height) as f32 > 0.7 {
        "tablet"
    } else {
        "phone"
    }
}

/// Recarrega a lista (bloqueante) e publica.
pub fn reload(ctx: &Ctx) {
    let devices = list_all(&ctx.backends());
    // Detecta transições para avisar o usuário.
    let prev: Vec<(String, State)> = ctx.devices.lock().unwrap().iter().map(|d| (d.id.clone(), d.state)).collect();
    for d in &devices {
        let old = prev.iter().find(|(id, _)| *id == d.id).map(|(_, s)| *s);
        if d.state == State::Running && matches!(old, Some(State::Booting)) {
            ctx.boot_started.lock().unwrap().remove(&d.id);
            ctx.toast_ok(tr!("{} is ready", d.name));
        }
        if d.state == State::Booting {
            ctx.boot_started.lock().unwrap().entry(d.id.clone()).or_insert_with(Instant::now);
        }
        if d.state == State::Stopped && matches!(old, Some(State::Booting)) {
            ctx.toast_with(2, tr!("{} closed during boot", d.name), tr!("View log"), Some(show_log_job(d.id.clone())));
            ctx.boot_started.lock().unwrap().remove(&d.id);
        }
    }
    *ctx.devices.lock().unwrap() = devices;
    publish(ctx);
}

fn show_log_job(id: String) -> crate::ctx::Job {
    Box::new(move |_| open_path(&emuzip_backend::avd::log_path(&id)))
}

pub fn open_path(p: &std::path::Path) {
    let prog = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(prog).arg(p).spawn();
}

/// Publica a lista filtrada na UI (lê busca/filtro da própria UI).
pub fn publish(ctx: &Ctx) {
    let devices = ctx.devices.lock().unwrap().clone();
    let busy = ctx.busy.lock().unwrap().clone();
    let boots = ctx.boot_started.lock().unwrap().clone();
    ctx.ui(move |ui| {
        let s = ui.global::<Store>();
        let q = s.get_search().to_lowercase();
        let f = s.get_filter();
        let items: Vec<DeviceItem> = devices
            .iter()
            .filter(|d| match f {
                1 => matches!(d.state, State::Running | State::Booting),
                2 => d.state == State::Stopped,
                3 => d.state == State::Broken,
                _ => true,
            })
            .filter(|d| q.is_empty() || d.name.to_lowercase().contains(&q) || d.android.to_lowercase().contains(&q) || d.id.to_lowercase().contains(&q))
            .map(|d| {
                let b = busy.get(&d.id);
                let boot = boots.get(&d.id).map(|t| (t.elapsed().as_secs_f32() / 50.0).min(0.95)).unwrap_or(-1.0);
                DeviceItem {
                    id: d.id.clone().into(),
                    name: d.name.clone().into(),
                    android: d.android.clone().into(),
                    tag: d.tag.clone().into(),
                    specs: d.summary().into(),
                    state: if b.map(|t| *t == tr!("Stopping…")).unwrap_or(false) {
                        4
                    } else {
                        match d.state {
                            State::Stopped => 0,
                            State::Booting => 1,
                            State::Running => 2,
                            State::Broken => 3,
                        }
                    },
                    busy: b.is_some(),
                    busy_text: b.cloned().unwrap_or_default().into(),
                    problem: d.problem.clone().unwrap_or_default().into(),
                    backend: d.backend.label().into(),
                    aspect: if d.height > 0 { d.width as f32 / d.height as f32 } else { 0.48 },
                    kind: kind_of(d).into(),
                    boot_progress: boot,
                }
            })
            .collect();
        sync_model(s.get_devices(), items);
        s.set_devices_loading(false);
    });
}

/// Atualização periódica em segundo plano.
pub fn start_auto_refresh(ctx: &Ctx) {
    let c = ctx.clone();
    ctx.spawn(async move {
        loop {
            let c2 = c.clone();
            let _ = tokio::task::spawn_blocking(move || reload(&c2)).await;
            let booting = c.devices.lock().unwrap().iter().any(|d| d.state == State::Booting) || !c.busy.lock().unwrap().is_empty();
            tokio::time::sleep(Duration::from_millis(if booting { 1200 } else { 3000 })).await;
        }
    });
}

pub fn start(ctx: &Ctx, id: &str, cold: bool, open_player: bool) {
    let Some(d) = ctx.device(id) else { return };
    let s = ctx.settings();
    let opts = StartOpts { cold_boot: cold || s.cold_boot, headless: s.embedded_display, gpu: s.gpu_mode.clone(), audio: s.audio, wipe: false };
    let id = id.to_string();
    ctx.set_busy(&id, Some(tr!("Powering on…")));
    let task = ctx.task(tr!("Starting {}", d.name), None);
    if open_player && s.embedded_display {
        crate::player::open(ctx, &id);
    }
    ctx.blocking(move |c| {
        let r =
            c.backends().iter().find(|b| b.kind() == d.backend).map(|b| b.start(&id, &opts)).unwrap_or_else(|| Err(anyhow::anyhow!("backend indisponível")));
        if let Err(e) = r {
            c.set_busy(&id, None);
            task.fail(format!("{e:#}"));
            c.toast_err(tr!("Couldn't start: {}", format!("{:#}", e)));
            return;
        }
        c.boot_started.lock().unwrap().insert(id.clone(), Instant::now());
        task.progress(-1.0, tr!("Emulator open, booting Android…"));
        // Acompanha até o boot terminar (máx. 5 min).
        let t0 = Instant::now();
        let mut cleared = false;
        while t0.elapsed() < Duration::from_secs(300) {
            std::thread::sleep(Duration::from_millis(1500));
            if !cleared && AvdBackend::find_running(&id).is_some() {
                c.set_busy(&id, None);
                cleared = true;
            }
            if d.backend == BackendKind::Qemu {
                c.set_busy(&id, None);
                task.done(tr!("VM started"));
                return;
            }
            let p = (t0.elapsed().as_secs_f32() / 50.0).min(0.95);
            match AvdBackend::find_running(&id) {
                Some(r) => {
                    let booted = emuzip_adb::shell(&r.serial(), "getprop sys.boot_completed").map(|s| s.trim() == "1").unwrap_or(false);
                    if booted {
                        task.done(tr!("Ready in {}s", t0.elapsed().as_secs()));
                        reload(&c);
                        return;
                    }
                    task.progress(p, tr!("Booting Android… {}s", t0.elapsed().as_secs()));
                }
                None if t0.elapsed() > Duration::from_secs(20) => {
                    c.set_busy(&id, None);
                    let log = std::fs::read_to_string(emuzip_backend::avd::log_path(&id)).unwrap_or_default();
                    let last: Vec<&str> = log.lines().filter(|l| l.contains("ERROR") || l.contains("FATAL")).take(3).collect();
                    task.fail(if last.is_empty() { tr!("The emulator closed. Check the log.") } else { last.join("\n") });
                    c.toast_with(2, tr!("The emulator closed during boot"), tr!("View log"), Some(show_log_job(id.clone())));
                    return;
                }
                None => task.progress(p, tr!("Opening the emulator…")),
            }
        }
        task.fail(tr!("Timed out waiting for boot"));
    });
}

pub fn stop(ctx: &Ctx, id: &str) {
    let Some(d) = ctx.device(id) else { return };
    let id = id.to_string();
    ctx.set_busy(&id, Some(tr!("Stopping…")));
    ctx.blocking(move |c| {
        let r = c.backends().iter().find(|b| b.kind() == d.backend).map(|b| b.stop(&d)).unwrap_or(Ok(()));
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(500));
            if AvdBackend::find_running(&id).is_none() {
                break;
            }
        }
        c.set_busy(&id, None);
        c.boot_started.lock().unwrap().remove(&id);
        match r {
            Ok(_) => c.toast(0, tr!("{} was powered off", d.name)),
            Err(e) => c.toast_err(format!("{e:#}")),
        }
        reload(&c);
    });
}

pub fn restart(ctx: &Ctx, id: &str) {
    let Some(r) = AvdBackend::find_running(id) else { return };
    let serial = r.serial();
    let name = ctx.device(id).map(|d| d.name).unwrap_or_default();
    ctx.blocking(move |c| {
        let _ = emuzip_adb::shell(&serial, "reboot");
        c.boot_started.lock().unwrap().insert(serial, Instant::now());
        c.toast(0, tr!("Restarting {}…", name));
    });
}

fn with_avd(ctx: &Ctx, title: String, ok: String, f: impl FnOnce(&AvdBackend) -> anyhow::Result<String> + Send + 'static) {
    let task = ctx.task(title, None);
    ctx.blocking(move |c| {
        let r = c.avd().ok_or_else(|| anyhow::anyhow!("Android SDK não encontrado")).and_then(|a| f(&a));
        match &r {
            Ok(m) if !m.is_empty() => {
                task.done(m.clone());
                c.toast_ok(m.clone());
            }
            _ => task.finish(&r, &ok),
        }
        reload(&c);
    });
}

pub fn ask_delete(ctx: &Ctx, id: &str) {
    let Some(d) = ctx.device(id) else { return };
    let text = tr!("“{}” and all its data will be erased. This can't be undone.", d.name);
    ctx.confirm(
        &tr!("Delete {}?", d.name),
        &text,
        true,
        Box::new(move |c| {
            let name = d.name.clone();
            let task = c.task(tr!("Deleting {}", name), None);
            c.blocking(move |c| {
                let r = c.backends().iter().find(|b| b.kind() == d.backend).map(|b| b.delete(&d)).unwrap_or(Ok(()));
                task.finish(&r, &tr!("{} deleted", name));
                reload(&c);
            });
        }),
    );
}

pub fn ask_wipe(ctx: &Ctx, id: &str) {
    let Some(d) = ctx.device(id) else { return };
    let id = id.to_string();
    ctx.confirm(
        &tr!("Factory reset {}?", d.name),
        tr!("The device's apps, accounts, files and snapshots will be erased. The hardware configuration is kept."),
        true,
        Box::new(move |c| {
            let name = d.name.clone();
            with_avd(c, tr!("Resetting {}", name), tr!("{} factory reset", name), move |a| a.wipe(&id).map(|_| String::new()));
        }),
    );
}

pub fn ask_clone(ctx: &Ctx, id: &str) {
    let Some(d) = ctx.device(id) else { return };
    let id = id.to_string();
    ctx.prompt(
        &tr!("Clone device"),
        tr!("Copy name:"),
        &tr!("{} (copy)", d.name),
        Box::new(move |c, name| {
            let n2 = name.clone();
            with_avd(c, format!("Clonando {}", d.name), tr!("{} created", n2), move |a| a.clone_avd(&id, &name).map(|_| String::new()));
        }),
    );
}

pub fn ask_rename(ctx: &Ctx, id: &str) {
    let Some(d) = ctx.device(id) else { return };
    let id = id.to_string();
    ctx.prompt(
        &tr!("Rename device"),
        tr!("New name:"),
        &d.name,
        Box::new(move |c, name| {
            let n2 = name.clone();
            with_avd(c, tr!("Renaming"), tr!("Renamed to {}", n2), move |a| a.rename(&id, &name).map(|_| String::new()));
        }),
    );
}

pub fn repair(ctx: &Ctx, id: &str) {
    let id = id.to_string();
    ctx.set_busy(&id, Some(tr!("Repairing…")));
    let id2 = id.clone();
    with_avd(ctx, tr!("Repairing {}", id), tr!("Repaired"), move |a| a.repair(&id2));
    ctx.set_busy(&id, None);
}

// ---------------- Wizard ----------------

pub fn profiles_model() -> Vec<ProfileItem> {
    profiles::presets()
        .into_iter()
        .map(|p| ProfileItem {
            id: p.id.into(),
            name: p.name.into(),
            maker: p.manufacturer.into(),
            kind: match p.kind {
                profiles::DeviceKind::Tablet | profiles::DeviceKind::Foldable => "tablet",
                profiles::DeviceKind::Tv => "tv",
                profiles::DeviceKind::Wear => "wear",
                _ => "phone",
            }
            .into(),
            width: p.width as i32,
            height: p.height as i32,
            density: p.density as i32,
            ram: p.ram_mb as i32,
            cpus: p.cpus as i32,
            storage: p.storage_gb as i32,
        })
        .collect()
}

pub fn installed_images(ctx: &Ctx) -> Vec<(emuzip_core::SystemImage, ImageItem)> {
    ctx.avd()
        .map(|a| a.images())
        .unwrap_or_default()
        .into_iter()
        .map(|i| {
            let item = ImageItem {
                id: i.id.clone().into(),
                title: i.title().into(),
                subtitle: format!("{} · {} · {}", i.api_dir, i.tag, i.abi).into(),
                size: i.abi.clone().into(),
                tag: i.tag.clone().into(),
                installed: true,
                downloading: false,
                progress: 0.0,
                status: SharedString::new(),
                api: i.api as i32,
            };
            (i, item)
        })
        .collect()
}

pub fn pick_profile(ui: &MainWindow, i: i32) {
    let w = ui.global::<Wizard>();
    w.set_profile_index(i);
    if let Some(p) = profiles::presets().get(i as usize) {
        w.set_width(p.width as i32);
        w.set_height(p.height as i32);
        w.set_density(p.density as i32);
        w.set_ram(p.ram_mb as i32);
        w.set_cpus(p.cpus as i32);
        w.set_storage(p.storage_gb as i32);
        let base = w.get_name();
        if base.is_empty() || base.starts_with("Meu ") || profiles::presets().iter().any(|q| q.name == base.as_str()) {
            w.set_name(p.name.clone().into());
        }
    }
}

pub fn open_wizard(ctx: &Ctx, ui: &MainWindow) {
    let imgs = installed_images(ctx);
    let s = ctx.settings();
    let st = ui.global::<Store>();
    st.set_installed_images(model(imgs.iter().map(|(_, i)| i.clone()).collect()));
    let w = ui.global::<Wizard>();
    w.set_edit_id("".into());
    w.set_step(0);
    w.set_error("".into());
    w.set_creating(false);
    w.set_image_index(imgs.iter().position(|(i, _)| i.play_store()).or(if imgs.is_empty() { None } else { Some(0) }).map(|i| i as i32).unwrap_or(-1));
    let pi = profiles::presets().iter().position(|p| p.id == s.default_profile).unwrap_or(0) as i32;
    w.set_name("".into());
    pick_profile(ui, pi);
    w.set_ram(s.default_ram_mb as i32);
    w.set_cpus(s.default_cpus as i32);
    w.set_gpu_index(GPU.iter().position(|g| *g == s.gpu_mode).unwrap_or(0) as i32);
    w.set_cam_back_index(1);
    w.set_cam_front_index(0);
    w.set_keyboard(true);
    w.set_nav_buttons(false);
    w.set_start_after(true);
    w.set_open(true);
}

pub fn open_edit(ctx: &Ctx, ui: &MainWindow, id: &str) {
    let Some(d) = ctx.device(id) else { return };
    open_wizard(ctx, ui);
    let cfg = AvdBackend::config(id);
    let w = ui.global::<Wizard>();
    let num = |k: &str, def: u32| cfg.get(k).and_then(|v| v.trim_end_matches(['G', 'M', 'B']).trim().parse::<u32>().ok()).unwrap_or(def) as i32;
    w.set_edit_id(id.into());
    w.set_name(d.name.into());
    w.set_width(num("hw.lcd.width", 1080));
    w.set_height(num("hw.lcd.height", 1920));
    w.set_density(num("hw.lcd.density", 420));
    w.set_ram(num("hw.ramSize", 2048));
    w.set_cpus(num("hw.cpu.ncore", 2));
    let storage = cfg.get("disk.dataPartition.size").cloned().unwrap_or_default();
    w.set_storage(if storage.ends_with('G') { num("disk.dataPartition.size", 6) } else { (storage.parse::<u64>().unwrap_or(6 << 30) >> 30) as i32 });
    w.set_gpu_index(GPU.iter().position(|g| Some(*g) == cfg.get("hw.gpu.mode").map(String::as_str)).unwrap_or(0) as i32);
    w.set_cam_back_index(CAM_BACK.iter().position(|g| Some(*g) == cfg.get("hw.camera.back").map(String::as_str)).unwrap_or(1) as i32);
    w.set_cam_front_index(CAM_FRONT.iter().position(|g| Some(*g) == cfg.get("hw.camera.front").map(String::as_str)).unwrap_or(0) as i32);
    w.set_keyboard(cfg.get("hw.keyboard").map(|v| v == "yes").unwrap_or(true));
    w.set_profile_index(profiles::presets().iter().position(|p| Some(&p.id) == cfg.get("hw.device.name")).map(|i| i as i32).unwrap_or(-1));
    let sysdir = cfg.get("image.sysdir.1").cloned().unwrap_or_default();
    let imgs = installed_images(ctx);
    w.set_image_index(imgs.iter().position(|(i, _)| i.sysdir() == sysdir || sysdir.contains(&i.id)).map(|i| i as i32).unwrap_or(-1));
    w.set_step(2);
}

pub fn wizard_create(ctx: &Ctx, ui: &MainWindow) {
    let w = ui.global::<Wizard>();
    let imgs = installed_images(ctx);
    let edit_id = w.get_edit_id().to_string();
    let Some((img, _)) = imgs.get(w.get_image_index().max(0) as usize).cloned() else {
        w.set_error(tr!("Select an installed image.").into());
        w.set_step(0);
        return;
    };
    let profile = profiles::presets().get(w.get_profile_index().max(0) as usize).map(|p| p.id.clone()).unwrap_or_else(|| "custom".into());
    let spec = CreateSpec {
        name: w.get_name().trim().to_string(),
        image: img.id.clone(),
        profile,
        width: w.get_width() as u32,
        height: w.get_height() as u32,
        density: w.get_density() as u32,
        ram_mb: w.get_ram() as u32,
        cpus: w.get_cpus() as u32,
        storage_gb: w.get_storage() as u32,
        gpu: GPU[w.get_gpu_index().clamp(0, 3) as usize].into(),
        camera_back: CAM_BACK[w.get_cam_back_index().clamp(0, 3) as usize].into(),
        camera_front: CAM_FRONT[w.get_cam_front_index().clamp(0, 2) as usize].into(),
        keyboard: w.get_keyboard(),
        nav_buttons: w.get_nav_buttons(),
    };
    let start_after = w.get_start_after() && edit_id.is_empty();
    w.set_creating(true);
    w.set_error("".into());
    let title = if edit_id.is_empty() { format!("Criando {}", spec.name) } else { format!("Salvando {}", spec.name) };
    let task = ctx.task(title, None);
    task.progress(-1.0, tr!("Saving configuration…"));
    ctx.blocking(move |c| {
        let r: anyhow::Result<String> = (|| {
            let a = c.avd().ok_or_else(|| anyhow::anyhow!("Android SDK não encontrado"))?;
            if edit_id.is_empty() {
                use emuzip_backend::Backend;
                a.create(&spec)
            } else {
                let full = a.build_config(&spec, &img);
                let keys = [
                    "avd.ini.displayname",
                    "hw.lcd.width",
                    "hw.lcd.height",
                    "hw.lcd.density",
                    "hw.ramSize",
                    "hw.cpu.ncore",
                    "disk.dataPartition.size",
                    "hw.gpu.mode",
                    "hw.camera.back",
                    "hw.camera.front",
                    "hw.keyboard",
                    "hw.navigation.buttons",
                    "hw.initialOrientation",
                    "image.sysdir.1",
                    "tag.id",
                    "tag.display",
                    "target",
                    "PlayStore.enabled",
                    "abi.type",
                ];
                let changes: Vec<(&str, String)> = keys.iter().filter_map(|k| full.get(*k).map(|v| (*k, v.clone()))).collect();
                AvdBackend::set_config(&edit_id, &changes)?;
                Ok(edit_id.clone())
            }
        })();
        let ok = r.is_ok();
        let msg = if edit_id.is_empty() { format!("{} criado", spec.name) } else { tr!("Changes saved").to_string() };
        task.finish(&r, &msg);
        let err = r.as_ref().err().map(|e| format!("{e:#}")).unwrap_or_default();
        c.ui(move |ui| {
            let w = ui.global::<Wizard>();
            w.set_creating(false);
            if ok {
                w.set_open(false);
            } else {
                w.set_error(err.into());
            }
        });
        reload(&c);
        if let (true, Ok(id)) = (start_after, r) {
            let c2 = c.clone();
            c.ui(move |_| start(&c2, &id, false, true));
        }
    });
}

pub fn wire(ctx: &Ctx, ui: &MainWindow) {
    let api = ui.global::<Api>();
    let c = ctx.clone();
    api.on_refresh(move || {
        let c = c.clone();
        c.clone().blocking(move |c| reload(&c));
    });
    let c = ctx.clone();
    api.on_filter_changed(move || publish(&c));
    let c = ctx.clone();
    api.on_start(move |id| start(&c, &id, false, true));
    let c = ctx.clone();
    api.on_cold_boot(move |id| start(&c, &id, true, true));
    let c = ctx.clone();
    api.on_stop(move |id| stop(&c, &id));
    let c = ctx.clone();
    api.on_restart(move |id| restart(&c, &id));
    let c = ctx.clone();
    api.on_ask_delete(move |id| ask_delete(&c, &id));
    let c = ctx.clone();
    api.on_ask_wipe(move |id| ask_wipe(&c, &id));
    let c = ctx.clone();
    api.on_ask_clone(move |id| ask_clone(&c, &id));
    let c = ctx.clone();
    api.on_ask_rename(move |id| ask_rename(&c, &id));
    let c = ctx.clone();
    api.on_repair(move |id| repair(&c, &id));
    api.on_open_folder(move |id| AvdBackend::open_folder(&id));
    api.on_show_log(move |id| {
        let p = emuzip_backend::avd::log_path(&id);
        if p.exists() {
            open_path(&p);
        }
    });
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_edit(move |id| open_edit(&c, &w.unwrap(), &id));
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_open_wizard(move || open_wizard(&c, &w.unwrap()));
    let w = ui.as_weak();
    api.on_pick_profile(move |i| pick_profile(&w.unwrap(), i));
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_wizard_create(move || wizard_create(&c, &w.unwrap()));
    let c = ctx.clone();
    api.on_confirm(move || c.run_confirm());
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_prompt_submit(move || {
        let v = w.unwrap().global::<Store>().get_prompt_value().to_string();
        c.run_prompt(v);
    });
}
