//! Android Emulator + AVDs, sem depender de avdmanager/sdkmanager (Java).
use crate::{Backend, CreateSpec, StartOpts};
use anyhow::{bail, Context, Result};
use emuzip_core::{android_label, android_sdk, avd_home, exe, home, ini, parse_api, BackendKind, Device, State};
use std::collections::HashMap;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct AvdBackend {
    pub sdk: PathBuf,
}

/// Emulador em execução descoberto pelos arquivos `avd/running/pid_*.ini`.
#[derive(Debug, Clone, Default)]
pub struct Running {
    pub pid: u32,
    pub avd: String,
    pub console_port: u16,
    pub grpc_port: Option<u16>,
    pub grpc_token: Option<String>,
}

impl Running {
    pub fn serial(&self) -> String {
        format!("emulator-{}", self.console_port)
    }
}

fn discovery_dirs() -> Vec<PathBuf> {
    let mut v = vec![home().join("Library/Caches/TemporaryItems/avd/running"), std::env::temp_dir().join("avd/running")];
    if let Some(x) = std::env::var_os("XDG_RUNTIME_DIR") {
        v.push(PathBuf::from(x).join("avd/running"));
    }
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        v.push(PathBuf::from(l).join("Temp/avd/running"));
    }
    v.push(home().join(".android/avd/running"));
    v
}

/// Lista emuladores rodando (inclusive iniciados fora do EmuZip).
pub fn running() -> Vec<Running> {
    let mut out: Vec<Running> = Vec::new();
    for d in discovery_dirs() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(pid) = name.strip_prefix("pid_").and_then(|s| s.strip_suffix(".ini")).and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            if !pid_alive(pid) {
                continue;
            }
            let i = ini::read(&e.path()).unwrap_or_default();
            let r = Running {
                pid,
                avd: i.get("cmdline").and_then(|c| avd_from_cmdline(c)).or_else(|| i.get("avd.name").cloned()).unwrap_or_default(),
                console_port: i.get("port.serial").and_then(|v| v.parse().ok()).unwrap_or(5554),
                grpc_port: i.get("grpc.port").and_then(|v| v.parse().ok()),
                grpc_token: i.get("grpc.token").cloned(),
            };
            if !out.iter().any(|o| o.pid == r.pid) {
                out.push(r);
            }
        }
    }
    out
}

/// Extrai o id do AVD de `"... "-avd" "Nome" ..."` (o avd.name pode ser o nome de exibição).
fn avd_from_cmdline(c: &str) -> Option<String> {
    let parts: Vec<&str> = c.split('"').map(str::trim).filter(|s| !s.is_empty()).collect();
    parts.iter().position(|p| *p == "-avd").and_then(|i| parts.get(i + 1)).map(|s| s.to_string())
}

pub fn pid_alive(pid: u32) -> bool {
    if cfg!(windows) {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}")])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    } else {
        Command::new("kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
    }
}

fn free_port(from: u16) -> u16 {
    (from..from + 200).find(|p| TcpListener::bind(("127.0.0.1", *p)).is_ok()).unwrap_or(from)
}

/// Par de portas console/adb livre (5554, 5556, ...).
fn free_console_port(taken: &[u16]) -> u16 {
    (5554..5682)
        .step_by(2)
        .find(|p| !taken.contains(p) && TcpListener::bind(("127.0.0.1", *p)).is_ok() && TcpListener::bind(("127.0.0.1", *p + 1)).is_ok())
        .unwrap_or(5554)
}

pub fn sanitize(name: &str) -> String {
    name.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

impl AvdBackend {
    pub fn detect() -> Option<Self> {
        let sdk = android_sdk()?;
        sdk.join("emulator").join(exe("emulator")).exists().then_some(Self { sdk })
    }

    pub fn emulator(&self) -> PathBuf {
        self.sdk.join("emulator").join(exe("emulator"))
    }

    pub fn adb(&self) -> PathBuf {
        self.sdk.join("platform-tools").join(exe("adb"))
    }

    /// Garante o servidor adb rodando (o cliente Rust fala com ele em :5037).
    pub fn ensure_adb_server(&self) {
        if emuzip_adb::devices().is_err() {
            let _ = Command::new(self.adb()).arg("start-server").stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
    }

    pub fn images(&self) -> Vec<emuzip_core::SystemImage> {
        emuzip_core::images::scan(&self.sdk)
    }

    fn avd_dir(id: &str) -> PathBuf {
        let i = ini::read(&avd_home().join(format!("{id}.ini"))).unwrap_or_default();
        i.get("path").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| avd_home().join(format!("{id}.avd")))
    }

    pub fn config(id: &str) -> ini::Ini {
        ini::read(&Self::avd_dir(id).join("config.ini")).unwrap_or_default()
    }

    pub fn set_config(id: &str, changes: &[(&str, String)]) -> Result<()> {
        let path = Self::avd_dir(id).join("config.ini");
        let mut c = ini::read(&path).context("config.ini not found")?;
        for (k, v) in changes {
            c.insert((*k).to_string(), v.clone());
        }
        ini::write(&path, &c)?;
        Ok(())
    }

    fn device_from(&self, id: &str, run: &HashMap<String, Running>) -> Device {
        let dir = Self::avd_dir(id);
        let top = ini::read(&avd_home().join(format!("{id}.ini"))).unwrap_or_default();
        let cfg = ini::read(&dir.join("config.ini"));
        let mut problem = None;
        if !dir.exists() {
            problem = Some(format!("Folder {} does not exist", dir.display()));
        } else if cfg.is_none() {
            problem = Some("config.ini ausente".into());
        }
        let cfg = cfg.unwrap_or_default();
        let sysdir = cfg.get("image.sysdir.1").cloned().unwrap_or_default();
        if problem.is_none() && !sysdir.is_empty() && !self.sdk.join(&sysdir).exists() {
            problem = Some(format!("Image {sysdir} is not installed"));
        }
        let target =
            cfg.get("target").or(top.get("target")).cloned().unwrap_or_else(|| sysdir.split('/').find(|p| p.starts_with("android-")).unwrap_or("").to_string());
        let num = |k: &str| cfg.get(k).and_then(|v| v.trim_end_matches("MB").trim().parse::<u32>().ok()).unwrap_or(0);
        let r = run.get(id);
        let state = match (&problem, r) {
            (_, Some(r)) => {
                let booted = emuzip_adb::shell(&r.serial(), "getprop sys.boot_completed").map(|s| s.trim() == "1").unwrap_or(false);
                if booted {
                    State::Running
                } else {
                    State::Booting
                }
            }
            (Some(_), None) => State::Broken,
            (None, None) => State::Stopped,
        };
        Device {
            id: id.into(),
            name: cfg.get("avd.ini.displayname").cloned().unwrap_or_else(|| id.replace('_', " ")),
            backend: BackendKind::Avd,
            android: android_label(&target),
            api: parse_api(&target),
            state,
            serial: r.map(Running::serial),
            grpc_port: r.and_then(|r| r.grpc_port),
            width: num("hw.lcd.width"),
            height: num("hw.lcd.height"),
            density: num("hw.lcd.density"),
            ram_mb: num("hw.ramSize"),
            cpus: num("hw.cpu.ncore").max(1),
            tag: cfg.get("tag.display").cloned().unwrap_or_default(),
            path: dir,
            problem,
        }
    }

    pub fn find_running(id: &str) -> Option<Running> {
        running().into_iter().find(|r| r.avd == id)
    }

    /// Clona um AVD (copia a pasta e ajusta caminhos).
    pub fn clone_avd(&self, id: &str, new_name: &str) -> Result<String> {
        let new_id = sanitize(new_name);
        let src = Self::avd_dir(id);
        let dst = avd_home().join(format!("{new_id}.avd"));
        if dst.exists() {
            bail!("a device named {new_id} already exists");
        }
        copy_dir(&src, &dst, &["multiinstance.lock", "hardware-qemu.ini.lock", "snapshots"])?;
        let mut top = ini::read(&avd_home().join(format!("{id}.ini"))).unwrap_or_default();
        top.insert("path".into(), dst.display().to_string());
        top.insert("path.rel".into(), format!("avd/{new_id}.avd"));
        ini::write(&avd_home().join(format!("{new_id}.ini")), &top)?;
        Self::set_config(&new_id, &[("avd.ini.displayname", new_name.into()), ("AvdId", new_id.clone())])?;
        Ok(new_id)
    }

    pub fn rename(&self, id: &str, display: &str) -> Result<()> {
        Self::set_config(id, &[("avd.ini.displayname", display.into())])
    }

    /// Factory reset: apaga os dados do usuário e snapshots.
    pub fn wipe(&self, id: &str) -> Result<()> {
        let dir = Self::avd_dir(id);
        for f in ["userdata-qemu.img", "userdata-qemu.img.qcow2", "cache.img", "cache.img.qcow2", "sdcard.img.qcow2", "snapshots"] {
            let p = dir.join(f);
            if p.is_dir() {
                let _ = std::fs::remove_dir_all(&p);
            } else {
                let _ = std::fs::remove_file(&p);
            }
        }
        Ok(())
    }

    /// Repara um AVD quebrado. Sem pasta/config, remove o registro órfão.
    pub fn repair(&self, id: &str) -> Result<String> {
        let dir = Self::avd_dir(id);
        if !dir.join("config.ini").exists() {
            let _ = std::fs::remove_file(avd_home().join(format!("{id}.ini")));
            return Ok("Orphan entry removed".into());
        }
        let _ = std::fs::remove_file(dir.join("multiinstance.lock"));
        let _ = std::fs::remove_file(dir.join("hardware-qemu.ini.lock"));
        Ok("Locks removidos".into())
    }

    pub fn snapshots(id: &str) -> Vec<String> {
        std::fs::read_dir(Self::avd_dir(id).join("snapshots"))
            .map(|r| r.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default()
    }

    pub fn open_folder(id: &str) {
        let dir = Self::avd_dir(id);
        let prog = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(windows) {
            "explorer"
        } else {
            "xdg-open"
        };
        let _ = Command::new(prog).arg(dir).spawn();
    }

    /// Gera o config.ini de um novo AVD.
    pub fn build_config(&self, spec: &CreateSpec, img: &emuzip_core::SystemImage) -> ini::Ini {
        let yn = |b: bool| if b { "yes" } else { "no" }.to_string();
        let mut c = ini::Ini::new();
        let mut set = |k: &str, v: String| {
            c.insert(k.into(), v);
        };
        set("avd.ini.encoding", "UTF-8".into());
        set("avd.ini.displayname", spec.name.clone());
        set("AvdId", sanitize(&spec.name));
        set("PlayStore.enabled", yn(img.play_store()));
        set("abi.type", img.abi.clone());
        set("hw.cpu.arch", if img.abi.starts_with("arm64") { "arm64".into() } else { img.abi.replace("x86_64", "x86_64") });
        set("image.sysdir.1", img.sysdir());
        set("tag.id", img.tag.clone());
        set("tag.display", img.tag_display.clone());
        set("target", img.api_dir.clone());
        set("hw.device.name", spec.profile.clone());
        set("hw.lcd.width", spec.width.to_string());
        set("hw.lcd.height", spec.height.to_string());
        set("hw.lcd.density", spec.density.to_string());
        set("hw.ramSize", spec.ram_mb.to_string());
        set("hw.cpu.ncore", spec.cpus.to_string());
        set("disk.dataPartition.size", format!("{}G", spec.storage_gb));
        set("vm.heapSize", "256".into());
        set("hw.gpu.enabled", "yes".into());
        set("hw.gpu.mode", if spec.gpu.is_empty() { "auto".into() } else { spec.gpu.clone() });
        set("hw.keyboard", yn(spec.keyboard));
        set("hw.mainKeys", "no".into());
        set("hw.camera.back", spec.camera_back.clone());
        set("hw.camera.front", spec.camera_front.clone());
        set("hw.gps", "yes".into());
        set("hw.battery", "yes".into());
        set("hw.accelerometer", "yes".into());
        set("hw.gyroscope", "yes".into());
        set("hw.sensors.proximity", "yes".into());
        set("hw.sensors.light", "yes".into());
        set("hw.sensors.pressure", "yes".into());
        set("hw.sensors.humidity", "yes".into());
        set("hw.sensors.temperature", "yes".into());
        set("hw.sensors.magnetic_field", "yes".into());
        set("hw.sensors.orientation", "yes".into());
        set("hw.audioInput", "yes".into());
        set("hw.audioOutput", "yes".into());
        set("hw.sdCard", "yes".into());
        set("sdcard.size", "512M".into());
        set("hw.initialOrientation", if spec.width > spec.height { "landscape" } else { "portrait" }.into());
        set("showDeviceFrame", "no".into());
        set("fastboot.forceColdBoot", "no".into());
        set("fastboot.forceFastBoot", "yes".into());
        set("firstboot.saveToLocalSnapshot", "yes".into());
        set("hw.dPad", yn(spec.profile.starts_with("tv")));
        set("hw.lcd.circular", yn(spec.profile.contains("round")));
        set("hw.navigation.buttons", yn(spec.nav_buttons));
        c
    }
}

fn copy_dir(src: &Path, dst: &Path, skip: &[&str]) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)?.flatten() {
        let name = e.file_name();
        if skip.iter().any(|s| name.to_string_lossy() == *s) {
            continue;
        }
        let (s, d) = (e.path(), dst.join(&name));
        if s.is_dir() {
            copy_dir(&s, &d, skip)?;
        } else {
            std::fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

impl Backend for AvdBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Avd
    }

    fn list(&self) -> Result<Vec<Device>> {
        self.ensure_adb_server();
        let run: HashMap<String, Running> = running().into_iter().map(|r| (r.avd.clone(), r)).collect();
        let mut ids: Vec<String> = std::fs::read_dir(avd_home())
            .map(|r| r.flatten().filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".ini").map(String::from)).collect())
            .unwrap_or_default();
        ids.sort_by_key(|s| s.to_lowercase());
        Ok(ids.iter().map(|id| self.device_from(id, &run)).collect())
    }

    fn create(&self, spec: &CreateSpec) -> Result<String> {
        let id = sanitize(&spec.name);
        if id.is_empty() {
            bail!("nome inválido");
        }
        let img = self.images().into_iter().find(|i| i.id == spec.image).with_context(|| format!("image {} is not installed", spec.image))?;
        let dir = avd_home().join(format!("{id}.avd"));
        if dir.exists() || avd_home().join(format!("{id}.ini")).exists() {
            bail!("a device named {id} already exists");
        }
        std::fs::create_dir_all(&dir)?;
        let mut top = ini::Ini::new();
        top.insert("avd.ini.encoding".into(), "UTF-8".into());
        top.insert("path".into(), dir.display().to_string());
        top.insert("path.rel".into(), format!("avd/{id}.avd"));
        top.insert("target".into(), img.api_dir.clone());
        ini::write(&avd_home().join(format!("{id}.ini")), &top)?;
        ini::write(&dir.join("config.ini"), &self.build_config(spec, &img))?;
        Ok(id)
    }

    fn start(&self, id: &str, o: &StartOpts) -> Result<()> {
        if Self::find_running(id).is_some() {
            return Ok(());
        }
        self.ensure_adb_server();
        let taken: Vec<u16> = running().iter().map(|r| r.console_port).collect();
        let console = free_console_port(&taken);
        let grpc = free_port(8554 + (console - 5554) / 2);
        let mut cmd = Command::new(self.emulator());
        cmd.args(["-avd", id, "-port", &console.to_string(), "-grpc", &grpc.to_string()]);
        if o.headless {
            cmd.arg("-no-window");
        }
        if o.cold_boot {
            cmd.arg("-no-snapshot-load");
        }
        if o.wipe {
            cmd.arg("-wipe-data");
        }
        if !o.audio {
            cmd.arg("-no-audio");
        }
        if !o.gpu.is_empty() && o.gpu != "auto" {
            cmd.args(["-gpu", &o.gpu]);
        }
        let log = emuzip_core::data_dir().join("logs");
        std::fs::create_dir_all(&log)?;
        let f = std::fs::File::create(log.join(format!("{id}.log")))?;
        cmd.stdin(Stdio::null()).stdout(f.try_clone()?).stderr(f).spawn().context("starting emulator")?;
        Ok(())
    }

    fn stop(&self, d: &Device) -> Result<()> {
        if let Some(r) = Self::find_running(&d.id) {
            if emuzip_adb::kill_emulator(&r.serial()).is_err() {
                let _ = Command::new(if cfg!(windows) { "taskkill" } else { "kill" })
                    .args(if cfg!(windows) { vec!["/PID".to_string(), r.pid.to_string(), "/F".into()] } else { vec![r.pid.to_string()] })
                    .status();
            }
        }
        Ok(())
    }

    fn delete(&self, d: &Device) -> Result<()> {
        if Self::find_running(&d.id).is_some() {
            bail!("stop the device before deleting it");
        }
        let dir = Self::avd_dir(&d.id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        let _ = std::fs::remove_file(avd_home().join(format!("{}.ini", d.id)));
        Ok(())
    }
}

/// Log do emulador iniciado pelo EmuZip (para diagnóstico).
pub fn log_path(id: &str) -> PathBuf {
    emuzip_core::data_dir().join("logs").join(format!("{id}.log"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cmdline_avd() {
        assert_eq!(avd_from_cmdline(r#""/x/qemu" "-avd" "EmuZip_Teste" "-port" "5554""#).as_deref(), Some("EmuZip_Teste"));
    }
    #[test]
    fn sanitize_names() {
        assert_eq!(sanitize(" Meu Pixel 9! "), "Meu_Pixel_9_");
    }
    #[test]
    fn config_from_spec() {
        let b = AvdBackend { sdk: "/sdk".into() };
        let img = emuzip_core::SystemImage {
            id: "android-34/google_apis_playstore/x86_64".into(),
            api: 34,
            api_dir: "android-34".into(),
            tag: "google_apis_playstore".into(),
            tag_display: "Google Play".into(),
            abi: "x86_64".into(),
            path: "/sdk/x".into(),
        };
        let spec = CreateSpec {
            name: "Teste".into(),
            image: img.id.clone(),
            profile: "pixel_9".into(),
            width: 1080,
            height: 2424,
            density: 420,
            ram_mb: 4096,
            cpus: 4,
            storage_gb: 8,
            gpu: "host".into(),
            camera_back: "emulated".into(),
            camera_front: "none".into(),
            keyboard: true,
            nav_buttons: false,
        };
        let c = b.build_config(&spec, &img);
        assert_eq!(c["image.sysdir.1"], "system-images/android-34/google_apis_playstore/x86_64/");
        assert_eq!(c["PlayStore.enabled"], "yes");
        assert_eq!(c["hw.lcd.density"], "420");
        assert_eq!(c["disk.dataPartition.size"], "8G");
    }
}
