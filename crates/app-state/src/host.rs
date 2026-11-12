//! The seam between `AppState` and the UI toolkit that drives it.
//!
//! State methods take `cx: &mut dyn Host` instead of a GPUI `Context`. GPUI's
//! `Context<AppState>` implements `Host`, so view code keeps passing its `cx`.
//! A host without an entity system (Rune) implements it on a plain struct.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

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

/// Posts jobs to the UI thread. Cheap to clone, not `Send`.
#[derive(Clone)]
pub struct StateHandle(Rc<dyn Fn(Job) -> bool>);

impl StateHandle {
    /// `post` runs the job on the UI thread, now or later, and returns false once the state is gone.
    pub fn new(post: impl Fn(Job) -> bool + 'static) -> Self {
        Self(Rc::new(post))
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
        if !(self.0)(job) {
            return None;
        }
        receiver.await.ok()
    }
}

mod gpui_host {
    use gpui_kit::{App, ClipboardItem, Context};

    use super::{Host, LocalTask, StateHandle};
    use crate::app_state::{AppEvent, AppState};

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
            let this = self.weak_entity();
            let cx = self.to_async();
            StateHandle::new(move |job| {
                let mut cx = cx.clone();
                this.update(&mut cx, |state, cx| job(state, cx)).is_ok()
            })
        }

        fn open_url(&mut self, url: &str) {
            App::open_url(self, url);
        }

        fn write_clipboard(&mut self, text: String) {
            App::write_to_clipboard(self, ClipboardItem::new_string(text));
        }
    }
}
