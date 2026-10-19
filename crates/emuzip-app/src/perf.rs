//! Monitor de desempenho: CPU/RAM do processo do emulador (host), CPU/RAM/GPU do Android e estresse de memória.
use crate::ctx::{model, Ctx};
use crate::{Api, MainWindow, Perf, ProcItem};
use emuzip_backend::avd::AvdBackend;
use slint::ComponentHandle;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const HISTORY: usize = 60;
/// Entrada por bloco do estresse de memória (o `sort` ocupa ~3× isso em RAM).
const FILL_CHUNK_MB: u32 = 32;

#[derive(Default)]
pub struct PerfState {
    pub running: Option<Arc<AtomicBool>>,
    pub sort_ram: bool,
    pub samples: Vec<Sample>,
    pub fg_pkg: String,
}

#[derive(Clone, Default, Debug)]
pub struct Sample {
    pub t: f32,
    pub host_cpu: f32,
    pub host_ram_mb: f32,
    pub guest_cpu: f32,
    pub guest_used_mb: f32,
    pub guest_total_mb: f32,
    pub jank: f32,
    pub fill_mb: f32,
}

#[derive(Debug, PartialEq, Default)]
pub struct Guest {
    pub cpu: f32,
    pub total_mb: f32,
    pub avail_mb: f32,
    pub procs: Vec<(i32, String, String, f32, f32)>,
}

fn parse_size_mb(s: &str) -> f32 {
    let s = s.trim();
    let (n, mul) = match s.chars().last() {
        Some('G') => (&s[..s.len() - 1], 1024.0),
        Some('M') => (&s[..s.len() - 1], 1.0),
        Some('K') => (&s[..s.len() - 1], 1.0 / 1024.0),
        _ => (s, 1.0 / 1024.0 / 1024.0),
    };
    n.parse::<f32>().unwrap_or(0.0) * mul
}

/// Interpreta `head -3 /proc/meminfo; top -b -n 1 -o PID,USER,%CPU,RES,NAME`.
pub fn parse_guest(out: &str) -> Guest {
    let mut g = Guest::default();
    let mut in_table = false;
    for l in out.lines() {
        let t = l.trim();
        if let Some(v) = t.strip_prefix("MemTotal:") {
            g.total_mb = v.trim().trim_end_matches("kB").trim().parse::<f32>().unwrap_or(0.0) / 1024.0;
        } else if let Some(v) = t.strip_prefix("MemAvailable:") {
            g.avail_mb = v.trim().trim_end_matches("kB").trim().parse::<f32>().unwrap_or(0.0) / 1024.0;
        } else if t.contains("%cpu") && t.contains("%idle") {
            let num = |key: &str| t.split_whitespace().find(|w| w.ends_with(key)).and_then(|w| w.trim_end_matches(key).parse::<f32>().ok()).unwrap_or(0.0);
            let total = num("%cpu").max(1.0);
            g.cpu = ((total - num("%idle")) / total * 100.0).clamp(0.0, 100.0);
        } else if t.starts_with("PID") {
            in_table = true;
        } else if in_table {
            let f: Vec<&str> = t.split_whitespace().collect();
            if f.len() >= 5 {
                if let Ok(pid) = f[0].parse::<i32>() {
                    let name = f[4..].join(" ");
                    if name == "top" || name.starts_with("sh") && f[1] == "shell" {
                        continue;
                    }
                    g.procs.push((pid, name, f[1].to_string(), f[2].parse().unwrap_or(0.0), parse_size_mb(f[3])));
                }
            }
        }
    }
    g
}

/// (jank %, texto) de `dumpsys gfxinfo <pkg>`.
pub fn parse_gfx(out: &str) -> Option<(f32, String)> {
    let mut total = 0u64;
    let mut jank = 0.0f32;
    let mut p50 = String::new();
    let mut p90 = String::new();
    let mut p99 = String::new();
    for l in out.lines().map(str::trim) {
        if let Some(v) = l.strip_prefix("Total frames rendered:") {
            total = v.trim().parse().unwrap_or(0);
        } else if l.starts_with("Janky frames:") {
            jank = l.split('(').nth(1).and_then(|s| s.split('%').next()).and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);
        } else if let Some(v) = l.strip_prefix("50th percentile:") {
            p50 = v.trim().into();
        } else if let Some(v) = l.strip_prefix("90th percentile:") {
            p90 = v.trim().into();
        } else if let Some(v) = l.strip_prefix("99th percentile:") {
            p99 = v.trim().into();
        }
    }
    if total == 0 {
        return None;
    }
    Some((jank, format!("{total} frames · p50 {p50} · p90 {p90} · p99 {p99}")))
}

/// Série normalizada (0..1) para o mini gráfico; `max` define o topo.
pub fn spark(values: &VecDeque<f32>, max: f32) -> Vec<f32> {
    values.iter().map(|v| (v / max.max(0.0001)).clamp(0.0, 1.0)).collect()
}

fn fmt_mb(mb: f32) -> String {
    if mb >= 1024.0 {
        format!("{:.2} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

fn serial_and_pid(ctx: &Ctx) -> Option<(String, u32, String)> {
    let id = ctx.player.lock().unwrap().as_ref()?.id.clone();
    let r = AvdBackend::find_running(&id)?;
    Some((r.serial(), r.pid, id))
}

/// RSS total (MB) dos processos `sort` usados para encher a memória.
fn filled_mb(serial: &str) -> f32 {
    emuzip_adb::shell(serial, "ps -A -o RSS,USER,NAME | grep ' shell .*sort$'")
        .map(|o| o.lines().filter_map(|l| l.split_whitespace().next()?.parse::<f32>().ok()).sum::<f32>() / 1024.0)
        .unwrap_or(0.0)
}

pub fn start(ctx: &Ctx) {
    let mut st = ctx.perf.lock().unwrap();
    if st.running.is_some() {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    st.running = Some(stop.clone());
    st.samples.clear();
    drop(st);
    let c = ctx.clone();
    ctx.blocking(move |_| collect_loop(c, stop));
}

pub fn stop(ctx: &Ctx) {
    if let Some(s) = ctx.perf.lock().unwrap().running.take() {
        s.store(true, Ordering::Relaxed);
    }
}

fn collect_loop(ctx: Ctx, stop: Arc<AtomicBool>) {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    let ncpu = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f32;
    let mut series: [VecDeque<f32>; 5] = Default::default();
    let t0 = Instant::now();
    let mut tick = 0u32;
    let mut gpu: (f32, String) = (0.0, "—".into());
    let mut fill = 0.0f32;
    let mut lmk = 0usize;
    while !stop.load(Ordering::Relaxed) {
        let Some((serial, pid, _)) = serial_and_pid(&ctx) else {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        };
        // Host: processo qemu do emulador.
        let p = Pid::from_u32(pid);
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[p]), true, ProcessRefreshKind::nothing().with_cpu().with_memory());
        let (host_cpu, host_ram) = sys.process(p).map(|pr| (pr.cpu_usage() / ncpu, pr.memory() as f32 / 1048576.0)).unwrap_or((0.0, 0.0));
        // Android: CPU, memória e processos.
        let sort_ram = ctx.perf.lock().unwrap().sort_ram;
        let cmd = format!("head -3 /proc/meminfo; top -b -n 1 -m 12 -o PID,USER,%CPU,RES,NAME -s {}", if sort_ram { 4 } else { 3 });
        let g = parse_guest(&emuzip_adb::shell(&serial, &cmd).unwrap_or_default());
        // GPU e app em primeiro plano a cada 3 s.
        if tick % 3 == 0 {
            let act = emuzip_adb::shell(&serial, "dumpsys activity activities | grep -m1 -E 'topResumedActivity|mResumedActivity'").unwrap_or_default();
            let pkg = act.split_whitespace().find(|w| w.contains('/')).and_then(|w| w.split('/').next()).unwrap_or("").to_string();
            if !pkg.is_empty() {
                let out = emuzip_adb::shell(&serial, &format!("dumpsys gfxinfo {pkg}; dumpsys gfxinfo {pkg} reset >/dev/null")).unwrap_or_default();
                gpu = parse_gfx(&out).unwrap_or((0.0, tr!("no new frames (static screen)").into()));
                ctx.perf.lock().unwrap().fg_pkg = pkg;
            }
            fill = filled_mb(&serial);
            lmk = ctx
                .player
                .lock()
                .unwrap()
                .as_ref()
                .map(|s| {
                    s.logcat.lock().unwrap().iter().filter(|l| l.tag.contains("lowmemorykiller") || (l.tag.contains("lmkd") && l.text.contains("Kill"))).count()
                })
                .unwrap_or(0);
        }
        tick += 1;
        let used = (g.total_mb - g.avail_mb).max(0.0);
        let vals = [host_cpu, host_ram, g.cpu, used, gpu.0];
        for (s, v) in series.iter_mut().zip(vals) {
            s.push_back(v);
            if s.len() > HISTORY {
                s.pop_front();
            }
        }
        let host_max = series[1].iter().cloned().fold(256.0, f32::max) * 1.15;
        let paths =
            [spark(&series[0], 100.0), spark(&series[1], host_max), spark(&series[2], 100.0), spark(&series[3], g.total_mb.max(1.0)), spark(&series[4], 100.0)];
        let fg = ctx.perf.lock().unwrap().fg_pkg.clone();
        ctx.perf.lock().unwrap().samples.push(Sample {
            t: t0.elapsed().as_secs_f32(),
            host_cpu,
            host_ram_mb: host_ram,
            guest_cpu: g.cpu,
            guest_used_mb: used,
            guest_total_mb: g.total_mb,
            jank: gpu.0,
            fill_mb: fill,
        });
        let procs: Vec<ProcItem> = g
            .procs
            .iter()
            .take(10)
            .map(|(pid, name, user, cpu, ram)| ProcItem { pid: *pid, name: name.into(), user: user.into(), cpu: *cpu, ram: fmt_mb(*ram).into(), ram_mb: *ram })
            .collect();
        let gtext = gpu.1.clone();
        let (gj, total) = (gpu.0, g.total_mb);
        ctx.ui(move |ui| {
            let p = ui.global::<Perf>();
            p.set_host_cpu(host_cpu);
            p.set_host_ram_mb(host_ram);
            p.set_host_ram(fmt_mb(host_ram).into());
            p.set_guest_cpu(g.cpu);
            p.set_guest_ram_used(used);
            p.set_guest_ram_total(total);
            p.set_guest_ram(format!("{} / {}", fmt_mb(used), fmt_mb(total)).into());
            p.set_gpu_jank(gj);
            p.set_gpu_text(gtext.into());
            p.set_fg_app(fg.into());
            p.set_host_cpu_series(model(paths[0].clone()));
            p.set_host_ram_series(model(paths[1].clone()));
            p.set_guest_cpu_series(model(paths[2].clone()));
            p.set_guest_ram_series(model(paths[3].clone()));
            p.set_gpu_series(model(paths[4].clone()));
            p.set_procs(model(procs));
            p.set_filled_mb(fill.round() as i32);
            p.set_lmk_kills(lmk as i32);
        });
        std::thread::sleep(Duration::from_millis(1000));
    }
}

fn fill(ctx: &Ctx, ui: &MainWindow) {
    let Some((serial, ..)) = serial_and_pid(ctx) else { return };
    let target = ui.global::<Perf>().get_fill_mb().max(32.0);
    ui.global::<Perf>().set_filling(true);
    let task = ctx.task(tr!("Filling {} MB of Android RAM", target.round()), None);
    ctx.blocking(move |c| {
        let start = filled_mb(&serial);
        let mut now = start;
        let mut guard = 0;
        // Cada bloco: `sort` guarda toda a entrada na memória enquanto o `sleep` impede o EOF.
        while now - start < target && guard < 200 {
            let s2 = serial.clone();
            std::thread::spawn(move || {
                let _ = emuzip_adb::shell(&s2, &format!("(head -c {} /dev/zero; sleep 86400) | sort > /dev/null", FILL_CHUNK_MB as u64 * 1048576));
            });
            std::thread::sleep(Duration::from_millis(900));
            now = filled_mb(&serial);
            guard += 1;
            task.progress(((now - start) / target).clamp(0.0, 1.0), tr!("{} of {} MB used", format!("{:.0}", now - start), format!("{:.0}", target)));
        }
        task.done(tr!("{} MB used on Android", format!("{:.0}", now - start)));
        c.toast_ok(tr!("Memory used: {} MB. Watch apps being killed by the LMK.", format!("{:.0}", now)));
        c.ui(|ui| ui.global::<Perf>().set_filling(false));
    });
}

fn release(ctx: &Ctx) {
    let Some((serial, ..)) = serial_and_pid(ctx) else { return };
    ctx.blocking(move |c| {
        let _ = emuzip_adb::shell(&serial, "pkill -u shell sort; pkill -u shell -f 'sleep 86400'");
        c.toast_ok(tr!("Memory released"));
    });
}

fn export_csv(ctx: &Ctx) {
    let rows = ctx.perf.lock().unwrap().samples.clone();
    if rows.is_empty() {
        ctx.toast(0, tr!("No samples yet"));
        return;
    }
    let Some(dest) = rfd::FileDialog::new().set_file_name("emuzip-desempenho.csv").add_filter("CSV", &["csv"]).save_file() else { return };
    let mut s = String::from("segundos,cpu_host_pct,ram_host_mb,cpu_android_pct,ram_android_usada_mb,ram_android_total_mb,gpu_jank_pct,memoria_ocupada_mb\n");
    for r in &rows {
        s += &format!(
            "{:.1},{:.1},{:.0},{:.1},{:.0},{:.0},{:.1},{:.0}\n",
            r.t, r.host_cpu, r.host_ram_mb, r.guest_cpu, r.guest_used_mb, r.guest_total_mb, r.jank, r.fill_mb
        );
    }
    match std::fs::write(&dest, s) {
        Ok(_) => ctx.toast_ok(tr!("{} samples exported", rows.len())),
        Err(e) => ctx.toast_err(e.to_string()),
    }
}

pub fn wire(ctx: &Ctx, ui: &MainWindow) {
    let api = ui.global::<Api>();
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_perf_open(move || {
        // Imagens sem root usam o `sort` de qualquer forma; trim fica como alternativa.
        w.unwrap().global::<Perf>().set_rooted(true);
        start(&c);
    });
    let (c, w) = (ctx.clone(), ui.as_weak());
    api.on_perf_fill(move || fill(&c, &w.unwrap()));
    let c = ctx.clone();
    api.on_perf_release(move || release(&c));
    let c = ctx.clone();
    api.on_perf_export(move || export_csv(&c));
    let c = ctx.clone();
    api.on_perf_trim(move |level| {
        let Some((serial, ..)) = serial_and_pid(&c) else { return };
        let pkg = c.perf.lock().unwrap().fg_pkg.clone();
        let level = level.to_string();
        c.blocking(move |c| {
            let r = emuzip_adb::shell(&serial, &format!("am send-trim-memory {pkg} {level}"));
            match r {
                Ok(o) if o.trim().is_empty() => c.toast_ok(tr!("{} sent to {}", level, pkg)),
                Ok(o) => c.toast_err(o.trim().to_string()),
                Err(e) => c.toast_err(format!("{e:#}")),
            }
        });
    });
    let c = ctx.clone();
    api.on_perf_kill(move |pid, name| {
        let Some((serial, ..)) = serial_and_pid(&c) else { return };
        let name = name.to_string();
        let cmd = if name.contains('.') && !name.contains('/') { format!("am force-stop {name}") } else { format!("kill -9 {pid}") };
        c.confirm(
            &tr!("Kill {}?", name),
            tr!("The process will be terminated immediately on Android."),
            true,
            Box::new(move |c| {
                let r = emuzip_adb::shell(&serial, &cmd);
                match r {
                    Ok(o) if o.contains("not permitted") || o.contains("denied") => c.toast_err(tr!("No permission to kill {} (system process)", name)),
                    Ok(_) => c.toast_ok(tr!("{} killed", name)),
                    Err(e) => c.toast_err(format!("{e:#}")),
                }
            }),
        );
    });
    // Ordenação lida pelo coletor.
    let (c, w) = (ctx.clone(), ui.as_weak());
    let timer = Box::leak(Box::new(slint::Timer::default()));
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(500), move || {
        if let Some(ui) = w.upgrade() {
            let p = ui.global::<crate::Player>();
            c.perf.lock().unwrap().sort_ram = ui.global::<Perf>().get_sort_ram();
            // Para a coleta quando o painel fecha ou o player sai.
            if p.get_panel() != "perf" || ui.global::<crate::Store>().get_page() != 4 {
                stop(&c);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    const TOP: &str = "MemTotal:        4013824 kB\nMemFree:          251992 kB\nMemAvailable:    2132336 kB\nTasks: 362 total\n  Mem:  4013824K total\n400%cpu   3%user   0%nice   6%sys 391%idle   0%iow\n  PID USER        [%CPU] RES NAME\n13954 shell         8.9 4.9M top\n  541 system        1.4  31M surfaceflinger\n  691 system        2.0 1.2G system_server\n";

    #[test]
    fn guest() {
        let g = parse_guest(TOP);
        assert!((g.total_mb - 3919.75).abs() < 0.1);
        assert!((g.cpu - 2.25).abs() < 0.01);
        assert_eq!(g.procs.len(), 2);
        assert_eq!(g.procs[0].1, "surfaceflinger");
        assert!((g.procs[1].4 - 1228.8).abs() < 0.1);
    }

    #[test]
    fn gfx() {
        let (j, t) =
            parse_gfx("Total frames rendered: 1801\nJanky frames: 1559 (86.56%)\n50th percentile: 109ms\n90th percentile: 250ms\n99th percentile: 400ms\n")
                .unwrap();
        assert!((j - 86.56).abs() < 0.01);
        assert!(t.contains("p90 250ms"));
        assert!(parse_gfx("nada").is_none());
    }

    #[test]
    fn spark_norm() {
        let v: VecDeque<f32> = [0.0, 50.0, 200.0].into_iter().collect();
        assert_eq!(spark(&v, 100.0), vec![0.0, 0.5, 1.0]);
    }
}
