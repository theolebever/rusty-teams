use calling::{CallCommand, CallState};
use rune::*;

use crate::bridge::Bridge;
use crate::calls::CallView;
use crate::types::{Actions, Update};
use crate::ui::UiState;

pub struct CallWindow {
    app: AppHandle<UiState<Bridge>>,
    shown: bool,
}

impl CallWindow {
    pub fn new(app: AppHandle<UiState<Bridge>>) -> Self {
        Self { app, shown: false }
    }

    pub fn open(&self) {
        self.app.open();
    }

    pub fn forward(&self, update: &Update) {
        if matches!(
            update,
            Update::Call(_) | Update::Ring(_) | Update::Appearance(_)
        ) {
            let update = update.clone();
            self.app.update(move |state, _| state.apply(update));
        }
    }

    pub fn video(&self, frames: Vec<(calling::VideoKey, std::sync::Arc<calling::VideoPicture>)>) {
        self.app
            .update(move |state, _| state.calls.apply_video(frames));
    }

    pub fn sync(&mut self, call: &CallView) {
        let wanted = needs_window(call);
        if wanted != self.shown {
            if wanted {
                self.app.open();
            } else {
                self.app.hide();
            }
            self.shown = wanted;
        }
    }
}

fn needs_window(call: &CallView) -> bool {
    call.state.as_ref().is_some_and(CallState::is_active) || call.ring.is_some()
}

pub fn view<A: Actions + 'static>(state: &UiState<A>, cx: &mut Cx<UiState<A>>) -> El {
    crate::theme::apply(cx, &state.appearance);
    crate::calls::view(state)
}

pub fn closed<A: Actions>(state: &mut UiState<A>, _: &mut Tx) {
    if state.calls.state.as_ref().is_some_and(CallState::is_active) || state.calls.pending {
        state.actions.call_command(CallCommand::Hangup);
    }
    if let Some(ring) = &state.calls.ring {
        state.actions.decline_call(ring.ring_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calling::{CallUpdate, EndReason};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        commands: RefCell<Vec<CallCommand>>,
    }
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn call_command(&self, command: CallCommand) {
            self.commands.borrow_mut().push(command);
        }
    }

    #[test]
    fn calls_leave_the_main_conversation_selected() {
        let mut state = UiState::new(Fake::default());
        let app = App::new(AppSpec::new(
            "Unused",
            (800.0, 600.0),
            || panic!("this app is never started"),
            view::<Bridge>,
        ));
        state.call_window = Some(CallWindow::new(app.handle()));
        state.screens.screen = crate::screens::Screen::Chat;
        state.apply(Update::Call(CallUpdate::State(CallState::Connecting)));
        assert!(state.screens.screen == crate::screens::Screen::Chat);
    }

    #[test]
    fn failed_calls_still_show_a_notice_in_the_main_window() {
        let mut state = UiState::new(Fake::default());
        let app = App::new(AppSpec::new(
            "Unused",
            (800.0, 600.0),
            || panic!("this app is never started"),
            view::<Bridge>,
        ));
        state.call_window = Some(CallWindow::new(app.handle()));
        state.apply(Update::Call(CallUpdate::State(CallState::Ended {
            reason: EndReason::Failed("Could not connect the call".into()),
        })));
        let mut runtime = Runtime::new(
            state,
            crate::ui::view::<Fake>,
            (1180.0, 760.0),
            Env {
                system_dark: false,
                scale: 1.0,
            },
        );
        runtime.frame(1.0);
        assert!(runtime.find("toast").is_some());
    }

    #[test]
    fn dedicated_view_has_call_controls_without_the_chat_composer() {
        let mut state = UiState::new(Fake::default());
        state.calls.apply(CallUpdate::State(CallState::Connecting));
        let mut runtime = Runtime::new(
            state,
            view::<Fake>,
            (1180.0, 760.0),
            Env {
                system_dark: false,
                scale: 1.0,
            },
        );
        runtime.frame(1.0);
        let (x, y) = runtime.find("call-leave").expect("dedicated call controls");
        assert!(runtime.find("composer").is_none());
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(2.0);
        assert_eq!(
            *runtime.state.actions.commands.borrow(),
            [CallCommand::Hangup]
        );
    }

    #[test]
    fn accepting_an_incoming_call_keeps_its_window_open() {
        let mut call = CallView::default();
        call.apply_ring(calling::EngineEvent::Incoming(calling::IncomingRing {
            ring_id: 7,
            caller: calling::Caller {
                mri: "8:orgid:peer".into(),
                display_name: "Peer".into(),
            },
            thread_id: None,
            video: false,
            is_group: false,
            subject: None,
        }));
        assert!(needs_window(&call));
        call.apply(CallUpdate::State(CallState::Connecting));
        call.apply_ring(calling::EngineEvent::RingEnded {
            ring_id: 7,
            kind: calling::EndKind::Normal,
            answered_by: None,
        });
        assert!(needs_window(&call));
        call.apply(CallUpdate::State(CallState::Ended {
            reason: EndReason::LocalHangup,
        }));
        assert!(!needs_window(&call));
    }

    #[test]
    fn focusing_the_call_does_not_mark_the_main_chat_as_read() {
        let mut state = UiState::new(Fake::default());
        state.screens.screen = crate::screens::Screen::Chat;
        state.selected = Some("chat".into());
        state.window_focused = true;
        assert_eq!(state.foreground_chat(), Some("chat"));
        state.window_focused = false;
        assert_eq!(state.foreground_chat(), None);
    }

    #[test]
    fn window_lives_from_dial_until_hangup() {
        let mut call = CallView::default();
        assert!(!needs_window(&call));
        call.apply(CallUpdate::State(CallState::Connecting));
        assert!(needs_window(&call));
        call.apply(CallUpdate::State(CallState::Ended {
            reason: EndReason::LocalHangup,
        }));
        assert!(!needs_window(&call));
    }

    #[test]
    fn closing_the_call_window_hangs_up_without_quitting_the_app() {
        let mut state = UiState::new(Fake::default());
        state.calls.apply(CallUpdate::State(CallState::Connecting));
        closed(&mut state, &mut Tx::default());
        assert_eq!(*state.actions.commands.borrow(), [CallCommand::Hangup]);
        state.calls.apply(CallUpdate::State(CallState::Ended {
            reason: EndReason::LocalHangup,
        }));
        closed(&mut state, &mut Tx::default());
        assert_eq!(state.actions.commands.borrow().len(), 1);
    }
}
