use crate::{Backend, CreateSpec, StartOpts};
use anyhow::{bail, Context, Result};
use emuzip_core::{data_dir, exe, BackendKind, Device, QemuProfile, State};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct QemuBackend {
    qemu: PathBuf,
    img: PathBuf,
}

fn find_in_path(bin: &str) -> Option<PathBuf> {
    let name = exe(bin);
    let extra = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "C:\\Program Files\\qemu"];
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .chain(extra.iter().map(PathBuf::from))
        .map(|d| d.join(&name))
        .find(|p| p.exists())
}

fn host_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    }
}

fn accel() -> &'static str {
    if cfg!(target_os = "macos") {
        "hvf"
    } else if cfg!(windows) {
        "whpx"
    } else {
        "kvm"
    }
}

fn profiles_dir() -> PathBuf {
    data_dir().join("qemu")
}

impl QemuBackend {
    pub fn detect() -> Option<Self> {
        if let Some(dir) = emuzip_core::Settings::load().qemu_path {
            let q = dir.join(exe(&format!("qemu-system-{}", host_arch())));
            let i = dir.join(exe("qemu-img"));
            if q.exists() && i.exists() {
                return Some(Self { qemu: q, img: i });
            }
        }
        let qemu = find_in_path(&format!("qemu-system-{}", host_arch()))?;
        let img = find_in_path("qemu-img")?;
        Some(Self { qemu, img })
    }

    fn pidfile(id: &str) -> PathBuf {
        profiles_dir().join(format!("{id}.pid"))
    }

    fn load(id: &str) -> Result<QemuProfile> {
        let t = std::fs::read_to_string(profiles_dir().join(format!("{id}.toml")))?;
        Ok(toml::from_str(&t)?)
    }

    /// Argumentos do QEMU para um perfil (separado para ser testável).
    pub fn args(p: &QemuProfile) -> Vec<String> {
        let mut a: Vec<String> = vec![
            "-accel".into(),
            accel().into(),
            "-smp".into(),
            p.cpus.to_string(),
            "-m".into(),
            p.ram_mb.to_string(),
            "-drive".into(),
            format!("file={},if=virtio,format=qcow2", p.disk.display()),
            "-device".into(),
            "virtio-gpu-pci".into(),
            "-device".into(),
            "qemu-xhci".into(),
            "-device".into(),
            "usb-tablet".into(),
            "-netdev".into(),
            "user,id=n0,hostfwd=tcp::5555-:5555".into(),
            "-device".into(),
            "virtio-net-pci,netdev=n0".into(),
            "-name".into(),
            p.name.clone(),
        ];
        if cfg!(target_arch = "aarch64") {
            a.extend(["-M".into(), "virt".into(), "-cpu".into(), "host".into()]);
        } else {
            a.extend(["-cpu".into(), "host".into()]);
        }
        a
    }
}

impl Backend for QemuBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Qemu
    }

    fn list(&self) -> Result<Vec<Device>> {
        let mut v = Vec::new();
        let Ok(rd) = std::fs::read_dir(profiles_dir()) else { return Ok(v) };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("toml") {
                continue;
            }
            let id = path.file_stem().unwrap().to_string_lossy().to_string();
            if let Ok(p) = Self::load(&id) {
                let running = std::fs::read_to_string(Self::pidfile(&id)).ok().and_then(|s| s.trim().parse::<u32>().ok()).map(pid_alive).unwrap_or(false);
                v.push(Device {
                    id,
                    name: p.name,
                    backend: BackendKind::Qemu,
                    android: "Android-x86/AOSP".into(),
                    api: 0,
                    state: if running { State::Running } else { State::Stopped },
                    serial: running.then(|| "127.0.0.1:5555".to_string()),
                    grpc_port: None,
                    width: 0,
                    height: 0,
                    density: 0,
                    ram_mb: p.ram_mb,
                    cpus: p.cpus,
                    tag: "QEMU".into(),
                    path: p.disk.clone(),
                    problem: None,
                });
            }
        }
        Ok(v)
    }

    /// Para o QEMU, `spec.image` é um disco base existente (qcow2/img/raw/iso).
    fn create(&self, spec: &CreateSpec) -> Result<String> {
        let (name, image) = (spec.name.as_str(), spec.image.as_str());
        let id = crate::avd::sanitize(name);
        std::fs::create_dir_all(profiles_dir())?;
        let disk = profiles_dir().join(format!("{id}.qcow2"));
        let out = Command::new(&self.img).args(["create", "-f", "qcow2", "-F", "raw", "-b", image]).arg(&disk).output()?;
        if !out.status.success() {
            bail!("{}", String::from_utf8_lossy(&out.stderr));
        }
        let p = QemuProfile { name: name.into(), disk, arch: host_arch().into(), cpus: spec.cpus.max(1), ram_mb: spec.ram_mb.max(1024) };
        std::fs::write(profiles_dir().join(format!("{id}.toml")), toml::to_string(&p)?)?;
        Ok(id)
    }

    fn start(&self, id: &str, _opts: &StartOpts) -> Result<()> {
        let p = Self::load(id)?;
        let child =
            Command::new(&self.qemu).args(Self::args(&p)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().context("starting qemu")?;
        std::fs::write(Self::pidfile(id), child.id().to_string())?;
        Ok(())
    }

    fn stop(&self, d: &Device) -> Result<()> {
        let pid = std::fs::read_to_string(Self::pidfile(&d.id))?.trim().to_string();
        let (prog, args): (&str, Vec<&str>) = if cfg!(windows) { ("taskkill", vec!["/PID", &pid, "/F"]) } else { ("kill", vec![&pid]) };
        Command::new(prog).args(args).status()?;
        let _ = std::fs::remove_file(Self::pidfile(&d.id));
        Ok(())
    }

    fn delete(&self, d: &Device) -> Result<()> {
        if d.state == State::Running {
            bail!("stop the device before deleting it");
        }
        let _ = std::fs::remove_file(profiles_dir().join(format!("{}.qcow2", d.id)));
        std::fs::remove_file(profiles_dir().join(format!("{}.toml", d.id)))?;
        Ok(())
    }
}

use crate::avd::pid_alive;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn args_have_memory_and_disk() {
        let p = QemuProfile { name: "t".into(), disk: "/x/d.qcow2".into(), arch: "x86_64".into(), cpus: 4, ram_mb: 4096 };
        let a = QemuBackend::args(&p);
        assert!(a.windows(2).any(|w| w == ["-m", "4096"]));
        assert!(a.iter().any(|s| s.contains("/x/d.qcow2")));
    }
}
