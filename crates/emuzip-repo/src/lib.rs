//! Catálogo oficial de imagens do Android e download nativo (sem sdkmanager/Java).
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use sha1::{Digest, Sha1};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const DEFAULT_BASE: &str = "https://dl.google.com/android/repository/sys-img";
/// Canais de imagens (tag do repositório).
pub const CHANNELS: [&str; 8] = ["google_apis_playstore", "google_apis", "android", "aosp_atd", "google_atd", "android-tv", "android-wear", "android-desktop"];

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteImage {
    pub package: String,
    pub api_dir: String,
    pub api: u32,
    pub tag: String,
    pub tag_display: String,
    pub abi: String,
    pub revision: String,
    pub display_name: String,
    pub size: u64,
    pub sha1: String,
    pub url: String,
}

impl RemoteImage {
    pub fn id(&self) -> String {
        format!("{}/{}/{}", self.api_dir, self.tag, self.abi)
    }
    pub fn title(&self) -> String {
        match emuzip_core::android_version(self.api) {
            "" => format!("API {} · {}", self.api, self.tag_display),
            v => format!("Android {v} · {}", self.tag_display),
        }
    }
}

fn base() -> String {
    emuzip_core::Settings::load().repo_mirror.filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_BASE.into())
}

pub fn client() -> Result<reqwest::Client> {
    let mut b = reqwest::Client::builder().user_agent("EmuZip/0.2");
    if let Some(p) = emuzip_core::Settings::load().download_proxy.filter(|s| !s.is_empty()) {
        b = b.proxy(reqwest::Proxy::all(p)?);
    }
    Ok(b.build()?)
}

/// Faz o parse de um `sys-img2-*.xml`, mantendo só o canal estável e não obsoletos.
pub fn parse_catalog(xml: &str, channel: &str) -> Result<Vec<RemoteImage>> {
    let doc = roxmltree::Document::parse(xml)?;
    let stable: Vec<String> =
        doc.descendants().filter(|n| n.has_tag_name("channel") && n.text() == Some("stable")).filter_map(|n| n.attribute("id").map(String::from)).collect();
    let txt = |n: roxmltree::Node, tag: &str| n.descendants().find(|c| c.has_tag_name(tag)).and_then(|c| c.text()).unwrap_or("").trim().to_string();
    let mut out = Vec::new();
    for pkg in doc.descendants().filter(|n| n.has_tag_name("remotePackage")) {
        let Some(path) = pkg.attribute("path") else { continue };
        if pkg.attribute("obsolete") == Some("true") || pkg.children().any(|c| c.has_tag_name("obsolete")) {
            continue;
        }
        let ch = pkg.children().find(|c| c.has_tag_name("channelRef")).and_then(|c| c.attribute("ref")).unwrap_or("channel-0");
        if !stable.is_empty() && !stable.iter().any(|s| s == ch) {
            continue;
        }
        // Prévias (beta/dev) têm <codename> ou "-beta"/"preview" no caminho.
        if pkg.descendants().any(|n| n.has_tag_name("codename")) || path.contains("-beta") || path.contains("preview") {
            continue;
        }
        let parts: Vec<&str> = path.split(';').collect();
        if parts.len() != 4 {
            continue;
        }
        let Some(archive) = pkg.descendants().filter(|n| n.has_tag_name("archive")).find(|a| {
            let os = txt(*a, "host-os");
            os.is_empty() || os == std::env::consts::OS || (os == "macosx" && cfg!(target_os = "macos"))
        }) else {
            continue;
        };
        let complete = archive.descendants().find(|n| n.has_tag_name("complete")).unwrap_or(archive);
        let tag_node = pkg.descendants().find(|n| n.has_tag_name("tag"));
        let rev = pkg.descendants().find(|n| n.has_tag_name("revision"));
        let url = txt(complete, "url");
        out.push(RemoteImage {
            package: path.into(),
            api_dir: parts[1].into(),
            api: txt(pkg, "api-level").parse().unwrap_or_else(|_| emuzip_core::parse_api(parts[1])),
            tag: parts[2].into(),
            tag_display: tag_node.map(|t| txt(t, "display")).filter(|s| !s.is_empty()).unwrap_or_else(|| emuzip_core::images::tag_display(parts[2])),
            abi: parts[3].into(),
            revision: rev.map(|r| txt(r, "major")).unwrap_or_default(),
            display_name: txt(pkg, "display-name"),
            size: txt(complete, "size").parse().unwrap_or(0),
            sha1: txt(complete, "checksum"),
            url: if url.starts_with("http") { url } else { format!("{}/{channel}/{url}", base()) },
        });
    }
    Ok(out)
}

/// Baixa o catálogo de todos os canais (falhas individuais são ignoradas).
pub async fn catalog() -> Result<Vec<RemoteImage>> {
    let c = client()?;
    let futs = CHANNELS.iter().map(|ch| {
        let c = c.clone();
        async move {
            let xml = c.get(format!("{}/{ch}/sys-img2-3.xml", base())).send().await?.error_for_status()?.text().await?;
            parse_catalog(&xml, ch)
        }
    });
    let mut all: Vec<RemoteImage> = futures_util::future::join_all(futs).await.into_iter().filter_map(Result::ok).flatten().collect();
    all.sort_by(|a, b| b.api.cmp(&a.api).then(a.tag.cmp(&b.tag)).then(a.abi.cmp(&b.abi)));
    all.dedup_by(|a, b| a.package == b.package);
    Ok(all)
}

#[derive(Debug, Clone, Copy)]
pub enum Phase {
    Downloading { done: u64, total: u64, bytes_per_sec: f64 },
    Verifying,
    Extracting { done: usize, total: usize },
}

/// Baixa, verifica e extrai uma imagem em `<sdk>/system-images/<api>/<tag>/<abi>`.
pub async fn install(img: &RemoteImage, sdk: &Path, cancel: Arc<AtomicBool>, mut progress: impl FnMut(Phase) + Send) -> Result<PathBuf> {
    let cache = emuzip_core::data_dir().join("downloads");
    std::fs::create_dir_all(&cache)?;
    let zip_path = cache.join(img.url.rsplit('/').next().unwrap_or("image.zip"));

    let resp = client()?.get(&img.url).send().await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(img.size);
    let mut file = std::fs::File::create(&zip_path)?;
    let mut hasher = Sha1::new();
    let mut done = 0u64;
    let start = std::time::Instant::now();
    let mut last = std::time::Instant::now();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = std::fs::remove_file(&zip_path);
            bail!("cancelado");
        }
        let chunk = chunk?;
        file.write_all(&chunk)?;
        hasher.update(&chunk);
        done += chunk.len() as u64;
        if last.elapsed().as_millis() > 150 {
            last = std::time::Instant::now();
            let bps = done as f64 / start.elapsed().as_secs_f64().max(0.001);
            progress(Phase::Downloading { done, total, bytes_per_sec: bps });
        }
    }
    file.flush()?;
    drop(file);

    progress(Phase::Verifying);
    let got = hex::encode(hasher.finalize());
    if !img.sha1.is_empty() && !got.eq_ignore_ascii_case(&img.sha1) {
        let _ = std::fs::remove_file(&zip_path);
        bail!("checksum inválido (esperado {}, obtido {got})", img.sha1);
    }

    let dest = sdk.join("system-images").join(&img.api_dir).join(&img.tag).join(&img.abi);
    let parent = dest.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&parent)?;
    let (zp, d) = (zip_path.clone(), dest.clone());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let job = tokio::task::spawn_blocking(move || {
        extract(&zp, &d, cancel, |a, b| {
            let _ = tx.send((a, b));
        })
    });
    while let Some((a, b)) = rx.recv().await {
        progress(Phase::Extracting { done: a, total: b });
    }
    job.await??;
    write_package_xml(img, &dest)?;
    let _ = std::fs::remove_file(&zip_path);
    Ok(dest)
}

/// Extrai removendo a pasta raiz do zip (ex.: "x86_64/") para dentro de `dest`.
fn extract(zip_path: &Path, dest: &Path, cancel: Arc<AtomicBool>, progress: impl Fn(usize, usize)) -> Result<()> {
    let f = std::fs::File::open(zip_path)?;
    let mut z = zip::ZipArchive::new(f).context("zip inválido")?;
    let tmp = dest.with_extension("partial");
    let _ = std::fs::remove_dir_all(&tmp);
    let n = z.len();
    for i in 0..n {
        if cancel.load(Ordering::Relaxed) {
            let _ = std::fs::remove_dir_all(&tmp);
            bail!("cancelado");
        }
        let mut e = z.by_index(i)?;
        let Some(rel) = e.enclosed_name() else { continue };
        let rel: PathBuf = rel.components().skip(1).collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = tmp.join(rel);
        if e.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        std::fs::create_dir_all(out.parent().unwrap())?;
        let mut o = std::fs::File::create(&out)?;
        std::io::copy(&mut e, &mut o)?;
        #[cfg(unix)]
        if let Some(mode) = e.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode));
        }
        if i % 4 == 0 || i + 1 == n {
            progress(i + 1, n);
        }
    }
    let _ = std::fs::remove_dir_all(dest);
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

/// Garante `source.properties` para o emulador/Android Studio reconhecerem a imagem.
fn write_package_xml(img: &RemoteImage, dest: &Path) -> Result<()> {
    let sp = dest.join("source.properties");
    if !sp.exists() {
        std::fs::write(
            sp,
            format!(
                "Pkg.Desc={}\nPkg.Revision={}\nAndroidVersion.ApiLevel={}\nSystemImage.Abi={}\nSystemImage.TagId={}\nSystemImage.TagDisplay={}\n",
                img.display_name, img.revision, img.api, img.abi, img.tag, img.tag_display
            ),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    const XML: &str = r#"<?xml version="1.0"?>
<sys-img:sdk-sys-img xmlns:sys-img="http://schemas.android.com/sdk/android/repo/sys-img2/03" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<channel id="channel-0">stable</channel><channel id="channel-1">beta</channel>
<remotePackage path="system-images;android-34;google_apis;x86_64">
 <type-details xsi:type="sys-img:sysImgDetailsType"><api-level>34</api-level><tag><id>google_apis</id><display>Google APIs</display></tag><abi>x86_64</abi></type-details>
 <revision><major>14</major></revision><display-name>Google APIs Intel x86_64</display-name><channelRef ref="channel-0"/>
 <archives><archive><complete><size>100</size><checksum type="sha1">abc</checksum><url>x86_64-34_r14.zip</url></complete></archive></archives>
</remotePackage>
<remotePackage path="system-images;android-99;google_apis;x86_64">
 <type-details xsi:type="sys-img:sysImgDetailsType"><api-level>99</api-level><abi>x86_64</abi></type-details>
 <revision><major>1</major></revision><channelRef ref="channel-1"/>
 <archives><archive><complete><size>1</size><url>b.zip</url></complete></archive></archives>
</remotePackage>
</sys-img:sdk-sys-img>"#;

    #[test]
    fn parses_stable_only() {
        let v = super::parse_catalog(XML, "google_apis").unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].api, 34);
        assert_eq!(v[0].size, 100);
        assert_eq!(v[0].tag_display, "Google APIs");
        assert!(v[0].url.ends_with("/google_apis/x86_64-34_r14.zip"));
        assert_eq!(v[0].title(), "Android 14 · Google APIs");
    }
}
