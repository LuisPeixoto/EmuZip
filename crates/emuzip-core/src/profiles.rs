//! Perfis de hardware (equivalente aos HwProfile do Genymotion).
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HwProfile {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub kind: DeviceKind,
    pub width: u32,
    pub height: u32,
    pub density: u32,
    pub ram_mb: u32,
    pub cpus: u32,
    pub storage_gb: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceKind {
    Phone,
    Foldable,
    Tablet,
    Tv,
    Wear,
}

impl DeviceKind {
    pub fn label(self) -> &'static str {
        match self {
            DeviceKind::Phone => "Phone",
            DeviceKind::Foldable => "Foldable",
            DeviceKind::Tablet => "Tablet",
            DeviceKind::Tv => "TV",
            DeviceKind::Wear => "Watch",
        }
    }
}

fn p(id: &str, name: &str, mf: &str, kind: DeviceKind, w: u32, h: u32, d: u32, ram: u32, cpus: u32, st: u32) -> HwProfile {
    HwProfile { id: id.into(), name: name.into(), manufacturer: mf.into(), kind, width: w, height: h, density: d, ram_mb: ram, cpus, storage_gb: st }
}

pub fn presets() -> Vec<HwProfile> {
    use DeviceKind::*;
    vec![
        p("pixel_9", "Pixel 9", "Google", Phone, 1080, 2424, 420, 4096, 4, 8),
        p("pixel_9_pro_xl", "Pixel 9 Pro XL", "Google", Phone, 1344, 2992, 480, 4096, 4, 8),
        p("pixel_8a", "Pixel 8a", "Google", Phone, 1080, 2400, 420, 3072, 4, 8),
        p("pixel_5", "Pixel 5", "Google", Phone, 1080, 2340, 440, 2048, 2, 6),
        p("galaxy_s24", "Galaxy S24", "Samsung", Phone, 1080, 2340, 416, 4096, 4, 8),
        p("galaxy_a54", "Galaxy A54", "Samsung", Phone, 1080, 2340, 403, 3072, 4, 8),
        p("moto_g", "Moto G (720p)", "Motorola", Phone, 720, 1600, 280, 2048, 2, 6),
        p("small_phone", "Small phone", "Generic", Phone, 720, 1280, 320, 2048, 2, 4),
        p("pixel_fold", "Pixel Fold", "Google", Foldable, 2208, 1840, 420, 4096, 4, 8),
        p("pixel_tablet", "Pixel Tablet", "Google", Tablet, 2560, 1600, 320, 4096, 4, 8),
        p("galaxy_tab_s9", "Galaxy Tab S9", "Samsung", Tablet, 2560, 1600, 340, 4096, 4, 8),
        p("tv_1080p", "TV 1080p", "Generic", Tv, 1920, 1080, 320, 2048, 2, 6),
        p("wear_round", "Wear OS round", "Generic", Wear, 454, 454, 320, 2048, 2, 4),
    ]
}

pub fn find(id: &str) -> Option<HwProfile> {
    presets().into_iter().find(|p| p.id == id)
}
