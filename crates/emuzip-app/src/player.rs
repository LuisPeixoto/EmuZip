//! Tela embutida (streaming gRPC), entrada e ferramentas do dispositivo.
use crate::ctx::{model, Ctx};
use crate::{Api, LogLine, MainWindow, NamedValue, Player, Store};
use emuzip_backend::avd::{AvdBackend, Running};
use emuzip_grpc::control::{self, image_format::ImgFormat};
use emuzip_grpc::Emulator;
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer, SharedString};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub enum Input {
    Mouse(i32, i32, bool),
    Key(String),
}

pub struct Session {
    pub id: String,
    pub stop: Arc<AtomicBool>,
    pub input: mpsc::UnboundedSender<Input>,
    pub emu: Arc<Mutex<Option<Emulator>>>,
    pub running: Arc<Mutex<Option<Running>>>,
    /// Tamanho real da tela do device (para mapear toques).
    pub dev_size: Arc<Mutex<(u32, u32)>>,
    pub frame_size: Arc<Mutex<(u32, u32)>>,
    pub logcat: Arc<Mutex<VecDeque<LogLine>>>,
    pub record: Arc<Mutex<Option<Instant>>>,
    pub route_stop: Arc<AtomicBool>,
}

impl Session {
    fn serial(&self) -> Option<String> {
        self.running.lock().unwrap().as_ref().map(Running::serial)
    }
    fn console(&self) -> Option<u16> {
        self.running.lock().unwrap().as_ref().map(|r| r.console_port)
    }
    fn emu(&self) -> Option<Emulator> {
        self.emu.lock().unwrap().clone()
    }
}

macro_rules! session {
    ($ctx:expr) => {
        match $ctx.player.lock().unwrap().as_ref() {
            Some(s) => (s.serial(), s.console(), s.emu(), s.id.clone()),
            None => return,
        }
    };
}

pub fn open(ctx: &Ctx, id: &str) {
    close(ctx);
    let Some(d) = ctx.device(id) else { return };
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::unbounded_channel();
    let sess = Session {
        id: id.to_string(),
        stop: stop.clone(),
        input: tx,
        emu: Arc::default(),
        running: Arc::default(),
        dev_size: Arc::new(Mutex::new((d.width.max(1), d.height.max(1)))),
        frame_size: Arc::new(Mutex::new((d.width.max(1), d.height.max(1)))),
        logcat: Arc::default(),
        record: Arc::default(),
        route_stop: Arc::new(AtomicBool::new(true)),
    };
    let (emu, running, dev, frame, logbuf) = (sess.emu.clone(), sess.running.clone(), sess.dev_size.clone(), sess.frame_size.clone(), sess.logcat.clone());
    *ctx.player.lock().unwrap() = Some(sess);
    let info = format!("{} · {} · {}", d.android, d.tag, d.summary());
    let (name, w, h) = (d.name.clone(), d.width, d.height);
    ctx.ui(move |ui| {
        let p = ui.global::<Player>();
        p.set_device_id(SharedString::from(d.id.as_str()));
        p.set_device_name(name.into());
        p.set_device_info(info.into());
        p.set_connected(false);
        p.set_status(tr!("Waiting for the emulator…").into());
        p.set_screen(slint::Image::default());
        p.set_screen_w(w.max(1) as i32);
        p.set_screen_h(h.max(1) as i32);
        p.set_packages(model(vec![]));
        p.set_snapshots(model(vec![]));
        p.set_logcat(model(vec![]));
        p.set_props(model(vec![]));
        p.set_recording(false);
        ui.global::<Store>().set_page(4);
    });
    let fps = ctx.settings().stream_fps.max(5);
    let c = ctx.clone();
    let id = id.to_string();
    ctx.spawn(stream_loop(c.clone(), id.clone(), stop.clone(), emu.clone(), running.clone(), dev, frame, fps));
    ctx.spawn(input_loop(emu, rx, stop.clone()));
    let c2 = c.clone();
    let s2 = stop.clone();
    ctx.blocking(move |_| logcat_loop(c2, running, logbuf, s2));
}

pub fn close(ctx: &Ctx) {
    // Nunca deixar o Android apontando para um proxy desligado.
    let c = ctx.clone();
    ctx.blocking(move |_| crate::network::stop(&c));
    crate::perf::stop(ctx);
    if let Some(s) = ctx.player.lock().unwrap().take() {
        s.stop.store(true, Ordering::Relaxed);
        s.route_stop.store(true, Ordering::Relaxed);
    }
}

#[allow(clippy::too_many_arguments)]
async fn stream_loop(
    ctx: Ctx,
    id: String,
    stop: Arc<AtomicBool>,
    emu_slot: Arc<Mutex<Option<Emulator>>>,
    run_slot: Arc<Mutex<Option<Running>>>,
    dev: Arc<Mutex<(u32, u32)>>,
    frame: Arc<Mutex<(u32, u32)>>,
    fps: u32,
) {
    let status = |c: &Ctx, s: &str| {
        let s = s.to_string();
        c.ui(move |ui| {
            let p = ui.global::<Player>();
            p.set_connected(false);
            p.set_status(s.into());
        });
    };
    let min_frame = Duration::from_millis(1000 / fps as u64);
    while !stop.load(Ordering::Relaxed) {
        let Some(r) = AvdBackend::find_running(&id) else {
            status(&ctx, &tr!("Powering on the emulator…"));
            tokio::time::sleep(Duration::from_millis(800)).await;
            continue;
        };
        *run_slot.lock().unwrap() = Some(r.clone());
        let Some(port) = r.grpc_port else {
            status(&ctx, &tr!("This emulator was started without gRPC. Stop it and start it from EmuZip."));
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        };
        let mut emu = match Emulator::connect(port, r.grpc_token.as_deref()).await {
            Ok(e) => e,
            Err(_) => {
                status(&ctx, &tr!("Connecting to display…"));
                tokio::time::sleep(Duration::from_millis(700)).await;
                continue;
            }
        };
        *emu_slot.lock().unwrap() = Some(emu.clone());
        // Proxy órfão de uma captura anterior (app fechado à força) deixaria o Android sem internet.
        if ctx.net.lock().unwrap().capture.is_none() {
            let s = r.serial();
            std::thread::spawn(move || crate::network::cleanup_orphan(&s));
        }
        // Resolução real do display.
        if let Ok(st) = emu.ctl.get_status(()).await {
            if let Some(hw) = st.into_inner().hardware_config {
                let get = |k: &str| hw.entry.iter().find(|e| e.key == k).and_then(|e| e.value.parse::<u32>().ok());
                if let (Some(w), Some(h)) = (get("hw.lcd.width"), get("hw.lcd.height")) {
                    *dev.lock().unwrap() = (w, h);
                }
            }
        }
        let (dw, _) = *dev.lock().unwrap();
        let req = control::ImageFormat { format: ImgFormat::Rgba8888 as i32, width: dw.min(900), ..Default::default() };
        let mut stream = match emu.ctl.stream_screenshot(req).await {
            Ok(s) => s.into_inner(),
            Err(e) => {
                status(&ctx, &tr!("Waiting for display… ({})", e.message()));
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let mut last = Instant::now() - min_frame;
        let mut count = 0u32;
        let mut tick = Instant::now();
        let mut first = true;
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let msg = match tokio::time::timeout(Duration::from_secs(5), stream.message()).await {
                Ok(Ok(Some(m))) => m,
                Ok(Ok(None)) | Ok(Err(_)) => break,
                Err(_) => {
                    if AvdBackend::find_running(&id).is_none() {
                        break;
                    }
                    continue;
                }
            };
            if last.elapsed() < min_frame {
                continue;
            }
            last = Instant::now();
            let fmt = msg.format.unwrap_or_default();
            let (w, h) = (fmt.width, fmt.height);
            if w == 0 || h == 0 || msg.image.len() < (w * h * 4) as usize {
                continue;
            }
            *frame.lock().unwrap() = (w, h);
            count += 1;
            let fps_now = if tick.elapsed() >= Duration::from_secs(1) {
                let f = count;
                count = 0;
                tick = Instant::now();
                Some(f as i32)
            } else {
                None
            };
            let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&msg.image[..(w * h * 4) as usize], w, h);
            let became = first;
            first = false;
            ctx.ui(move |ui| {
                let p = ui.global::<Player>();
                p.set_screen(slint::Image::from_rgba8(buf));
                p.set_screen_w(w as i32);
                p.set_screen_h(h as i32);
                if became {
                    p.set_connected(true);
                    if emuzip_core::Settings::load().auto_fit_window {
                        fit_window(&ui);
                    }
                }
                if let Some(f) = fps_now {
                    p.set_fps(f);
                }
            });
        }
        *emu_slot.lock().unwrap() = None;
        if AvdBackend::find_running(&id).is_none() {
            status(&ctx, &tr!("The device was powered off."));
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

async fn input_loop(emu: Arc<Mutex<Option<Emulator>>>, mut rx: mpsc::UnboundedReceiver<Input>, stop: Arc<AtomicBool>) {
    while let Some(ev) = rx.recv().await {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let Some(mut e) = emu.lock().unwrap().clone() else { continue };
        match ev {
            Input::Mouse(x, y, down) => {
                let _ = e.ctl.send_mouse(control::MouseEvent { x, y, buttons: if down { 1 } else { 0 }, display: 0 }).await;
            }
            Input::Key(k) => {
                let _ = e
                    .ctl
                    .send_key(control::KeyboardEvent { event_type: control::keyboard_event::KeyEventType::Keypress as i32, key: k, ..Default::default() })
                    .await;
            }
        }
    }
}

fn parse_log(l: &str) -> Option<LogLine> {
    // Formato brief: "D/Tag( 123): mensagem"
    let (head, msg) = l.split_once("): ")?;
    let (lvl, rest) = head.split_once('/')?;
    let tag = rest.rsplit_once('(').map(|(t, _)| t.trim()).unwrap_or(rest);
    Some(LogLine { level: lvl.trim().into(), tag: tag.into(), text: msg.into() })
}

fn level_rank(l: &str) -> i32 {
    match l {
        "V" => 0,
        "D" => 1,
        "I" => 2,
        "W" => 3,
        "E" | "F" | "A" => 4,
        _ => 0,
    }
}

fn logcat_loop(ctx: Ctx, running: Arc<Mutex<Option<Running>>>, buf: Arc<Mutex<VecDeque<LogLine>>>, stop: Arc<AtomicBool>) {
    // Publicador periódico (aplica filtro e pausa).
    let (c2, b2, s2) = (ctx.clone(), buf.clone(), stop.clone());
    std::thread::spawn(move || {
        let mut last_len = usize::MAX;
        let mut last_gen = 0u64;
        while !s2.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(400));
            let lines: Vec<LogLine> = b2.lock().unwrap().iter().cloned().collect();
            let gen = lines.last().map(|l| l.text.len() as u64).unwrap_or(0) ^ lines.len() as u64;
            if lines.len() == last_len && gen == last_gen {
                continue;
            }
            last_len = lines.len();
            last_gen = gen;
            c2.ui(move |ui| {
                let p = ui.global::<Player>();
                if p.get_logcat_paused() || p.get_panel() != "logcat" {
                    return;
                }
                let q = p.get_logcat_filter().to_lowercase();
                let min = p.get_logcat_level();
                let v: Vec<LogLine> = lines
                    .into_iter()
                    .filter(|l| level_rank(&l.level) >= min)
                    .filter(|l| q.is_empty() || l.tag.to_lowercase().contains(&q) || l.text.to_lowercase().contains(&q))
                    .rev()
                    .take(800)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                p.set_logcat(model(v));
            });
        }
    });
    while !stop.load(Ordering::Relaxed) {
        let serial = running.lock().unwrap().as_ref().map(Running::serial);
        let Some(serial) = serial else {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        };
        let _ = emuzip_adb::shell_stream(&serial, "logcat -v brief -T 300", |l| {
            if let Some(line) = parse_log(l) {
                let mut b = buf.lock().unwrap();
                b.push_back(line);
                while b.len() > 3000 {
                    b.pop_front();
                }
            }
            !stop.load(Ordering::Relaxed)
        });
        std::thread::sleep(Duration::from_secs(1));
    }
}

// ------------------------------------------------------------------ ferramentas

fn shell(ctx: &Ctx, cmd: String, ok: Option<String>) {
    let (serial, ..) = session!(ctx);
    let Some(serial) = serial else {
        ctx.toast_err(tr!("The device isn't ready yet"));
        return;
    };
    ctx.blocking(move |c| match emuzip_adb::shell(&serial, &cmd) {
        Ok(out) => {
            let t = out.trim();
            if t.contains("Exception") || t.contains("Error:") || t.contains("not allowed") || t.starts_with("Failure") {
                c.toast_err(t.lines().take(2).collect::<Vec<_>>().join(" "));
            } else if let Some(m) = ok {
                c.toast_ok(m);
            }
        }
        Err(e) => c.toast_err(format!("{e:#}")),
    });
}

fn console(ctx: &Ctx, cmd: String, ok: Option<String>) {
    let (_, port, ..) = session!(ctx);
    let Some(port) = port else { return };
    ctx.blocking(move |c| match emuzip_console::run(port, &cmd) {
        Ok(_) => {
            if let Some(m) = ok {
                c.toast_ok(m);
            }
        }
        Err(e) => c.toast_err(format!("{e:#}")),
    });
}

/// Executa uma chamada gRPC com o cliente da sessão.
fn grpc<F, Fut>(ctx: &Ctx, ok: Option<String>, f: F)
where
    F: FnOnce(Emulator) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), tonic::Status>> + Send,
{
    let (_, _, emu, _) = session!(ctx);
    let Some(emu) = emu else {
        ctx.toast_err(tr!("Display isn't connected yet"));
        return;
    };
    let c = ctx.clone();
    ctx.spawn(async move {
        match f(emu).await {
            Ok(_) => {
                if let Some(m) = ok {
                    c.toast_ok(m);
                }
            }
            Err(e) => c.toast_err(e.message().to_string()),
        }
    });
}

fn nav(ctx: &Ctx, what: &str) {
    let key = match what {
        "back" => "GoBack",
        "home" => "Home",
        "recents" => "AppSwitch",
        "power" => "Power",
        "vol-up" => "AudioVolumeUp",
        "vol-down" => "AudioVolumeDown",
        _ => return,
    };
    if let Some(s) = ctx.player.lock().unwrap().as_ref() {
        let _ = s.input.send(Input::Key(key.into()));
    }
}

fn key_name(t: &str) -> Option<String> {
    let k = match t {
        "\n" | "\r" => "Enter",
        "\u{8}" => "Backspace",
        "\u{7f}" => "Delete",
        "\t" => "Tab",
        "\u{1b}" => "Escape",
        "\u{F700}" => "ArrowUp",
        "\u{F701}" => "ArrowDown",
        "\u{F702}" => "ArrowLeft",
        "\u{F703}" => "ArrowRight",
        "\u{F729}" => "Home",
        "\u{F72B}" => "End",
        "\u{F72C}" => "PageUp",
        "\u{F72D}" => "PageDown",
        " " => " ",
        _ => {
            let c = t.chars().next()?;
            if t.chars().count() == 1 && !c.is_control() && !('\u{F700}'..='\u{F8FF}').contains(&c) && !('\u{10}'..='\u{18}').contains(&c) {
                return Some(t.to_string());
            }
            return None;
        }
    };
    Some(k.into())
}

fn pointer(ctx: &Ctx, xr: f32, yr: f32, down: bool) {
    if let Some(s) = ctx.player.lock().unwrap().as_ref() {
        let (dw, dh) = *s.dev_size.lock().unwrap();
        let (fw, fh) = *s.frame_size.lock().unwrap();
        // Em paisagem o frame vem rotacionado: troca os eixos.
        let (w, h) = if (fw > fh) != (dw > dh) { (dh, dw) } else { (dw, dh) };
        let x = (xr.clamp(0.0, 1.0) * w as f32) as i32;
        let y = (yr.clamp(0.0, 1.0) * h as f32) as i32;
        let _ = s.input.send(Input::Mouse(x, y, down));
    }
}

fn scroll(ctx: &Ctx, dy: f32) {
    let (serial, ..) = session!(ctx);
    let Some(serial) = serial else { return };
    let (w, h) = ctx.player.lock().unwrap().as_ref().map(|s| *s.dev_size.lock().unwrap()).unwrap_or((1080, 1920));
    let (cx, cy) = (w / 2, h / 2);
    let d = (dy * 4.0).clamp(-(h as f32) / 3.0, h as f32 / 3.0) as i32;
    std::thread::spawn(move || {
        let _ = emuzip_adb::shell(&serial, &format!("input swipe {cx} {cy} {cx} {} 120", cy as i32 + d));
    });
}

fn captures_dir(ctx: &Ctx) -> std::path::PathBuf {
    let d = ctx.settings().captures_dir();
    let _ = std::fs::create_dir_all(&d);
    d
}

fn stamp() -> String {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0).to_string()
}

fn screenshot(ctx: &Ctx) {
    let (_, _, emu, id) = session!(ctx);
    let Some(mut emu) = emu else { return };
    let dir = captures_dir(ctx);
    let c = ctx.clone();
    ctx.spawn(async move {
        let r = emu.ctl.get_screenshot(control::ImageFormat { format: ImgFormat::Png as i32, ..Default::default() }).await;
        match r {
            Ok(img) => {
                let path = dir.join(format!("{id}-{}.png", stamp()));
                match std::fs::write(&path, &img.into_inner().image) {
                    Ok(_) => {
                        let p2 = path.clone();
                        c.toast_with(
                            1,
                            tr!("Screenshot saved: {}", path.file_name().unwrap().to_string_lossy()),
                            tr!("Open"),
                            Some(Box::new(move |_| crate::devices::open_path(&p2))),
                        );
                    }
                    Err(e) => c.toast_err(e.to_string()),
                }
            }
            Err(e) => c.toast_err(e.message().to_string()),
        }
    });
}

fn toggle_record(ctx: &Ctx) {
    let (serial, _, _, id) = session!(ctx);
    let Some(serial) = serial else { return };
    let rec = ctx.player.lock().unwrap().as_ref().map(|s| s.record.clone()).unwrap();
    let started = *rec.lock().unwrap();
    if started.is_some() {
        // Parar: interrompe o screenrecord e baixa o vídeo.
        *rec.lock().unwrap() = None;
        let dir = captures_dir(ctx);
        let task = ctx.task(tr!("Saving recording"), None);
        ctx.blocking(move |c| {
            let _ = emuzip_adb::shell(&serial, "pkill -INT screenrecord");
            std::thread::sleep(Duration::from_millis(1500));
            let path = dir.join(format!("{id}-{}.mp4", stamp()));
            let r = emuzip_adb::pull(&serial, "/sdcard/emuzip_rec.mp4", &path);
            let _ = emuzip_adb::shell(&serial, "rm -f /sdcard/emuzip_rec.mp4");
            task.finish(&r, &tr!("Recording saved"));
            if r.is_ok() {
                let p2 = path.clone();
                c.toast_with(
                    1,
                    tr!("Video: {}", path.file_name().unwrap().to_string_lossy()),
                    tr!("Open"),
                    Some(Box::new(move |_| crate::devices::open_path(&p2))),
                );
            }
            c.ui(|ui| ui.global::<Player>().set_recording(false));
        });
        return;
    }
    *rec.lock().unwrap() = Some(Instant::now());
    ctx.ui(|ui| {
        let p = ui.global::<Player>();
        p.set_recording(true);
        p.set_record_time("0:00".into());
    });
    let s2 = serial.clone();
    std::thread::spawn(move || {
        let _ = emuzip_adb::shell(&s2, "screenrecord --time-limit 180 /sdcard/emuzip_rec.mp4");
    });
    let c = ctx.clone();
    ctx.spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let Some(t) = *rec.lock().unwrap() else { break };
            let s = t.elapsed().as_secs();
            c.ui(move |ui| ui.global::<Player>().set_record_time(format!("{}:{:02}", s / 60, s % 60).into()));
            if s >= 180 {
                break;
            }
        }
    });
}

fn install_apk(ctx: &Ctx) {
    let (serial, ..) = session!(ctx);
    let Some(serial) = serial else { return };
    let Some(apk) = rfd::FileDialog::new().add_filter("Android APK", &["apk"]).pick_file() else { return };
    let name = apk.file_name().unwrap().to_string_lossy().to_string();
    let task = ctx.task(tr!("Installing {}", name), None);
    ctx.blocking(move |c| {
        let r = emuzip_adb::install_with_progress(&serial, &apk, |d, t| {
            task.progress(d as f32 / t.max(1) as f32 * 0.8, tr!("Uploading {} of {}", emuzip_core::human_bytes(d), emuzip_core::human_bytes(t)));
            if d == t {
                task.progress(-1.0, tr!("Installing on Android…"));
            }
        });
        task.finish(&r, &tr!("{} installed", name));
        if r.is_ok() {
            load_packages(&c);
        }
    });
}

fn push_file(ctx: &Ctx, ui: &MainWindow) {
    let (serial, ..) = session!(ctx);
    let Some(serial) = serial else { return };
    let Some(files) = rfd::FileDialog::new().pick_files() else { return };
    let dir = ui.global::<Player>().get_remote_dir().to_string();
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let dest = format!("{}/{}", dir.trim_end_matches('/'), name);
        let task = ctx.task(tr!("Sending {}", name), None);
        let serial = serial.clone();
        ctx.blocking(move |_| {
            let r = emuzip_adb::push(&serial, &f, &dest, |d, t| {
                task.progress(d as f32 / t.max(1) as f32, tr!("{} of {}", emuzip_core::human_bytes(d), emuzip_core::human_bytes(t)));
            });
            if r.is_ok() {
                let _ = emuzip_adb::shell(&serial, &format!("am broadcast -a android.intent.action.MEDIA_SCANNER_SCAN_FILE -d 'file://{dest}'"));
            }
            task.finish(&r, &tr!("{} sent to {}", name, dest));
        });
    }
}

fn load_packages(ctx: &Ctx) {
    let (serial, ..) = session!(ctx);
    let Some(serial) = serial else { return };
    let c = ctx.clone();
    ctx.ui(move |ui| {
        let p = ui.global::<Player>();
        p.set_packages_loading(true);
        let all = p.get_show_system_apps();
        c.blocking(move |c| {
            let v = emuzip_adb::packages(&serial, !all).unwrap_or_default();
            c.ui(move |ui| {
                let p = ui.global::<Player>();
                p.set_packages(model(v.into_iter().map(SharedString::from).collect()));
                p.set_packages_loading(false);
            });
        });
    });
}

fn app_action(ctx: &Ctx, action: &str, pkg: &str) {
    let (cmd, ok) = match action {
        "open" => (format!("monkey -p {pkg} -c android.intent.category.LAUNCHER 1"), None),
        "stop" => (format!("am force-stop {pkg}"), Some(tr!("{} stopped", pkg))),
        "clear" => (format!("pm clear {pkg}"), Some(tr!("{} data cleared", pkg))),
        "uninstall" => {
            let (pkg, c2) = (pkg.to_string(), ctx.clone());
            ctx.confirm(
                &tr!("Uninstall {}?", pkg),
                tr!("The app and its data will be removed from the device."),
                true,
                Box::new(move |c| {
                    shell(c, format!("pm uninstall {pkg}"), Some(format!("{pkg} desinstalado")));
                    let c3 = c2.clone();
                    c.spawn(async move {
                        tokio::time::sleep(Duration::from_millis(800)).await;
                        load_packages(&c3);
                    });
                }),
            );
            return;
        }
        _ => return,
    };
    shell(ctx, cmd, ok);
}

fn load_snapshots(ctx: &Ctx) {
    let (_, port, _, id) = session!(ctx);
    let c = ctx.clone();
    ctx.blocking(move |_| {
        let mut names: Vec<String> = port
            .and_then(|p| emuzip_console::run(p, "avd snapshot list").ok())
            .map(|out| out.lines().skip_while(|l| !l.starts_with("ID")).skip(1).filter_map(|l| l.split_whitespace().nth(1).map(String::from)).collect())
            .unwrap_or_default();
        if names.is_empty() {
            names = AvdBackend::snapshots(&id);
        }
        names.retain(|n| n != "default_boot");
        c.ui(move |ui| ui.global::<Player>().set_snapshots(model(names.into_iter().map(SharedString::from).collect())));
    });
}

fn snapshot_action(ctx: &Ctx, action: &str, name: &str) {
    let name = name.trim().replace(' ', "_");
    if name.is_empty() {
        return;
    }
    let (_, port, ..) = session!(ctx);
    let Some(port) = port else { return };
    let (title, ok) = match action {
        "save" => (tr!("Saving snapshot {}", name), tr!("Snapshot {} saved", name)),
        "load" => (format!("Restaurando {name}"), format!("Snapshot {name} restaurado")),
        _ => (tr!("Deleting {}", name), tr!("Snapshot {} deleted", name)),
    };
    let cmd = format!("avd snapshot {action} {name}");
    let task = ctx.task(title, None);
    ctx.blocking(move |c| {
        let r = emuzip_console::run(port, &cmd);
        task.finish(&r, &ok);
        load_snapshots(&c);
    });
}

const CITIES: [(&str, f64, f64, f64); 9] = [
    ("sp", -23.5505, -46.6333, 760.0),
    ("rj", -22.9068, -43.1729, 5.0),
    ("bsb", -15.7939, -47.8828, 1172.0),
    ("nyc", 40.7128, -74.0060, 10.0),
    ("lon", 51.5072, -0.1276, 11.0),
    ("tyo", 35.6762, 139.6503, 40.0),
    ("lis", 38.7223, -9.1393, 2.0),
    ("syd", -33.8688, 151.2093, 3.0),
    ("mtv", 37.3861, -122.0839, 32.0),
];

fn set_gps(ctx: &Ctx, lat: f64, lon: f64, alt: f64, quiet: bool) {
    grpc(ctx, if quiet { None } else { Some(tr!("Location updated")) }, move |mut e| async move {
        e.ctl
            .set_gps(control::GpsState { passive_update: false, latitude: lat, longitude: lon, altitude: alt, satellites: 12, speed: 0.0, bearing: 0.0 })
            .await
            .map(|_| ())
    });
}

fn gps_route(ctx: &Ctx, ui: &MainWindow) {
    let Some(stop) = ctx.player.lock().unwrap().as_ref().map(|s| s.route_stop.clone()) else { return };
    let p = ui.global::<Player>();
    if p.get_route_running() {
        stop.store(true, Ordering::Relaxed);
        p.set_route_running(false);
        return;
    }
    stop.store(false, Ordering::Relaxed);
    p.set_route_running(true);
    let (lat0, lon0) = (p.get_lat().parse::<f64>().unwrap_or(-23.5505), p.get_lon().parse::<f64>().unwrap_or(-46.6333));
    let (lat1, lon1) = (-23.5614, -46.6559); // Av. Paulista
    let c = ctx.clone();
    ctx.spawn(async move {
        for i in 0..=60 {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let t = i as f64 / 60.0;
            let (la, lo) = (lat0 + (lat1 - lat0) * t, lon0 + (lon1 - lon0) * t);
            set_gps(&c, la, lo, 760.0, true);
            c.ui(move |ui| {
                let p = ui.global::<Player>();
                p.set_lat(format!("{la:.5}").into());
                p.set_lon(format!("{lo:.5}").into());
            });
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        c.ui(|ui| ui.global::<Player>().set_route_running(false));
        c.toast(0, tr!("Route completed"));
    });
}

fn apply_sensors(ctx: &Ctx, ui: &MainWindow) {
    use control::sensor_value::SensorType as T;
    let p = ui.global::<Player>();
    let vals: Vec<(T, Vec<f32>)> = vec![
        (T::Acceleration, vec![p.get_accel_x(), p.get_accel_y(), p.get_accel_z()]),
        (T::Light, vec![p.get_light()]),
        (T::Proximity, vec![p.get_proximity()]),
        (T::Temperature, vec![p.get_temperature()]),
        (T::Humidity, vec![p.get_humidity()]),
        (T::Pressure, vec![p.get_pressure()]),
    ];
    grpc(ctx, None, move |mut e| async move {
        for (t, data) in vals {
            e.ctl.set_sensor(control::SensorValue { target: t as i32, status: 0, value: Some(control::ParameterValue { data }) }).await?;
        }
        Ok(())
    });
}

fn random_hex16() -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
    let mut x = (n as u64) ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    format!("{x:016x}")
}

fn load_props(ctx: &Ctx) {
    let (serial, ..) = session!(ctx);
    let Some(serial) = serial else { return };
    ctx.blocking(move |c| {
        let out = emuzip_adb::shell(&serial, "getprop").unwrap_or_default();
        let keys = [
            ("ro.product.model", tr!("Model")),
            ("ro.product.manufacturer", tr!("Manufacturer")),
            ("ro.build.version.release", "Android".to_string()),
            ("ro.build.version.sdk", "API".to_string()),
            ("ro.build.version.security_patch", tr!("Security patch")),
            ("ro.product.cpu.abi", "ABI".to_string()),
            ("ro.build.fingerprint", "Fingerprint".to_string()),
            ("persist.sys.locale", tr!("Language")),
            ("gsm.operator.alpha", tr!("Carrier")),
            ("ro.serialno", "Serial".to_string()),
            ("ro.boot.qemu.avd_name", "AVD".to_string()),
        ];
        let map: std::collections::HashMap<String, String> = out
            .lines()
            .filter_map(|l| {
                let (k, v) = l.split_once("]: [")?;
                Some((k.trim_start_matches('[').to_string(), v.trim_end_matches(']').to_string()))
            })
            .collect();
        let aid = emuzip_adb::shell(&serial, "settings get secure android_id").unwrap_or_default().trim().to_string();
        let mut v: Vec<NamedValue> = vec![NamedValue { name: "Android ID".into(), value: aid.clone().into() }];
        v.extend(keys.iter().filter_map(|(k, n)| map.get(*k).map(|val| NamedValue { name: n.clone().into(), value: val.clone().into() })));
        c.ui(move |ui| {
            let p = ui.global::<Player>();
            p.set_props(model(v));
            if p.get_android_id().is_empty() {
                p.set_android_id(aid.into());
            }
        });
    });
}

/// Redimensiona a janela para o tamanho da tela do emulador (+ toolbar).
pub fn fit_window(ui: &MainWindow) {
    let p = ui.global::<Player>();
    let (fw, fh) = (p.get_screen_w().max(1) as f32, p.get_screen_h().max(1) as f32);
    let win = ui.window();
    let scale = win.scale_factor();
    let cur = win.size().to_logical(scale);
    // Altura atual limitada a 92% da altura típica de tela para caber no monitor.
    let h = cur.height.clamp(420.0, 1400.0);
    let w = ((h - 8.0) * fw / fh + 8.0 + 58.0).max(320.0);
    if p.get_fullscreen() {
        win.set_fullscreen(false);
        p.set_fullscreen(false);
    }
    p.set_panel("".into());
    p.set_panel_expanded(false);
    win.set_size(slint::LogicalSize::new(w.round(), h.round()));
}

pub fn wire(ctx: &Ctx, ui: &MainWindow) {
    let api = ui.global::<Api>();
    let w = ui.as_weak();
    api.on_fit_window(move || fit_window(&w.unwrap()));
    let w = ui.as_weak();
    api.on_toggle_fullscreen(move || {
        let ui = w.unwrap();
        let p = ui.global::<Player>();
        let fs = !p.get_fullscreen();
        p.set_fullscreen(fs);
        ui.window().set_fullscreen(fs);
    });
    let c = ctx.clone();
    api.on_open_player(move |id| open(&c, &id));
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_close_player(move || {
        close(&c);
        let ui = w.unwrap();
        if ui.global::<Player>().get_fullscreen() {
            ui.global::<Player>().set_fullscreen(false);
            ui.window().set_fullscreen(false);
        }
        ui.global::<Store>().set_page(0);
    });
    let c = ctx.clone();
    api.on_pointer(move |x, y, d| pointer(&c, x, y, d));
    let c = ctx.clone();
    api.on_key(move |t, down| {
        if !down {
            return;
        }
        if let (Some(k), Some(s)) = (key_name(&t), c.player.lock().unwrap().as_ref()) {
            let _ = s.input.send(Input::Key(k));
        }
    });
    let c = ctx.clone();
    api.on_text_input(move |t| {
        if let Some(s) = c.player.lock().unwrap().as_ref() {
            for ch in t.chars() {
                let _ = s.input.send(Input::Key(ch.to_string()));
            }
        }
    });
    let c = ctx.clone();
    api.on_scroll(move |dy| scroll(&c, dy));
    let c = ctx.clone();
    api.on_nav(move |w| nav(&c, &w));
    let c = ctx.clone();
    api.on_rotate(move |_| console(&c, "rotate".into(), None));
    let c = ctx.clone();
    api.on_screenshot(move || screenshot(&c));
    let c = ctx.clone();
    api.on_toggle_record(move || toggle_record(&c));
    let c = ctx.clone();
    api.on_install_apk(move || install_apk(&c));
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_push_file(move || push_file(&c, &w.unwrap()));

    // Bateria
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_apply_battery(move || {
        let ui = w.unwrap();
        let p = ui.global::<Player>();
        let st = control::BatteryState {
            has_battery: true,
            is_present: true,
            charger: p.get_charger_index(),
            charge_level: p.get_battery_level().round() as i32,
            health: p.get_battery_health_index(),
            status: p.get_battery_status_index(),
        };
        grpc(&c, None, move |mut e| async move { e.ctl.set_battery(st).await.map(|_| ()) });
    });
    // GPS
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_apply_gps(move || {
        let ui = w.unwrap();
        let p = ui.global::<Player>();
        match (p.get_lat().trim().parse::<f64>(), p.get_lon().trim().parse::<f64>()) {
            (Ok(la), Ok(lo)) => set_gps(&c, la, lo, p.get_alt().trim().parse().unwrap_or(0.0), false),
            _ => c.toast_err(tr!("Invalid latitude/longitude")),
        }
    });
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_gps_preset(move |k| {
        if let Some((_, la, lo, al)) = CITIES.iter().find(|x| x.0 == k.as_str()) {
            let ui = w.unwrap();
            let p = ui.global::<Player>();
            p.set_lat(format!("{la}").into());
            p.set_lon(format!("{lo}").into());
            p.set_alt(format!("{al}").into());
            set_gps(&c, *la, *lo, *al, false);
        }
    });
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_gps_route(move || gps_route(&c, &w.unwrap()));
    // Rede
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_apply_network(move || {
        let ui = w.unwrap();
        let p = ui.global::<Player>();
        let speed = ["full", "5g", "lte", "hsdpa", "umts", "edge", "gprs", "gsm"][p.get_net_speed_index().clamp(0, 7) as usize];
        let delay = ["none", "umts", "edge", "gprs", "gsm"][p.get_net_delay_index().clamp(0, 4) as usize];
        let sig = p.get_signal_index().clamp(0, 4);
        let (_, port, ..) = session!(c);
        let Some(port) = port else { return };
        c.blocking(move |c| {
            let r = emuzip_console::Console::connect(port).and_then(|mut con| {
                con.cmd(&format!("network speed {speed}"))?;
                con.cmd(&format!("network delay {delay}"))?;
                con.cmd(&format!("gsm signal-profile {sig}"))?;
                Ok(())
            });
            match r {
                Ok(_) => c.toast_ok(tr!("Network: {}, latency {}, signal {}/4", speed, delay, sig)),
                Err(e) => c.toast_err(format!("{e:#}")),
            }
        });
    });
    let c = ctx.clone();
    api.on_toggle_wifi(move |on| {
        shell(&c, format!("svc wifi {}", if on { "enable" } else { "disable" }), Some(if on { tr!("Wi-Fi on") } else { tr!("Wi-Fi off") }))
    });
    let c = ctx.clone();
    api.on_toggle_data(move |on| {
        shell(&c, format!("svc data {}", if on { "enable" } else { "disable" }), Some(if on { tr!("Mobile data on") } else { tr!("Mobile data off") }))
    });
    let c = ctx.clone();
    api.on_toggle_airplane(move |on| {
        shell(
            &c,
            format!("cmd connectivity airplane-mode {}", if on { "enable" } else { "disable" }),
            Some(if on { tr!("Airplane mode on") } else { tr!("Airplane mode off") }),
        )
    });
    // Telefone
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_phone_call(move |op| {
        use control::phone_call::Operation as O;
        let number = w.unwrap().global::<Player>().get_phone_number().to_string();
        let o = match op.as_str() {
            "call" => O::InitCall,
            "accept" => O::AcceptCall,
            "hold" => O::PlaceCallOnHold,
            "unhold" => O::TakeCallOffHold,
            "busy" => O::RejectCallBusy,
            _ => O::DisconnectCall,
        };
        grpc(&c, None, move |mut e| async move { e.ctl.send_phone(control::PhoneCall { operation: o as i32, number }).await.map(|_| ()) });
    });
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_send_sms(move || {
        let ui = w.unwrap();
        let p = ui.global::<Player>();
        let m = control::SmsMessage { src_address: p.get_phone_number().to_string(), text: p.get_sms_text().to_string() };
        grpc(&c, Some(tr!("SMS sent")), move |mut e| async move { e.ctl.send_sms(m).await.map(|_| ()) });
    });
    // Sensores e biometria
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_apply_sensors(move || apply_sensors(&c, &w.unwrap()));
    let c = ctx.clone();
    api.on_fingerprint(move |id| {
        grpc(&c, None, move |mut e| async move {
            e.ctl.send_fingerprint(control::Fingerprint { is_touching: true, touch_id: id }).await?;
            tokio::time::sleep(Duration::from_millis(450)).await;
            e.ctl.send_fingerprint(control::Fingerprint { is_touching: false, touch_id: id }).await.map(|_| ())
        });
    });
    // Identificadores
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_set_android_id(move || {
        let id = w.unwrap().global::<Player>().get_android_id().trim().to_lowercase();
        if id.len() != 16 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
            c.toast_err(tr!("Android ID must be 16 hex digits"));
            return;
        }
        shell(&c, format!("settings put secure android_id {id}"), Some(tr!("Android ID updated").into()));
    });
    let w = ui.as_weak();
    api.on_random_android_id(move || w.unwrap().global::<Player>().set_android_id(random_hex16().into()));
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_set_own_number(move || {
        let n = w.unwrap().global::<Player>().get_own_number().to_string();
        grpc(&c, Some(tr!("Number updated")), move |mut e| async move { e.ctl.set_phone_number(control::PhoneNumber { number: n }).await.map(|_| ()) });
    });
    let c = ctx.clone();
    api.on_load_props(move || load_props(&c));
    // Tela
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_apply_display(move || {
        let ui = w.unwrap();
        let p = ui.global::<Player>();
        let b = p.get_brightness().round() as u32;
        grpc(&c, None, move |mut e| async move { e.ctl.set_brightness(control::BrightnessValue { target: 0, value: b }).await.map(|_| ()) });
        let dens = p.get_density().trim().to_string();
        let cmd = format!(
            "cmd uimode night {}; settings put system show_touches {}; settings put system font_scale {:.2}; wm density {}",
            if p.get_night_mode() { "yes" } else { "no" },
            if p.get_show_touches() { 1 } else { 0 },
            p.get_font_scale(),
            if dens.is_empty() { "reset".to_string() } else { dens }
        );
        shell(&c, cmd, None);
    });
    // Câmera
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_apply_camera(move || {
        let ui = w.unwrap();
        let p = ui.global::<Player>();
        let back = ["virtualscene", "emulated", "webcam0", "none"][p.get_cam_back_index().clamp(0, 3) as usize];
        let front = ["emulated", "webcam0", "none"][p.get_cam_front_index().clamp(0, 2) as usize];
        let id = p.get_device_id().to_string();
        match AvdBackend::set_config(&id, &[("hw.camera.back", back.into()), ("hw.camera.front", front.into())]) {
            Ok(_) => c.toast_ok(tr!("Cameras saved: restart the device to apply")),
            Err(e) => c.toast_err(format!("{e:#}")),
        }
    });
    // Apps
    let c = ctx.clone();
    api.on_load_packages(move || load_packages(&c));
    let c = ctx.clone();
    api.on_app_action(move |a, p| app_action(&c, &a, &p));
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_open_link(move || {
        let url = w.unwrap().global::<Player>().get_deep_link().to_string().replace('\'', "");
        shell(&c, format!("am start -a android.intent.action.VIEW -d '{url}'"), None);
    });
    // Snapshots
    let c = ctx.clone();
    api.on_load_snapshots(move || load_snapshots(&c));
    let c = ctx.clone();
    api.on_snapshot_action(move |a, n| snapshot_action(&c, &a, &n));
    // Logcat
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_logcat_clear(move || {
        if let Some(s) = c.player.lock().unwrap().as_ref() {
            s.logcat.lock().unwrap().clear();
        }
        w.unwrap().global::<Player>().set_logcat(model(vec![]));
        shell(&c, "logcat -c".into(), None);
    });
    let c = ctx.clone();
    api.on_logcat_export(move || {
        let lines: Vec<LogLine> = c.player.lock().unwrap().as_ref().map(|s| s.logcat.lock().unwrap().iter().cloned().collect()).unwrap_or_default();
        let Some(dest) = rfd::FileDialog::new().set_file_name("logcat.txt").save_file() else { return };
        let text: String = lines.iter().map(|l| format!("{}/{}: {}\n", l.level, l.tag, l.text)).collect();
        match std::fs::write(&dest, text) {
            Ok(_) => c.toast_ok(tr!("{} lines exported", lines.len())),
            Err(e) => c.toast_err(e.to_string()),
        }
    });
    // Shell
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_run_shell(move || {
        let ui = w.unwrap();
        let p = ui.global::<Player>();
        let cmd = p.get_shell_cmd().to_string();
        if cmd.trim().is_empty() {
            return;
        }
        p.set_shell_cmd("".into());
        let (serial, ..) = session!(c);
        let Some(serial) = serial else { return };
        c.blocking(move |c| {
            let out = emuzip_adb::shell(&serial, &cmd).unwrap_or_else(|e| tr!("error: {}", format!("{:#}", e)));
            c.ui(move |ui| {
                let p = ui.global::<Player>();
                let mut all = format!("{}{}\n{}\n$ ", p.get_shell_output(), cmd, out.trim_end());
                if all.len() > 60_000 {
                    all = all[all.len() - 60_000..].to_string();
                }
                p.set_shell_output(all.into());
            });
        });
    });
    // Proxy e idioma
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_apply_proxy(move |on| {
        let v = if on { w.unwrap().global::<Player>().get_proxy().trim().to_string() } else { ":0".into() };
        // 10.0.2.2 é o computador: avisa se nada escuta na porta (o Android ficaria sem internet).
        if on {
            if let Some(port) = v.strip_prefix("10.0.2.2:").and_then(|p| p.parse::<u16>().ok()) {
                if std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)).is_err() {
                    c.toast_err(tr!("Nothing is listening on port {} of your computer: Android will have no internet until you disable the proxy.", port));
                }
            }
        }
        shell(&c, format!("settings put global http_proxy {v}"), Some(if on { format!("Proxy ativo: {v}") } else { "Proxy desativado".into() }));
    });
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_apply_locale(move || {
        let loc =
            ["pt-BR", "en-US", "es-ES", "fr-FR", "de-DE", "it-IT", "ja-JP", "zh-CN"][w.unwrap().global::<Player>().get_locale_index().clamp(0, 7) as usize];
        let (serial, ..) = session!(c);
        let Some(serial) = serial else { return };
        let task = c.task(tr!("Switching language to {}", loc), None);
        c.blocking(move |_| {
            // Sem root (imagens Play Store) usa o comando de sistema; com root reinicia a UI.
            let r = emuzip_adb::shell(
                &serial,
                &format!("cmd locale set-system-locales {loc} 2>&1 || (setprop persist.sys.locale {loc}; setprop ctl.restart zygote)"),
            );
            let r = r.and_then(|o| if o.contains("rror") || o.contains("denied") { Err(anyhow::anyhow!(o.trim().to_string())) } else { Ok(()) });
            task.finish(&r, &tr!("Language: {}", loc));
        });
    });
    // Clipboard
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_clipboard_send(move || {
        let t = w.unwrap().global::<Player>().get_clipboard().to_string();
        grpc(&c, Some(tr!("Text sent to Android clipboard")), move |mut e| async move { e.ctl.set_clipboard(control::ClipData { text: t }).await.map(|_| ()) });
    });
    let c = ctx.clone();
    api.on_clipboard_get(move || {
        let (_, _, emu, _) = session!(c);
        let Some(mut emu) = emu else { return };
        let c2 = c.clone();
        c.spawn(async move {
            match emu.ctl.get_clipboard(()).await {
                Ok(d) => {
                    let t = d.into_inner().text;
                    crate::prefs::copy_to_clipboard(&t);
                    c2.ui(move |ui| ui.global::<Player>().set_clipboard(t.into()));
                    c2.toast_ok(tr!("Android clipboard copied to computer"));
                }
                Err(e) => c2.toast_err(e.message().to_string()),
            }
        });
    });
}
