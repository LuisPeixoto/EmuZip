//! Configurações persistentes (TOML em data_dir()/settings.toml).
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub sdk_path: Option<PathBuf>,
    pub avd_path: Option<PathBuf>,
    pub qemu_path: Option<PathBuf>,
    pub captures_dir: Option<PathBuf>,
    /// "light", "dark" ou "system".
    pub theme: String,
    /// Tela embutida (gRPC) ou janela do emulador.
    pub embedded_display: bool,
    pub stream_fps: u32,
    pub default_profile: String,
    pub default_ram_mb: u32,
    pub default_cpus: u32,
    /// "auto", "host", "swiftshader_indirect", "angle_indirect"
    pub gpu_mode: String,
    pub cold_boot: bool,
    pub audio: bool,
    pub download_proxy: Option<String>,
    pub repo_mirror: Option<String>,
    pub confirm_destructive: bool,
    /// "system", "pt-BR", "en", "es", "fr", "de" ou "zh-CN".
    pub language: String,
    pub auto_fit_window: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sdk_path: None,
            avd_path: None,
            qemu_path: None,
            captures_dir: None,
            theme: "light".into(),
            embedded_display: true,
            stream_fps: 30,
            default_profile: "pixel_9".into(),
            default_ram_mb: 4096,
            default_cpus: 4,
            gpu_mode: "auto".into(),
            cold_boot: false,
            audio: false,
            download_proxy: None,
            repo_mirror: None,
            confirm_destructive: true,
            language: "system".into(),
            auto_fit_window: false,
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        crate::data_dir().join("settings.toml")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(crate::data_dir())?;
        std::fs::write(Self::path(), toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn captures_dir(&self) -> PathBuf {
        self.captures_dir.clone().unwrap_or_else(|| {
            directories::UserDirs::new().and_then(|u| u.picture_dir().map(|p| p.join("EmuZip"))).unwrap_or_else(|| crate::data_dir().join("captures"))
        })
    }
}
