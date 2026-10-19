//! Tipos de domínio, configuração e utilitários do EmuZip.
pub mod images;
pub mod ini;
pub mod profiles;
pub mod settings;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use images::SystemImage;
pub use profiles::HwProfile;
pub use settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendKind {
    Avd,
    Qemu,
}

impl BackendKind {
    pub fn label(self) -> &'static str {
        match self {
            BackendKind::Avd => "Android Emulator",
            BackendKind::Qemu => "QEMU",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Stopped,
    Booting,
    Running,
    /// Configuração do AVD incompleta (ex.: pasta .avd ausente).
    Broken,
}

#[derive(Debug, Clone)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub backend: BackendKind,
    pub android: String,
    pub api: u32,
    pub state: State,
    /// Serial do adb (ex.: emulator-5554) quando em execução.
    pub serial: Option<String>,
    /// Porta gRPC quando iniciado pelo EmuZip.
    pub grpc_port: Option<u16>,
    pub width: u32,
    pub height: u32,
    pub density: u32,
    pub ram_mb: u32,
    pub cpus: u32,
    pub tag: String,
    pub path: PathBuf,
    pub problem: Option<String>,
}

impl Device {
    pub fn summary(&self) -> String {
        if self.width > 0 {
            format!("{}×{} · {} dpi · {} MB RAM · {} CPU", self.width, self.height, self.density, self.ram_mb, self.cpus)
        } else {
            String::new()
        }
    }
}

/// Perfil de um dispositivo QEMU criado pelo EmuZip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QemuProfile {
    pub name: String,
    pub disk: PathBuf,
    pub arch: String,
    pub cpus: u32,
    pub ram_mb: u32,
}

pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("br", "vidyacode", "EmuZip").map(|d| d.data_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from(".emuzip"))
}

pub fn home() -> PathBuf {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()).unwrap_or_default()
}

/// Localiza o Android SDK (configuração do usuário tem prioridade).
pub fn android_sdk() -> Option<PathBuf> {
    if let Some(p) = Settings::load().sdk_path.filter(|p| p.exists()) {
        return Some(p);
    }
    detect_android_sdk()
}

pub fn detect_android_sdk() -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = ["ANDROID_HOME", "ANDROID_SDK_ROOT"].iter().filter_map(|k| std::env::var_os(k).map(PathBuf::from)).collect();
    if let Some(h) = directories::BaseDirs::new() {
        let home = h.home_dir();
        cands.push(home.join("Library/Android/sdk"));
        cands.push(home.join("Android/Sdk"));
        cands.push(h.data_local_dir().join("Android/Sdk"));
    }
    cands.into_iter().find(|p| p.join("platform-tools").exists() || p.join("emulator").exists())
}

/// Pasta dos AVDs.
pub fn avd_home() -> PathBuf {
    if let Some(p) = Settings::load().avd_path {
        return p;
    }
    std::env::var_os("ANDROID_AVD_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".android/avd"))
}

pub fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

pub fn android_version(api: u32) -> &'static str {
    match api {
        37 => "17",
        36 => "16",
        35 => "15",
        34 => "14",
        33 => "13",
        32 => "12L",
        31 => "12",
        30 => "11",
        29 => "10",
        28 => "9",
        27 => "8.1",
        26 => "8.0",
        25 => "7.1",
        24 => "7.0",
        23 => "6.0",
        22 => "5.1",
        21 => "5.0",
        20 => "4.4W",
        19 => "4.4",
        18 => "4.3",
        17 => "4.2",
        16 => "4.1",
        15 => "4.0.3",
        14 => "4.0",
        10 => "2.3.3",
        _ => "",
    }
}

/// Extrai o nível de API de "android-34", "android-36.1" ou "android-34-ext10".
pub fn parse_api(s: &str) -> u32 {
    s.trim_start_matches("android-").chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)
}

/// "Android 14" a partir de "android-34".
pub fn android_label(api: &str) -> String {
    let n = parse_api(api);
    match android_version(n) {
        "" => format!("API {n}"),
        v => format!("Android {v}"),
    }
}

pub fn human_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels() {
        assert_eq!(android_label("android-34"), "Android 14");
        assert_eq!(android_label("android-34-ext10"), "Android 14");
        assert_eq!(android_label("android-36.1"), "Android 16");
        assert_eq!(android_label("android-99"), "API 99");
        assert_eq!(android_label("android-10"), "Android 2.3.3");
        assert_eq!(human_bytes(1536 * 1024), "1.5 MB");
    }
}
