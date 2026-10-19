use anyhow::{anyhow, bail, Result};
use emuzip_backend::avd::AvdBackend;
use emuzip_backend::{detect, list_all, Backend, CreateSpec, StartOpts};
use emuzip_core::{human_bytes, profiles, State};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const HELP: &str = "emuzipctl <command>
  list                              devices
  images                            installed images
  catalog                           images available for download
  download <package|id>              download and install an image
  profiles                          hardware profiles
  create <nome> <imagem> [profile]   create an AVD (no Java needed)
  start <id> [--cold] [--window]    start (headless unless --window)
  stop <id> | delete <id> | wipe <id> | repair <id>
  clone <id> <new-name>
  install <id> <apk>
  push <id> <file> [destination]
  screenshot <id> <out.png>        via gRPC
  check <id>                        check gRPC, console and adb on a running device";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let avd = AvdBackend::detect();
    let backends = detect();
    let need_avd = || avd.as_ref().ok_or_else(|| anyhow!("Android SDK/emulator not found"));
    let device = |id: &str| list_all(&backends).into_iter().find(|d| d.id == id).ok_or_else(|| anyhow!("device {id} not found"));
    let backend_of = |d: &emuzip_core::Device| backends.iter().find(|b| b.kind() == d.backend).unwrap();

    match a.as_slice() {
        ["list"] => {
            for d in list_all(&backends) {
                let st = match d.state {
                    State::Running => "running",
                    State::Booting => "booting",
                    State::Stopped => "stopped",
                    State::Broken => "BROKEN",
                };
                println!("{:<24} {:<12} {:<14} {:<10} {}", d.id, d.android, d.tag, st, d.problem.clone().unwrap_or_else(|| d.summary()));
            }
        }
        ["images"] => {
            for i in need_avd()?.images() {
                println!("{:<45} {}", i.id, i.title());
            }
        }
        ["catalog"] => {
            for i in emuzip_repo::catalog().await? {
                println!("{:<48} {:<28} {:>10}", i.id(), i.title(), human_bytes(i.size));
            }
        }
        ["download", what] => {
            let cat = emuzip_repo::catalog().await?;
            let img = cat.iter().find(|i| i.package == *what || i.id() == *what).ok_or_else(|| anyhow!("image not found in the catalog"))?;
            let dest = emuzip_repo::install(img, &need_avd()?.sdk, Arc::new(AtomicBool::new(false)), |p| match p {
                emuzip_repo::Phase::Downloading { done, total, bytes_per_sec } => {
                    eprint!("\rdownloading {} / {} ({}/s)   ", human_bytes(done), human_bytes(total), human_bytes(bytes_per_sec as u64))
                }
                emuzip_repo::Phase::Verifying => eprint!("\rverificando…                         "),
                emuzip_repo::Phase::Extracting { done, total } => eprint!("\rextraindo {done}/{total}                 "),
            })
            .await?;
            eprintln!("\ninstalled at {}", dest.display());
        }
        ["profiles"] => {
            for p in profiles::presets() {
                println!("{:<16} {:<20} {:<9} {}x{} @{}dpi", p.id, p.name, p.kind.label(), p.width, p.height, p.density);
            }
        }
        ["create", name, image, rest @ ..] => {
            let p = profiles::find(rest.first().copied().unwrap_or("pixel_9")).ok_or_else(|| anyhow!("unknown profile"))?;
            let id = need_avd()?.create(&CreateSpec {
                name: name.to_string(),
                image: image.to_string(),
                profile: p.id.clone(),
                width: p.width,
                height: p.height,
                density: p.density,
                ram_mb: p.ram_mb,
                cpus: p.cpus,
                storage_gb: p.storage_gb,
                gpu: "auto".into(),
                camera_back: "emulated".into(),
                camera_front: "emulated".into(),
                keyboard: true,
                nav_buttons: false,
            })?;
            println!("created: {id}");
        }
        ["start", id, flags @ ..] => {
            let d = device(id)?;
            let opts = StartOpts { cold_boot: flags.contains(&"--cold"), headless: !flags.contains(&"--window"), gpu: "auto".into(), ..Default::default() };
            backend_of(&d).start(id, &opts)?;
            println!("starting {id}…");
        }
        ["stop", id] => {
            let d = device(id)?;
            backend_of(&d).stop(&d)?;
        }
        ["delete", id] => {
            let d = device(id)?;
            backend_of(&d).delete(&d)?;
        }
        ["wipe", id] => need_avd()?.wipe(id)?,
        ["repair", id] => println!("{}", need_avd()?.repair(id)?),
        ["clone", id, name] => println!("clonado: {}", need_avd()?.clone_avd(id, name)?),
        ["install", id, apk] => {
            let serial = device(id)?.serial.ok_or_else(|| anyhow!("device is not running"))?;
            println!("{}", emuzip_adb::install_with_progress(&serial, apk.as_ref(), |d, t| eprint!("\r{}%", d * 100 / t.max(1)))?);
        }
        ["push", id, file, rest @ ..] => {
            let serial = device(id)?.serial.ok_or_else(|| anyhow!("device is not running"))?;
            let name = std::path::Path::new(file).file_name().unwrap().to_string_lossy().to_string();
            let dest = rest.first().map(|s| s.to_string()).unwrap_or(format!("/sdcard/Download/{name}"));
            emuzip_adb::push(&serial, file.as_ref(), &dest, |d, t| eprint!("\r{}%", d * 100 / t.max(1)))?;
            println!("\nenviado para {dest}");
        }
        ["screenshot", id, out] => {
            let r = AvdBackend::find_running(id).ok_or_else(|| anyhow!("device is not running"))?;
            let port = r.grpc_port.ok_or_else(|| anyhow!("gRPC unavailable"))?;
            let mut emu = emuzip_grpc::Emulator::connect(port, r.grpc_token.as_deref()).await?;
            let img = emu
                .ctl
                .get_screenshot(emuzip_grpc::control::ImageFormat { format: emuzip_grpc::control::image_format::ImgFormat::Png as i32, ..Default::default() })
                .await?
                .into_inner();
            if img.image.is_empty() {
                bail!("empty screen (is the device still booting?)");
            }
            std::fs::write(out, &img.image)?;
            println!("salvo {out} ({} bytes)", img.image.len());
        }
        ["check", id] => {
            use emuzip_grpc::control;
            let r = AvdBackend::find_running(id).ok_or_else(|| anyhow!("device is not running"))?;
            let serial = r.serial();
            let ok = |n: &str, res: Result<String>| match res {
                Ok(m) => println!("  ✓ {n:<22} {m}"),
                Err(e) => println!("  ✗ {n:<22} {e:#}"),
            };
            let mut emu = emuzip_grpc::Emulator::connect(r.grpc_port.ok_or_else(|| anyhow!("sem gRPC"))?, r.grpc_token.as_deref()).await?;
            let st = emu.ctl.get_status(()).await.map(|s| format!("uptime {} ms", s.into_inner().uptime)).map_err(Into::into);
            ok("gRPC status", st);
            let b =
                emu.ctl.set_battery(control::BatteryState { has_battery: true, is_present: true, charger: 0, charge_level: 42, health: 0, status: 2 }).await;
            ok(
                "gRPC bateria 42%",
                b.map(|_| emuzip_adb::shell(&serial, "dumpsys battery | grep level").unwrap_or_default().trim().to_string()).map_err(Into::into),
            );
            let g = emu.ctl.set_gps(control::GpsState { latitude: -22.9068, longitude: -43.1729, altitude: 5.0, satellites: 12, ..Default::default() }).await;
            ok("gRPC GPS Rio", g.map(|_| "ok".to_string()).map_err(Into::into));
            let s = emu.ctl.send_sms(control::SmsMessage { src_address: "+5511987654321".into(), text: "teste EmuZip".into() }).await;
            ok("gRPC SMS", s.map(|_| "ok".to_string()).map_err(Into::into));
            let c = emu.ctl.set_clipboard(control::ClipData { text: "clip EmuZip".into() }).await;
            ok("gRPC clipboard", c.map(|_| "ok".to_string()).map_err(Into::into));
            let br = emu.ctl.set_brightness(control::BrightnessValue { target: 0, value: 120 }).await;
            ok("gRPC brilho", br.map(|_| "ok".to_string()).map_err(Into::into));
            let sens = emu
                .ctl
                .set_sensor(control::SensorValue {
                    target: control::sensor_value::SensorType::Light as i32,
                    status: 0,
                    value: Some(control::ParameterValue { data: vec![500.0] }),
                })
                .await;
            ok("gRPC sensor luz", sens.map(|_| "ok".to_string()).map_err(Into::into));
            let k = emu.ctl.send_key(control::KeyboardEvent { event_type: 2, key: "Home".into(), ..Default::default() }).await;
            ok("gRPC tecla Home", k.map(|_| "ok".to_string()).map_err(Into::into));
            ok(
                "console network",
                emuzip_console::Console::connect(r.console_port)
                    .and_then(|mut c| {
                        c.cmd("network speed lte")?;
                        c.cmd("network delay none")?;
                        c.cmd("gsm signal-profile 3")
                    })
                    .map(|_| "lte".into()),
            );
            ok("console snapshots", emuzip_console::run(r.console_port, "avd snapshot list").map(|o| format!("{} linha(s)", o.lines().count())));
            ok("adb packages", emuzip_adb::packages(&serial, false).map(|p| format!("{} pacotes", p.len())));
            let tmp = std::env::temp_dir().join("emuzip-push.txt");
            std::fs::write(&tmp, "EmuZip push test")?;
            ok(
                "adb push",
                emuzip_adb::push(&serial, &tmp, "/sdcard/Download/emuzip-push.txt", |_, _| {})
                    .map(|_| emuzip_adb::shell(&serial, "cat /sdcard/Download/emuzip-push.txt").unwrap_or_default()),
            );
            ok("adb pull", emuzip_adb::pull(&serial, "/sdcard/Download/emuzip-push.txt", &tmp).map(|_| std::fs::read_to_string(&tmp).unwrap_or_default()));
            let mut n = 0;
            ok(
                "adb logcat stream",
                emuzip_adb::shell_stream(&serial, "logcat -v brief -T 20", |_| {
                    n += 1;
                    n < 20
                })
                .map(|_| format!("{n} linhas")),
            );
            ok(
                "adb wm/settings",
                emuzip_adb::shell(&serial, "settings put system show_touches 0; cmd uimode night no; wm density").map(|o| o.trim().replace('\n', " ")),
            );
        }
        _ => println!("{HELP}"),
    }
    Ok(())
}
