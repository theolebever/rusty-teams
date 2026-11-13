use calling::{CallCommand, CallState};
use rune::*;

use crate::bridge::Bridge;
use crate::calls::CallView;
use crate::types::{Actions, Update};
use crate::ui::UiState;

pub struct CallWindow {
    app: AppHandle<UiState<Bridge>>,
    shown: bool,
    /// The prejoin choices went to the call window for the call now starting.
    prepared: bool,
}

impl CallWindow {
    pub fn new(app: AppHandle<UiState<Bridge>>) -> Self {
        Self { app, shown: false, prepared: false }
    }

    pub fn open(&self) {
        self.app.open();
    }

    pub fn shortcut(&self, action: crate::calls::Shortcut) {
        if action == crate::calls::Shortcut::Share {
            self.app.open();
        }
        self.app.update(move |state, _| {
            crate::calls::shortcut(state, action);
        });
    }

    pub fn forward(&self, update: &Update) {
        if matches!(
            update,
            Update::Call(_)
                | Update::CallIntro(_)
                | Update::RecordingRights { .. }
                | Update::Ring(_)
                | Update::Appearance(_)
                | Update::People { .. }
                | Update::Me { .. }
                | Update::Details(_)
                // The chat panel beside the call: its messages, pictures and unread state.
                | Update::Messages { .. }
                | Update::Chats(_)
                | Update::Image { .. }
                | Update::ImagePreview { .. }
                | Update::ImageFailed { .. }
        ) {
            let update = update.clone();
            if matches!(
                &update,
                Update::Call(calling::CallUpdate::State(CallState::Connecting))
                    | Update::Ring(calling::EngineEvent::Incoming(_))
            ) {
                self.app.open();
            }
            self.app.update(move |state, _| {
                let ended = matches!(
                    &update,
                    Update::Call(calling::CallUpdate::State(CallState::Ended { .. }))
                );
                state.apply(update);
                // The main window plays the call chimes.
                state.calls.tones.clear();
                if ended {
                    state.calls.retry = state.actions.retry_call();
                }
            });
        }
    }

    pub fn video(&self, frames: Vec<(calling::VideoKey, std::sync::Arc<calling::VideoPicture>)>) {
        self.app
            .update(move |state, _| state.calls.apply_video(frames));
    }

    pub fn sync(&mut self, call: &CallView) {
        // An ended call keeps the window, so copy on each start rather than on the window edge.
        let starting = matches!(call.state, Some(CallState::Connecting)) || call.ring.is_some();
        if starting && !self.prepared {
            let camera = call.selected_camera.clone();
            let blur = call.background_blur;
            let effects = call.effects;
            self.app.update(move |state, _| {
                state.calls.selected_camera = camera;
                state.calls.background_blur = blur;
                state.calls.effects = effects;
            });
        }
        self.prepared = starting;
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
    call.state
        .as_ref()
        .is_some_and(|state| !matches!(state, CallState::Idle))
        || call.ring.is_some()
}

pub fn view<A: Actions + 'static>(state: &UiState<A>, cx: &mut Cx<UiState<A>>) -> El {
    crate::theme::apply(cx, &state.appearance);
    crate::calls::view_with_motion(state, cx.is_reduced_motion())
}

pub fn closed<A: Actions>(state: &mut UiState<A>, _: &mut Tx) {
    if state.calls.state.as_ref().is_some_and(CallState::is_active) || state.calls.pending {
        state.actions.call_command(CallCommand::Hangup);
    }
    if let Some(ring) = &state.calls.ring {
        state.actions.decline_call(ring.ring_id);
    }
    if matches!(state.calls.state, Some(CallState::Ended { .. })) {
        state.calls.state = None;
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
    fn gallery_and_toolbar_fit_the_window_with_cameras_off() {
        for size in [(1180.0, 760.0), (560.0, 420.0)] {
            let mut state = UiState::new(Fake::default());
            state.calls.apply(CallUpdate::State(CallState::Connecting));
            state
                .calls
                .apply(CallUpdate::Roster(vec![calling::RosterEntry {
                    mri: "ada".into(),
                    display_name: "Ada Example".into(),
                    muted: true,
                    ..Default::default()
                }]));
            let mut rt = Runtime::new(
                state,
                view::<Fake>,
                size,
                Env {
                    system_dark: false,
                    scale: 1.0,
                },
            );
            rt.frame(1.0);
            let ([_, y, width, height], _) = rt
                .presented("call-person-ada")
                .expect("camera-off participant tile");
            assert!(
                width > 200.0 && height > 180.0,
                "tile at {size:?}: {width}x{height}"
            );
            assert!(y >= 72.0);
            for key in ["call-mute", "call-camera", "call-leave", "call-people"] {
                let ([x, y, w, h], _) = rt.presented(key).unwrap();
                assert!(
                    x >= 0.0 && x + w <= size.0 && y + h <= 72.0,
                    "{key} at {size:?}: {x},{y},{w},{h}"
                );
            }
            let (x, y) = rt.find("call-people").unwrap();
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(2.0);
            assert!(rt.find("call-panel").is_some());
        }
    }

    #[test]
    fn screen_share_keeps_its_stage_when_the_participant_panel_opens() {
        let mut state = UiState::new(Fake::default());
        state.calls.apply(CallUpdate::State(CallState::Connecting));
        state.calls.apply(CallUpdate::Camera(true));
        state.calls.apply_video(vec![(
            calling::VideoKey::Screen,
            std::sync::Arc::new(calling::VideoPicture::solid(1280, 720, 0, 0, 0)),
        )]);
        let mut rt = Runtime::new(
            state,
            view::<Fake>,
            (1180.0, 760.0),
            Env {
                system_dark: false,
                scale: 1.0,
            },
        );
        rt.frame(1.0);
        let ([_, _, width, height], _) = rt.presented("call-presentation").unwrap();
        assert!(width > 800.0 && height > 400.0);
        let ([x, y, w, h], _) = rt.presented("call-self").unwrap();
        assert!(w >= 180.0 && h >= 120.0 && x + w <= 1180.0 && y + h <= 760.0);
        let (x, y) = rt.find("call-people").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(2.0);
        assert!(rt.find("call-presentation").is_some());
        assert!(rt.find("call-panel").is_some());
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
        assert!(needs_window(&call));
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
    fn window_lives_from_dial_until_end_screen_is_closed() {
        let mut call = CallView::default();
        assert!(!needs_window(&call));
        call.apply(CallUpdate::State(CallState::Connecting));
        assert!(needs_window(&call));
        call.apply(CallUpdate::State(CallState::Ended {
            reason: EndReason::LocalHangup,
        }));
        assert!(needs_window(&call));
        call.state = None;
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
