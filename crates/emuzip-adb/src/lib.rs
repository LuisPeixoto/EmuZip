//! Cliente mínimo do protocolo ADB (servidor em 127.0.0.1:5037), em Rust puro.
use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;

pub const ADDR: &str = "127.0.0.1:5037";

fn connect(cmd: &str) -> Result<TcpStream> {
    let mut s = TcpStream::connect(ADDR).context("adb server unavailable (run `adb start-server`)")?;
    s.write_all(format!("{:04x}{}", cmd.len(), cmd).as_bytes())?;
    let mut st = [0u8; 4];
    s.read_exact(&mut st)?;
    if &st == b"OKAY" {
        return Ok(s);
    }
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let n = usize::from_str_radix(std::str::from_utf8(&len)?, 16)?;
    let mut msg = vec![0; n];
    s.read_exact(&mut msg)?;
    bail!("adb: {}", String::from_utf8_lossy(&msg))
}

fn read_len_prefixed(s: &mut TcpStream) -> Result<String> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let n = usize::from_str_radix(std::str::from_utf8(&len)?, 16)?;
    let mut buf = vec![0; n];
    s.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Retorna (serial, estado) de cada dispositivo.
pub fn devices() -> Result<Vec<(String, String)>> {
    let mut s = connect("host:devices")?;
    Ok(parse_devices(&read_len_prefixed(&mut s)?))
}

pub fn parse_devices(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.to_string(), it.next()?.to_string()))
        })
        .collect()
}

fn transport(serial: &str, svc: &str) -> Result<TcpStream> {
    let mut s = TcpStream::connect(ADDR).context("adb server unavailable")?;
    for cmd in [format!("host:transport:{serial}"), svc.to_string()] {
        s.write_all(format!("{:04x}{}", cmd.len(), cmd).as_bytes())?;
        let mut st = [0u8; 4];
        s.read_exact(&mut st)?;
        if &st != b"OKAY" {
            bail!("adb recusou `{cmd}`");
        }
    }
    Ok(s)
}

pub fn shell(serial: &str, cmd: &str) -> Result<String> {
    let mut s = transport(serial, &format!("shell:{cmd}"))?;
    let mut out = Vec::new();
    s.read_to_end(&mut out)?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Instala um APK enviando o arquivo direto ao `cmd package install`.
pub fn install(serial: &str, apk: &Path) -> Result<String> {
    let mut f = File::open(apk).with_context(|| format!("abrindo {}", apk.display()))?;
    let size = f.metadata()?.len();
    let mut s = transport(serial, &format!("exec:cmd package install -S {size}"))?;
    std::io::copy(&mut f, &mut s)?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    if out.contains("Success") {
        Ok(out.trim().into())
    } else {
        bail!("install failed: {}", out.trim())
    }
}

pub fn kill_emulator(serial: &str) -> Result<()> {
    let mut s = transport(serial, "emu:kill")?;
    let mut sink = Vec::new();
    let _ = s.read_to_end(&mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse() {
        let d = super::parse_devices("emulator-5554\tdevice\nR58M\toffline\n");
        assert_eq!(d, vec![("emulator-5554".into(), "device".into()), ("R58M".into(), "offline".into())]);
    }
}

/// Saída binária de um comando (`exec:`), ex.: `screencap -p`.
pub fn exec_out(serial: &str, cmd: &str) -> Result<Vec<u8>> {
    let mut s = transport(serial, &format!("exec:{cmd}"))?;
    let mut out = Vec::new();
    s.read_to_end(&mut out)?;
    Ok(out)
}

/// Envia um arquivo pelo protocolo `sync:` (SEND/DATA/DONE), com progresso.
pub fn push(serial: &str, local: &Path, remote: &str, mut progress: impl FnMut(u64, u64)) -> Result<()> {
    let mut f = File::open(local).with_context(|| format!("abrindo {}", local.display()))?;
    let total = f.metadata()?.len();
    let mut s = transport(serial, "sync:")?;
    let spec = format!("{remote},33188");
    s.write_all(b"SEND")?;
    s.write_all(&(spec.len() as u32).to_le_bytes())?;
    s.write_all(spec.as_bytes())?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        s.write_all(b"DATA")?;
        s.write_all(&(n as u32).to_le_bytes())?;
        s.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    let mtime = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0);
    s.write_all(b"DONE")?;
    s.write_all(&mtime.to_le_bytes())?;
    let mut hdr = [0u8; 8];
    s.read_exact(&mut hdr)?;
    if &hdr[..4] != b"OKAY" {
        let n = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
        let mut msg = vec![0; n];
        let _ = s.read_exact(&mut msg);
        bail!("push falhou: {}", String::from_utf8_lossy(&msg));
    }
    Ok(())
}

/// Baixa um arquivo do device pelo protocolo `sync:` (RECV).
pub fn pull(serial: &str, remote: &str, local: &Path) -> Result<()> {
    let mut s = transport(serial, "sync:")?;
    s.write_all(b"RECV")?;
    s.write_all(&(remote.len() as u32).to_le_bytes())?;
    s.write_all(remote.as_bytes())?;
    let mut out = File::create(local)?;
    loop {
        let mut hdr = [0u8; 8];
        s.read_exact(&mut hdr)?;
        let n = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
        match &hdr[..4] {
            b"DATA" => {
                let mut buf = vec![0; n];
                s.read_exact(&mut buf)?;
                out.write_all(&buf)?;
            }
            b"DONE" => return Ok(()),
            _ => {
                let mut msg = vec![0; n];
                let _ = s.read_exact(&mut msg);
                bail!("pull falhou: {}", String::from_utf8_lossy(&msg));
            }
        }
    }
}

/// Pacotes instalados pelo usuário (`-3`) ou todos.
pub fn packages(serial: &str, third_party_only: bool) -> Result<Vec<String>> {
    let out = shell(serial, if third_party_only { "pm list packages -3" } else { "pm list packages" })?;
    let mut v: Vec<String> = out.lines().filter_map(|l| l.trim().strip_prefix("package:").map(String::from)).collect();
    v.sort();
    Ok(v)
}

/// Instala um APK com progresso do envio (push + `pm install`).
pub fn install_with_progress(serial: &str, apk: &Path, progress: impl FnMut(u64, u64)) -> Result<String> {
    let remote = "/data/local/tmp/emuzip_install.apk";
    push(serial, apk, remote, progress)?;
    let out = shell(serial, &format!("pm install -r -t -g {remote}; rm -f {remote}"))?;
    if out.contains("Success") {
        Ok("Installed successfully".into())
    } else {
        bail!("install failed: {}", out.trim())
    }
}

/// Executa um comando e entrega a saída linha a linha até `on_line` retornar false.
pub fn shell_stream(serial: &str, cmd: &str, mut on_line: impl FnMut(&str) -> bool) -> Result<()> {
    use std::io::BufRead;
    let s = transport(serial, &format!("shell:{cmd}"))?;
    let mut r = std::io::BufReader::new(s);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        if r.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        if !on_line(String::from_utf8_lossy(&buf).trim_end()) {
            return Ok(());
        }
    }
}

/// Reinicia o adbd como root (imagens não-Play Store).
pub fn root(serial: &str) -> Result<String> {
    let mut s = transport(serial, "root:")?;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    Ok(out.trim().to_string())
}
