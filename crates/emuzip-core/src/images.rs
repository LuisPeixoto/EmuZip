//! Imagens de sistema Android instaladas no SDK.
use crate::{android_version, ini, parse_api};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct SystemImage {
    /// Ex.: "android-34/google_apis/x86_64"
    pub id: String,
    pub api: u32,
    pub api_dir: String,
    pub tag: String,
    pub tag_display: String,
    pub abi: String,
    pub path: PathBuf,
}

impl SystemImage {
    pub fn title(&self) -> String {
        let v = android_version(self.api);
        let base = if v.is_empty() { format!("API {}", self.api) } else { format!("Android {v}") };
        format!("{base} · {}", self.tag_display)
    }
    pub fn play_store(&self) -> bool {
        self.tag.contains("playstore")
    }
    /// Caminho relativo usado em `image.sysdir.1`.
    pub fn sysdir(&self) -> String {
        format!("system-images/{}/", self.id)
    }
    /// Pacote SDK correspondente: "system-images;android-34;google_apis;x86_64".
    pub fn package(&self) -> String {
        format!("system-images;{};{};{}", self.api_dir, self.tag, self.abi)
    }
}

pub fn tag_display(tag: &str) -> String {
    match tag {
        "google_apis" => "Google APIs".into(),
        "google_apis_playstore" => "Google Play".into(),
        "default" => "AOSP".into(),
        "android-tv" | "google-tv" => "Android TV".into(),
        "android-wear" | "wear" => "Wear OS".into(),
        "android-automotive" | "android-automotive-playstore" => "Automotive".into(),
        "android-desktop" => "Desktop".into(),
        t => t.replace(['_', '-'], " "),
    }
}

/// Varre `<sdk>/system-images/<api>/<tag>/<abi>` (instantâneo, sem Java).
pub fn scan(sdk: &Path) -> Vec<SystemImage> {
    let mut out = Vec::new();
    let root = sdk.join("system-images");
    let dirs = |p: &Path| -> Vec<PathBuf> { std::fs::read_dir(p).map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default() };
    for api_p in dirs(&root) {
        for tag_p in dirs(&api_p) {
            for abi_p in dirs(&tag_p) {
                let props = ini::read(&abi_p.join("source.properties")).unwrap_or_default();
                if !abi_p.join("system.img").exists() && props.is_empty() {
                    continue;
                }
                let name = |p: &Path| p.file_name().unwrap().to_string_lossy().to_string();
                let (api_dir, tag, abi) = (name(&api_p), name(&tag_p), name(&abi_p));
                let api = props.get("AndroidVersion.ApiLevel").and_then(|v| v.parse().ok()).unwrap_or_else(|| parse_api(&api_dir));
                out.push(SystemImage {
                    id: format!("{api_dir}/{tag}/{abi}"),
                    api,
                    tag_display: props
                        .get("SystemImage.TagDisplay")
                        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
                        .unwrap_or_else(|| tag_display(&tag)),
                    api_dir,
                    tag,
                    abi,
                    path: abi_p,
                });
            }
        }
    }
    out.sort_by(|a, b| b.api.cmp(&a.api).then(a.tag.cmp(&b.tag)));
    out
}
