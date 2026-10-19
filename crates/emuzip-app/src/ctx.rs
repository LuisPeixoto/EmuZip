//! Contexto compartilhado: UI, runtime, tarefas, toasts e ações pendentes.
use crate::{MainWindow, Store, TaskItem, ToastItem};
use emuzip_backend::avd::AvdBackend;
use emuzip_backend::Backend;
use emuzip_core::{Device, Settings};
use slint::{ComponentHandle, Model, ModelRc, VecModel, Weak};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

pub type Job = Box<dyn FnOnce(&Ctx) + Send>;
pub type PromptJob = Box<dyn FnOnce(&Ctx, String) + Send>;

#[derive(Clone)]
pub struct Ctx(pub Arc<Inner>);

pub struct Inner {
    pub ui: Weak<MainWindow>,
    pub rt: tokio::runtime::Handle,
    pub backends: RwLock<Arc<Vec<Box<dyn Backend>>>>,
    pub settings: RwLock<Settings>,
    pub devices: Mutex<Vec<Device>>,
    pub busy: Mutex<HashMap<String, String>>,
    pub boot_started: Mutex<HashMap<String, Instant>>,
    pub tasks: Mutex<Vec<Task>>,
    pub task_seq: AtomicI32,
    pub toast_seq: AtomicI32,
    pub toast_actions: Mutex<HashMap<i32, Job>>,
    pub confirm: Mutex<Option<Job>>,
    pub prompt: Mutex<Option<PromptJob>>,
    pub catalog: Mutex<Vec<emuzip_repo::RemoteImage>>,
    pub downloads: Mutex<HashMap<String, Download>>,
    pub player: Mutex<Option<crate::player::Session>>,
    pub perf: Mutex<crate::perf::PerfState>,
    pub net: Mutex<crate::network::NetState>,
}

#[derive(Clone)]
pub struct Download {
    pub progress: f32,
    pub status: String,
    pub cancel: Arc<AtomicBool>,
}

pub struct Task {
    pub id: i32,
    pub title: String,
    pub detail: String,
    pub progress: f32,
    pub state: i32,
    pub cancel: Option<Arc<AtomicBool>>,
    pub started: Instant,
    pub ended: Option<Instant>,
}

impl std::ops::Deref for Ctx {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

/// Atualiza um VecModel linha a linha (mantém componentes, popups e scroll).
pub fn sync_model<T: Clone + PartialEq + 'static>(m: ModelRc<T>, new: Vec<T>) {
    if let Some(vm) = m.as_any().downcast_ref::<VecModel<T>>() {
        if vm.row_count() == new.len() {
            for (i, v) in new.into_iter().enumerate() {
                if vm.row_data(i).as_ref() != Some(&v) {
                    vm.set_row_data(i, v);
                }
            }
        } else {
            vm.set_vec(new);
        }
    }
}

pub fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

fn ago(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}min", s / 60)
    } else {
        format!("{}h", s / 3600)
    }
}

impl Ctx {
    /// Executa `f` na thread da UI.
    pub fn ui(&self, f: impl FnOnce(&MainWindow) + Send + 'static) {
        let _ = self.ui.upgrade_in_event_loop(move |ui| f(&ui));
    }

    pub fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.rt.spawn(fut);
    }

    /// Roda código bloqueante numa thread do pool.
    pub fn blocking(&self, f: impl FnOnce(Ctx) + Send + 'static) {
        let c = self.clone();
        self.rt.spawn_blocking(move || f(c));
    }

    pub fn backends(&self) -> Arc<Vec<Box<dyn Backend>>> {
        self.backends.read().unwrap().clone()
    }

    pub fn avd(&self) -> Option<AvdBackend> {
        AvdBackend::detect()
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn device(&self, id: &str) -> Option<Device> {
        self.devices.lock().unwrap().iter().find(|d| d.id == id).cloned()
    }

    // ---------- Tarefas (Central de atividades) ----------

    pub fn task(&self, title: impl Into<String>, cancel: Option<Arc<AtomicBool>>) -> TaskHandle {
        let id = self.task_seq.fetch_add(1, Ordering::Relaxed) + 1;
        self.tasks.lock().unwrap().insert(
            0,
            Task { id, title: title.into(), detail: tr!("Preparing…").into(), progress: -1.0, state: 0, cancel, started: Instant::now(), ended: None },
        );
        self.publish_tasks();
        TaskHandle { ctx: self.clone(), id }
    }

    pub fn publish_tasks(&self) {
        let (items, running): (Vec<TaskItem>, i32) = {
            let t = self.tasks.lock().unwrap();
            (
                t.iter()
                    .map(|t| TaskItem {
                        id: t.id,
                        title: t.title.clone().into(),
                        detail: t.detail.clone().into(),
                        progress: t.progress,
                        state: t.state,
                        cancellable: t.cancel.is_some(),
                        time: ago(t.ended.unwrap_or_else(Instant::now) - t.started).into(),
                    })
                    .collect(),
                t.iter().filter(|t| t.state == 0).count() as i32,
            )
        };
        self.ui(move |ui| {
            let s = ui.global::<Store>();
            sync_model(s.get_tasks(), items);
            s.set_running_tasks(running);
        });
    }

    pub fn cancel_task(&self, id: i32) {
        if let Some(t) = self.tasks.lock().unwrap().iter().find(|t| t.id == id) {
            if let Some(c) = &t.cancel {
                c.store(true, Ordering::Relaxed);
            }
        }
    }

    pub fn clear_tasks(&self) {
        self.tasks.lock().unwrap().retain(|t| t.state == 0);
        self.publish_tasks();
    }

    // ---------- Toasts ----------

    pub fn toast(&self, kind: i32, text: impl Into<String>) {
        self.toast_with(kind, text, "", None);
    }

    pub fn toast_ok(&self, text: impl Into<String>) {
        self.toast(1, text);
    }

    pub fn toast_err(&self, text: impl Into<String>) {
        self.toast(2, text);
    }

    pub fn toast_with(&self, kind: i32, text: impl Into<String>, action: impl AsRef<str>, job: Option<Job>) {
        let id = self.toast_seq.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(j) = job {
            self.toast_actions.lock().unwrap().insert(id, j);
        }
        let item = ToastItem { id, text: text.into().into(), kind, action: action.as_ref().into() };
        self.ui(move |ui| {
            let m = ui.global::<Store>().get_toasts();
            if let Some(vm) = m.as_any().downcast_ref::<VecModel<ToastItem>>() {
                vm.push(item);
                while vm.row_count() > 4 {
                    vm.remove(0);
                }
            }
        });
        let c = self.clone();
        let secs = if kind == 2 { 9 } else { 5 };
        self.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            c.dismiss_toast(id);
        });
    }

    pub fn dismiss_toast(&self, id: i32) {
        self.toast_actions.lock().unwrap().remove(&id);
        self.ui(move |ui| {
            let m = ui.global::<Store>().get_toasts();
            if let Some(vm) = m.as_any().downcast_ref::<VecModel<ToastItem>>() {
                if let Some(i) = (0..vm.row_count()).find(|i| vm.row_data(*i).map(|t| t.id) == Some(id)) {
                    vm.remove(i);
                }
            }
        });
    }

    pub fn toast_action(&self, id: i32) {
        let job = self.toast_actions.lock().unwrap().remove(&id);
        if let Some(j) = job {
            j(self);
        }
        self.dismiss_toast(id);
    }

    // ---------- Confirmação e prompt ----------

    pub fn confirm(&self, title: impl AsRef<str>, text: impl AsRef<str>, danger: bool, job: Job) {
        if !self.settings().confirm_destructive && danger {
            job(self);
            return;
        }
        *self.confirm.lock().unwrap() = Some(job);
        let (t, x) = (title.as_ref().to_string(), text.as_ref().to_string());
        self.ui(move |ui| {
            let s = ui.global::<Store>();
            s.set_confirm_title(t.into());
            s.set_confirm_text(x.into());
            s.set_confirm_danger(danger);
            s.set_confirm_open(true);
        });
    }

    pub fn run_confirm(&self) {
        let job = self.confirm.lock().unwrap().take();
        if let Some(j) = job {
            j(self);
        }
    }

    pub fn prompt(&self, title: impl AsRef<str>, label: impl AsRef<str>, value: impl AsRef<str>, job: PromptJob) {
        *self.prompt.lock().unwrap() = Some(job);
        let (t, l, v) = (title.as_ref().to_string(), label.as_ref().to_string(), value.as_ref().to_string());
        self.ui(move |ui| {
            let s = ui.global::<Store>();
            s.set_prompt_title(t.into());
            s.set_prompt_label(l.into());
            s.set_prompt_value(v.into());
            s.set_prompt_open(true);
        });
    }

    pub fn run_prompt(&self, value: String) {
        let job = self.prompt.lock().unwrap().take();
        if let Some(j) = job {
            j(self, value);
        }
    }

    pub fn set_busy(&self, id: &str, text: Option<String>) {
        {
            let mut b = self.busy.lock().unwrap();
            match text {
                Some(t) => b.insert(id.to_string(), t),
                None => b.remove(id),
            };
        }
        crate::devices::publish(self);
    }
}

pub struct TaskHandle {
    ctx: Ctx,
    pub id: i32,
}

impl TaskHandle {
    fn update(&self, f: impl FnOnce(&mut Task)) {
        if let Some(t) = self.ctx.tasks.lock().unwrap().iter_mut().find(|t| t.id == self.id) {
            f(t);
        }
        self.ctx.publish_tasks();
    }

    /// progress < 0 = indeterminado.
    pub fn progress(&self, p: f32, detail: impl Into<String>) {
        let d = detail.into();
        self.update(|t| {
            t.progress = p;
            t.detail = d;
        });
    }

    pub fn done(&self, detail: impl Into<String>) {
        let d = detail.into();
        self.update(|t| {
            t.state = 1;
            t.progress = 1.0;
            t.detail = d;
            t.ended = Some(Instant::now());
        });
    }

    pub fn fail(&self, detail: impl Into<String>) {
        let d = detail.into();
        let cancelled = d.contains("cancelado");
        self.update(|t| {
            t.state = if cancelled { 3 } else { 2 };
            t.detail = d;
            t.ended = Some(Instant::now());
        });
    }

    /// Conclui a tarefa a partir de um Result, com toast.
    pub fn finish<T>(&self, r: &anyhow::Result<T>, ok_msg: &str) {
        match r {
            Ok(_) => {
                self.done(ok_msg);
                self.ctx.toast_ok(ok_msg);
            }
            Err(e) => {
                let m = format!("{e:#}");
                self.fail(&m);
                if !m.contains("cancelado") {
                    self.ctx.toast_err(m);
                }
            }
        }
    }
}
