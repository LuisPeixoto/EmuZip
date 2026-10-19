//! Configurações, tema, ambiente e diagnóstico.
use crate::ctx::Ctx;
use crate::{Api, MainWindow, Prefs, Store, Theme};
use emuzip_core::{data_dir, detect_android_sdk, human_bytes, profiles, Settings};
use slint::ComponentHandle;
use std::io::Write;
use std::path::PathBuf;

const GPU: [&str; 4] = ["auto", "host", "swiftshader_indirect", "angle_indirect"];
const FPS: [u32; 4] = [15, 24, 30, 60];

fn path_str(p: &Option<PathBuf>) -> String {
    p.as_ref().map(|p| p.display().to_string()).unwrap_or_default()
}

fn opt_path(s: &str) -> Option<PathBuf> {
    let s = s.trim();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

pub fn load_into_ui(ctx: &Ctx, ui: &MainWindow) {
    let s = ctx.settings();
    let p = ui.global::<Prefs>();
    p.set_sdk_path(path_str(&s.sdk_path).into());
    p.set_avd_path(path_str(&s.avd_path).into());
    p.set_qemu_path(path_str(&s.qemu_path).into());
    p.set_captures_path(path_str(&s.captures_dir).into());
    p.set_theme_index(match s.theme.as_str() {
        "dark" => 1,
        "system" => 2,
        _ => 0,
    });
    p.set_embedded(s.embedded_display);
    p.set_fps_index(FPS.iter().position(|f| *f == s.stream_fps).unwrap_or(2) as i32);
    p.set_gpu_index(GPU.iter().position(|g| *g == s.gpu_mode).unwrap_or(0) as i32);
    p.set_cold_boot(s.cold_boot);
    p.set_audio(s.audio);
    p.set_profile_index(profiles::presets().iter().position(|x| x.id == s.default_profile).unwrap_or(0) as i32);
    p.set_ram(s.default_ram_mb as i32);
    p.set_cpus(s.default_cpus as i32);
    p.set_proxy(s.download_proxy.clone().unwrap_or_default().into());
    p.set_mirror(s.repo_mirror.clone().unwrap_or_default().into());
    p.set_confirm_destructive(s.confirm_destructive);
    p.set_language_index(crate::i18n::LANGUAGES.iter().position(|l| l.0 == s.language).unwrap_or(0) as i32);
    p.set_auto_fit(s.auto_fit_window);
    p.set_dirty(false);
    apply_theme(ui, p.get_theme_index());
    validate(ui);
    p.set_cache_size(format!("{} em {}", human_bytes(dir_size(&data_dir().join("downloads"))), data_dir().join("downloads").display()).into());
}

fn from_ui(ui: &MainWindow) -> Settings {
    let p = ui.global::<Prefs>();
    Settings {
        sdk_path: opt_path(&p.get_sdk_path()),
        avd_path: opt_path(&p.get_avd_path()),
        qemu_path: opt_path(&p.get_qemu_path()),
        captures_dir: opt_path(&p.get_captures_path()),
        theme: ["light", "dark", "system"][p.get_theme_index().clamp(0, 2) as usize].into(),
        embedded_display: p.get_embedded(),
        stream_fps: FPS[p.get_fps_index().clamp(0, 3) as usize],
        default_profile: profiles::presets().get(p.get_profile_index().max(0) as usize).map(|x| x.id.clone()).unwrap_or_else(|| "pixel_9".into()),
        default_ram_mb: p.get_ram() as u32,
        default_cpus: p.get_cpus() as u32,
        gpu_mode: GPU[p.get_gpu_index().clamp(0, 3) as usize].into(),
        cold_boot: p.get_cold_boot(),
        audio: p.get_audio(),
        download_proxy: Some(p.get_proxy().to_string()).filter(|s| !s.trim().is_empty()),
        repo_mirror: Some(p.get_mirror().to_string()).filter(|s| !s.trim().is_empty()),
        confirm_destructive: p.get_confirm_destructive(),
        language: crate::i18n::LANGUAGES.get(p.get_language_index().max(0) as usize).map(|l| l.0).unwrap_or("system").into(),
        auto_fit_window: p.get_auto_fit(),
    }
}

pub fn validate(ui: &MainWindow) {
    let p = ui.global::<Prefs>();
    let sdk = opt_path(&p.get_sdk_path()).or_else(detect_android_sdk);
    p.set_sdk_valid(sdk.map(|s| s.join("emulator").exists()).unwrap_or(false));
    let avd = opt_path(&p.get_avd_path()).unwrap_or_else(|| emuzip_core::home().join(".android/avd"));
    p.set_avd_valid(avd.exists());
    let q = opt_path(&p.get_qemu_path());
    p.set_qemu_valid(match q {
        Some(d) => std::fs::read_dir(d).map(|r| r.flatten().any(|e| e.file_name().to_string_lossy().starts_with("qemu-system"))).unwrap_or(false),
        None => emuzip_backend::qemu::QemuBackend::detect().is_some(),
    });
}

pub fn system_dark() -> bool {
    if cfg!(target_os = "macos") {
        std::process::Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("Dark"))
            .unwrap_or(false)
    } else {
        false
    }
}

pub fn apply_theme(ui: &MainWindow, i: i32) {
    let dark = i == 1 || (i == 2 && system_dark());
    ui.global::<Theme>().set_dark(dark);
    ui.invoke_apply_scheme();
}

fn save(ctx: &Ctx, ui: &MainWindow) {
    let s = from_ui(ui);
    match s.save() {
        Ok(_) => {
            *ctx.settings.write().unwrap() = s;
            ui.global::<Prefs>().set_dirty(false);
            ctx.toast_ok(tr!("Settings saved"));
            refresh_environment(ctx);
        }
        Err(e) => ctx.toast_err(tr!("Couldn't save: {}", format!("{:#}", e))),
    }
}

/// Recarrega backends e o status do ambiente (SDK, aceleração).
pub fn refresh_environment(ctx: &Ctx) {
    ctx.blocking(|c| {
        *c.backends.write().unwrap() = std::sync::Arc::new(emuzip_backend::detect());
        let names: Vec<&str> = c.backends().iter().map(|b| b.kind().label()).collect();
        let backends = if names.is_empty() { tr!("No backend") } else { names.join(" + ") };
        let sdk = emuzip_core::android_sdk();
        let (accel_ok, accel) = accel_status(sdk.as_deref());
        crate::images::publish_installed(&c);
        crate::devices::reload(&c);
        c.ui(move |ui| {
            let s = ui.global::<Store>();
            s.set_backends(backends.into());
            s.set_sdk_ok(sdk.is_some());
            s.set_sdk_status(sdk.map(|p| p.display().to_string()).unwrap_or_else(|| tr!("not found — set the path in Settings").into()).into());
            s.set_accel_ok(accel_ok);
            s.set_accel_status(accel.into());
        });
    });
}

fn accel_status(sdk: Option<&std::path::Path>) -> (bool, String) {
    if cfg!(target_os = "macos") {
        let hv = std::process::Command::new("sysctl")
            .args(["-n", "kern.hv_support"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
            .unwrap_or(false);
        return (hv, if hv { tr!("Hypervisor.framework active").into() } else { tr!("No Hypervisor.framework").into() });
    }
    if cfg!(target_os = "linux") {
        let kvm = std::path::Path::new("/dev/kvm").exists();
        return (kvm, if kvm { tr!("KVM available").into() } else { tr!("KVM unavailable (/dev/kvm)").into() });
    }
    if let Some(s) = sdk {
        let out = std::process::Command::new(s.join("emulator").join(emuzip_core::exe("emulator"))).arg("-accel-check").output();
        if let Ok(o) = out {
            let t = String::from_utf8_lossy(&o.stdout).to_string();
            let ok = t.lines().next().map(|l| l.trim() == "0").unwrap_or(false);
            return (ok, t.lines().nth(1).unwrap_or("WHPX/AEHD").trim().to_string());
        }
    }
    (false, tr!("Unknown acceleration").into())
}

fn dir_size(p: &std::path::Path) -> u64 {
    std::fs::read_dir(p)
        .map(|r| r.flatten().map(|e| if e.path().is_dir() { dir_size(&e.path()) } else { e.metadata().map(|m| m.len()).unwrap_or(0) }).sum())
        .unwrap_or(0)
}

fn export_diagnostics(ctx: &Ctx) {
    let Some(dest) = rfd::FileDialog::new().set_file_name("emuzip-diagnostico.zip").add_filter("ZIP", &["zip"]).save_file() else { return };
    let task = ctx.task(tr!("Exporting diagnostics"), None);
    ctx.blocking(move |c| {
        let r = (|| -> anyhow::Result<()> {
            let f = std::fs::File::create(&dest)?;
            let mut z = zip::ZipWriter::new(f);
            let opt: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
            let mut info = format!("EmuZip {}\nOS: {} {}\n", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
            if let Some(a) = c.avd() {
                info += &format!("SDK: {}\n", a.sdk.display());
                if let Ok(o) = std::process::Command::new(a.emulator()).arg("-version").output() {
                    info += &String::from_utf8_lossy(&o.stdout);
                }
                info += "\nImagens:\n";
                for i in a.images() {
                    info += &format!("  {}\n", i.id);
                }
            }
            info += "\nDispositivos:\n";
            for d in c.devices.lock().unwrap().iter() {
                info += &format!("  {} [{:?}] {} {}\n", d.id, d.state, d.android, d.problem.clone().unwrap_or_default());
            }
            z.start_file("info.txt", opt)?;
            z.write_all(info.as_bytes())?;
            if let Ok(s) = std::fs::read(Settings::path()) {
                z.start_file("settings.toml", opt)?;
                z.write_all(&s)?;
            }
            if let Ok(rd) = std::fs::read_dir(data_dir().join("logs")) {
                for e in rd.flatten() {
                    z.start_file(format!("logs/{}", e.file_name().to_string_lossy()), opt)?;
                    z.write_all(&std::fs::read(e.path())?)?;
                }
            }
            z.finish()?;
            Ok(())
        })();
        task.finish(&r, &tr!("Diagnostics exported"));
        if r.is_ok() {
            let d = dest.clone();
            c.toast_with(1, tr!("Saved to {}", dest.display()), tr!("Show"), Some(Box::new(move |_| crate::devices::open_path(d.parent().unwrap()))));
        }
    });
}

pub fn wire(ctx: &Ctx, ui: &MainWindow) {
    let api = ui.global::<Api>();
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_save_prefs(move || save(&c, &w.unwrap()));
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_reset_prefs(move || {
        *c.settings.write().unwrap() = Settings::default();
        let ui = w.unwrap();
        load_into_ui(&c, &ui);
        ui.global::<Prefs>().set_dirty(true);
    });
    let w = ui.as_weak();
    api.on_validate_paths(move || validate(&w.unwrap()));
    let w = ui.as_weak();
    api.on_set_theme(move |i| apply_theme(&w.unwrap(), i));
    let w = ui.as_weak();
    api.on_set_language(move |i| {
        let code = crate::i18n::LANGUAGES.get(i.max(0) as usize).map(|l| l.0).unwrap_or("system");
        crate::i18n::set_language(code);
        refresh_language_names(&w.unwrap());
    });
    let w = ui.as_weak();
    api.on_browse(move |key| {
        let ui = w.unwrap();
        let Some(dir) = rfd::FileDialog::new().pick_folder() else { return };
        let v = dir.display().to_string().into();
        let p = ui.global::<Prefs>();
        match key.as_str() {
            "sdk" => p.set_sdk_path(v),
            "avd" => p.set_avd_path(v),
            "qemu" => p.set_qemu_path(v),
            _ => p.set_captures_path(v),
        }
        p.set_dirty(true);
        validate(&ui);
    });
    let c = ctx.clone();
    api.on_export_diagnostics(move || export_diagnostics(&c));
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_clear_cache(move || {
        let _ = std::fs::remove_dir_all(data_dir().join("downloads"));
        c.toast_ok(tr!("Download cache cleared"));
        w.unwrap().global::<Prefs>().set_cache_size("0 B".into());
    });
    let c = ctx.clone();
    api.on_copy_text(move |t| {
        copy_to_clipboard(&t);
        c.toast(0, tr!("Copied to clipboard"));
    });
}

pub fn copy_to_clipboard(t: &str) {
    let cmd = if cfg!(target_os = "macos") {
        Some(("pbcopy", vec![]))
    } else if cfg!(windows) {
        Some(("clip", vec![]))
    } else {
        Some(("xclip", vec!["-selection", "clipboard"]))
    };
    if let Some((p, a)) = cmd {
        if let Ok(mut ch) = std::process::Command::new(p).args(a).stdin(std::process::Stdio::piped()).spawn() {
            if let Some(mut i) = ch.stdin.take() {
                let _ = i.write_all(t.as_bytes());
            }
            let _ = ch.wait();
        }
    }
}

/// Nomes dos idiomas para o seletor ("System" é traduzido; os demais no próprio idioma).
pub fn refresh_language_names(ui: &MainWindow) {
    let names: Vec<slint::SharedString> =
        crate::i18n::LANGUAGES.iter().map(|l| if l.0 == "system" { tr!("System (automatic)").into() } else { l.2.into() }).collect();
    let idx = ui.global::<Prefs>().get_language_index();
    ui.global::<Store>().set_language_names(crate::ctx::model(names));
    ui.global::<Prefs>().set_language_index(idx);
}
