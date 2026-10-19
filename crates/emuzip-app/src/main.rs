//! EmuZip: emulador Android leve, em Rust + Slint.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[macro_use]
mod i18n;
mod ctx;
mod devices;
mod images;
mod network;
mod perf;
mod player;
mod prefs;

use ctx::{model, Ctx, Inner};
use slint::ComponentHandle;
use std::sync::atomic::AtomicI32;
use std::sync::{Arc, Mutex, RwLock};

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build()?;
    let ui = MainWindow::new()?;
    i18n::set_language(&std::env::var("EMUZIP_LANG").unwrap_or_else(|_| emuzip_core::Settings::load().language));

    let ctx = Ctx(Arc::new(Inner {
        ui: ui.as_weak(),
        rt: rt.handle().clone(),
        backends: RwLock::new(Arc::new(Vec::new())),
        settings: RwLock::new(emuzip_core::Settings::load()),
        devices: Mutex::default(),
        busy: Mutex::default(),
        boot_started: Mutex::default(),
        tasks: Mutex::default(),
        task_seq: AtomicI32::new(0),
        toast_seq: AtomicI32::new(0),
        toast_actions: Mutex::default(),
        confirm: Mutex::default(),
        prompt: Mutex::default(),
        catalog: Mutex::default(),
        downloads: Mutex::default(),
        player: Mutex::default(),
        perf: Mutex::default(),
        net: Mutex::default(),
    }));

    // Modelos iniciais (VecModel para atualizações incrementais).
    let s = ui.global::<Store>();
    s.set_devices(model(vec![]));
    s.set_catalog(model(vec![]));
    s.set_installed_images(model(vec![]));
    s.set_tasks(model(vec![]));
    s.set_toasts(model(vec![]));
    s.set_profiles(model(devices::profiles_model()));
    s.set_profile_names(model(emuzip_core::profiles::presets().into_iter().map(|p| p.name.into()).collect()));
    s.set_version(env!("CARGO_PKG_VERSION").into());
    s.set_backends(tr!("Detecting…").into());
    s.set_accel_status(tr!("Checking acceleration…").into());

    devices::wire(&ctx, &ui);
    images::wire(&ctx, &ui);
    prefs::wire(&ctx, &ui);
    player::wire(&ctx, &ui);
    perf::wire(&ctx, &ui);
    network::wire(&ctx, &ui);
    prefs::load_into_ui(&ctx, &ui);
    prefs::refresh_language_names(&ui);

    // Detecta ambiente em segundo plano (a janela abre imediatamente).
    prefs::refresh_environment(&ctx);
    devices::start_auto_refresh(&ctx);

    if let Ok(id) = std::env::var("EMUZIP_OPEN") {
        player::open(&ctx, &id);
    }
    if let Ok(dir) = std::env::var("EMUZIP_SHOT_DIR") {
        shot_tour(&ctx, &ui, dir);
    }

    ui.run()?;
    network::stop(&ctx);
    player::close(&ctx);
    rt.shutdown_timeout(std::time::Duration::from_millis(300));
    Ok(())
}

/// Modo de diagnóstico: percorre as telas e salva capturas só da janela do app.
fn shot_tour(ctx: &Ctx, ui: &MainWindow, dir: String) {
    let steps: Vec<(&str, Box<dyn Fn(&MainWindow)>)> = vec![
        ("01-dispositivos", Box::new(|_| {})),
        (
            "02-imagens",
            Box::new(|ui| {
                ui.global::<Store>().set_page(1);
            }),
        ),
        ("03-atividades", Box::new(|ui| ui.global::<Store>().set_page(2))),
        ("04-config", Box::new(|ui| ui.global::<Store>().set_page(3))),
        (
            "05-wizard",
            Box::new(|ui| {
                ui.global::<Store>().set_page(0);
                ui.global::<Api>().invoke_open_wizard();
            }),
        ),
        ("06-wizard-perfil", Box::new(|ui| ui.global::<Wizard>().set_step(1))),
        ("07-wizard-hw", Box::new(|ui| ui.global::<Wizard>().set_step(2))),
        ("08-wizard-revisao", Box::new(|ui| ui.global::<Wizard>().set_step(3))),
        (
            "09-player",
            Box::new(|ui| {
                ui.global::<Wizard>().set_open(false);
                if let Ok(id) = std::env::var("EMUZIP_SHOT_DEVICE") {
                    ui.global::<Api>().invoke_open_player(id.into());
                }
            }),
        ),
        (
            "10-perf",
            Box::new(|ui| {
                ui.global::<Player>().set_panel("perf".into());
                ui.global::<Api>().invoke_perf_open();
            }),
        ),
        ("11-perf-expandido", Box::new(|ui| ui.global::<Player>().set_panel_expanded(true))),
        ("12-rede-expandido", Box::new(|ui| ui.global::<Player>().set_panel("network".into()))),
        ("13-ajustado", Box::new(|ui| ui.global::<Api>().invoke_fit_window())),
        ("14-ajustado-painel", Box::new(|ui| ui.global::<Player>().set_panel("gps".into()))),
    ];
    let _ = std::fs::create_dir_all(&dir);
    let steps = std::rc::Rc::new(steps);
    let i = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let w = ui.as_weak();
    let timer = Box::leak(Box::new(slint::Timer::default()));
    let _ = ctx;
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(3500), move || {
        let ui = w.unwrap();
        let n = i.get();
        if n > 0 {
            if let Ok(img) = ui.window().take_snapshot() {
                let (wd, ht) = (img.width(), img.height());
                let path = format!("{dir}/{}.png", steps[n - 1].0);
                let _ = save_png(&path, wd, ht, img.as_bytes());
            }
        }
        if n >= steps.len() {
            let _ = slint::quit_event_loop();
            return;
        }
        (steps[n].1)(&ui);
        i.set(n + 1);
    });
}

fn save_png(path: &str, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    // PNG mínimo (sem dependências): filtro 0 por linha + zlib "stored".
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn adler(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in data {
            a = (a + x as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }
    let stride = (w * 4) as usize;
    let mut raw = Vec::with_capacity((stride + 1) * h as usize);
    for y in 0..h as usize {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * stride..(y + 1) * stride]);
    }
    let mut z = vec![0x78, 0x01];
    for (i, chunk) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    z.extend_from_slice(&adler(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut put = |t: &[u8], d: &[u8]| {
        out.extend_from_slice(&(d.len() as u32).to_be_bytes());
        let mut td = t.to_vec();
        td.extend_from_slice(d);
        out.extend_from_slice(&td);
        out.extend_from_slice(&crc(&td).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    put(b"IHDR", &ihdr);
    put(b"IDAT", &z);
    put(b"IEND", &[]);
    std::fs::write(path, out)
}
