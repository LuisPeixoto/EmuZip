//! Backends de emulação: Android Emulator (AVD) e QEMU direto.
pub mod avd;
pub mod qemu;

use anyhow::Result;
use emuzip_core::{BackendKind, Device};

/// Especificação de um novo dispositivo.
#[derive(Debug, Clone)]
pub struct CreateSpec {
    pub name: String,
    /// AVD: id da imagem (android-34/google_apis/x86_64). QEMU: caminho do disco/ISO.
    pub image: String,
    pub profile: String,
    pub width: u32,
    pub height: u32,
    pub density: u32,
    pub ram_mb: u32,
    pub cpus: u32,
    pub storage_gb: u32,
    pub gpu: String,
    pub camera_back: String,
    pub camera_front: String,
    pub keyboard: bool,
    pub nav_buttons: bool,
}

#[derive(Debug, Clone, Default)]
pub struct StartOpts {
    pub cold_boot: bool,
    pub headless: bool,
    pub gpu: String,
    pub audio: bool,
    pub wipe: bool,
}

pub trait Backend: Send + Sync {
    fn kind(&self) -> BackendKind;
    fn list(&self) -> Result<Vec<Device>>;
    fn create(&self, spec: &CreateSpec) -> Result<String>;
    fn start(&self, id: &str, opts: &StartOpts) -> Result<()>;
    fn stop(&self, device: &Device) -> Result<()>;
    fn delete(&self, device: &Device) -> Result<()>;
}

/// Backends detectados nesta máquina.
pub fn detect() -> Vec<Box<dyn Backend>> {
    let mut v: Vec<Box<dyn Backend>> = Vec::new();
    if let Some(b) = avd::AvdBackend::detect() {
        v.push(Box::new(b));
    }
    if let Some(b) = qemu::QemuBackend::detect() {
        v.push(Box::new(b));
    }
    v
}

pub fn list_all(backends: &[Box<dyn Backend>]) -> Vec<Device> {
    backends.iter().filter_map(|b| b.list().ok()).flatten().collect()
}
