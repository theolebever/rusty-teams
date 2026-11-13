//! The seam between `AppState` and the UI toolkit that drives it.
//!
//! State methods take `cx: &mut dyn Host` instead of a GPUI `Context`. GPUI's
//! `Context<AppState>` implements `Host`, so view code keeps passing its `cx`.
//! A host without an entity system (Rune) implements it on a plain struct.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;

use crate::app_state::{AppEvent, AppState};

pub type LocalTask = Pin<Box<dyn Future<Output = ()>>>;
pub type Job = Box<dyn FnOnce(&mut AppState, &mut dyn Host)>;

pub trait Host {
    /// The state changed; views redraw.
    fn notify(&mut self);
    fn emit(&mut self, event: AppEvent);
    /// Runs `task` on the UI thread's executor, detached.
    fn spawn(&mut self, task: LocalTask);
    /// A handle that spawned tasks use to get back to the state.
    fn handle(&mut self) -> StateHandle;
    fn open_url(&mut self, url: &str);
    fn write_clipboard(&mut self, text: String);
}

/// What a host gives spawned tasks: a way back to the state, a timer and a
/// pool for blocking work. GPUI uses its executors, which keeps tests deterministic.
pub trait TaskHost {
    /// Runs the job on the UI thread, now or later. False once the state is gone.
    fn post(&self, job: Job) -> bool;
    fn timer(&self, duration: Duration) -> LocalTask;
    /// Runs `work` off the UI thread; the task ends when it is done.
    fn background(&self, work: Box<dyn FnOnce() + Send>) -> LocalTask;
}

/// Posts jobs to the UI thread. Cheap to clone, not `Send`.
#[derive(Clone)]
pub struct StateHandle(Rc<dyn TaskHost>);

impl StateHandle {
    pub fn new(host: impl TaskHost + 'static) -> Self {
        Self(Rc::new(host))
    }

    pub async fn sleep(&self, duration: Duration) {
        self.0.timer(duration).await;
    }

    /// Runs blocking work (disk, SQLite) off the UI thread.
    pub async fn run_blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.0
            .background(Box::new(move || {
                let _ = sender.send(work());
            }))
            .await;
        receiver.await.ok()
    }

    /// Runs `f` on the UI thread. `None` once the state is gone.
    /// Only call it from a spawned task, never from inside a state update.
    pub async fn update<R: 'static>(
        &self,
        f: impl FnOnce(&mut AppState, &mut dyn Host) -> R + 'static,
    ) -> Option<R> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let job: Job = Box::new(move |state, cx| {
            let _ = sender.send(f(state, cx));
        });
        if !self.0.post(job) {
            return None;
        }
        receiver.await.ok()
    }
}

#[cfg(feature = "gpui")]
pub use gpui_host::follow_state;

#[cfg(feature = "gpui")]
mod gpui_host {
    use std::time::Duration;

    use gpui_kit::{App, AsyncApp, BackgroundExecutor, ClipboardItem, Context, Entity, WeakEntity};

    use super::{Host, Job, LocalTask, StateHandle, TaskHost};
    use crate::app_state::{AgentNote, AppEvent, AppState};
    use crate::{agent_api, task_dialog};

    /// Carries out the events that need GPUI-only parts: the agent API and hosted
    /// task dialogs.
    pub fn follow_state(state: &Entity<AppState>, cx: &mut App) {
        cx.subscribe(state, |state, event: &AppEvent, cx| forward(state, event, cx))
            .detach();
    }

    fn forward(state: Entity<AppState>, event: &AppEvent, cx: &mut App) {
        match event {
            AppEvent::Agents(AgentNote::EngineReady(engine)) => {
                agent_api::engine_ready(engine.clone(), cx)
            }
            AppEvent::Agents(AgentNote::Caption {
                meeting_chat_id,
                caption_id,
                speaker,
                text,
            }) => agent_api::note_caption(meeting_chat_id, caption_id, speaker, text, cx),
            AppEvent::Agents(AgentNote::ClearCaptions(meeting_chat_id)) => {
                agent_api::clear_captions(meeting_chat_id, cx)
            }
            AppEvent::UrlDialog(dialog) => {
                let engine = state.read(cx).engine.clone();
                task_dialog::open_url_dialog(dialog.clone(), engine, cx);
            }
            _ => {}
        }
    }

    impl Host for Context<'_, AppState> {
        fn notify(&mut self) {
            Context::notify(self);
        }

        fn emit(&mut self, event: AppEvent) {
            Context::emit(self, event);
        }

        fn spawn(&mut self, task: LocalTask) {
            App::spawn(self, async move |_| task.await).detach();
        }

        fn handle(&mut self) -> StateHandle {
            StateHandle::new(GpuiTasks {
                state: self.weak_entity(),
                cx: self.to_async(),
                executor: self.background_executor().clone(),
            })
        }

        fn open_url(&mut self, url: &str) {
            App::open_url(self, url);
        }

        fn write_clipboard(&mut self, text: String) {
            App::write_to_clipboard(self, ClipboardItem::new_string(text));
        }
    }

    struct GpuiTasks {
        state: WeakEntity<AppState>,
        cx: AsyncApp,
        executor: BackgroundExecutor,
    }

    impl TaskHost for GpuiTasks {
        fn post(&self, job: Job) -> bool {
            let mut cx = self.cx.clone();
            self.state
                .update(&mut cx, |state, cx| job(state, cx))
                .is_ok()
        }

        fn timer(&self, duration: Duration) -> LocalTask {
            Box::pin(self.executor.timer(duration))
        }

        fn background(&self, work: Box<dyn FnOnce() + Send>) -> LocalTask {
            Box::pin(self.executor.spawn(async move { work() }))
        }
    }
}
