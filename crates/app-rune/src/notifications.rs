//! Desktop notification filtering shared with the existing client.

use app_backend::backend::Engine;
use app_backend::notify::{incoming::{IncomingTracker, LateArrivals}, rules::{self, Environment, Preview}, settings};
use chrono::Utc;

pub const ACTIVATION_SCHEME: &str = "rusty-teams-rune";
pub const ACTIVATION_PREFIX: &str = "rusty-teams-rune://notification/";

pub fn clicked(click: &rune_notify::Click) -> Option<crate::types::Update> {
    let id = click.get("conversation_id").filter(|id| !id.trim().is_empty())?;
    Some(crate::types::Update::OpenChat(id.to_owned()))
}

/// Drains the same pending open request used by a person's profile card.
pub fn open_pending<A: crate::types::Actions>(s: &mut crate::ui::UiState<A>, tx: &mut rune::Tx) -> bool {
    let Some(id) = s.profile.chat_request.take() else { return false };
    crate::screens::navigate(s, crate::screens::Screen::Chat);
    crate::ui::open_chat(s, &id, tx);
    true
}

pub fn environment(engine: &Engine, chat_in_foreground: bool, native_in_call: bool) -> Environment {
    let store = engine.store();
    let presence = store.meta("me_user_id").ok().flatten().and_then(|id| engine.presence(&id));
    let paused = store.meta("notify_paused_until").ok().flatten()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
        .is_some_and(|until| Utc::now() < until);
    Environment {
        chat_in_foreground,
        system_quiet: system_quiet(),
        own_do_not_disturb: presence.is_none_or(|p| p.availability == teams_core::Availability::DoNotDisturb),
        own_busy: presence.is_some_and(|p| p.availability == teams_core::Availability::Busy),
        in_call: native_in_call || presence.is_some_and(|p| p.in_call()),
        paused,
    }
}

#[cfg(windows)]
fn system_quiet() -> bool {
    use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME, QUNS_RUNNING_D3D_FULL_SCREEN};
    // SAFETY: the API has no arguments and reports the current shell notification state.
    unsafe { SHQueryUserNotificationState() }.map_or(true, |state| matches!(state, QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE | QUNS_QUIET_TIME))
}

#[cfg(not(windows))]
fn system_quiet() -> bool { false }

pub struct Notifications {
    tracker: IncomingTracker,
    late: LateArrivals,
}

impl Notifications {
    pub fn new() -> Self {
        Self { tracker: IncomingTracker::new(Utc::now()), late: LateArrivals::default() }
    }

    pub fn mark_late(&mut self, conversation_id: &str, message_ids: Vec<String>) {
        self.late.insert(conversation_id, message_ids);
    }

    pub fn messages(&mut self, engine: &Engine, conversation_id: &str, environment: Environment) {
        let store = engine.store();
        let Ok(sidebar) = engine.sidebar() else { return };
        let me = store.meta("me_user_id").ok().flatten();
        let mut settings = settings::load(store);
        settings.account = teams_core::stored_chat_settings(store).settings().notifications;
        for mut incoming in self.tracker.collect(store, &sidebar, me.as_deref(), &self.late, conversation_id) {
            incoming.chat.desktop = engine.chat_desktop_notifications(conversation_id);
            if !rules::decide(&settings, &incoming, environment).toast { continue; }
            let body = if !rules::shows_preview(&settings) {
                String::new()
            } else {
                match &incoming.preview {
                    Preview::Text(text) => text.replace('\n', " "),
                    Preview::Image => "Sent a picture".to_owned(),
                }
            };
            let title = format!("{} · {}", incoming.sender_name, incoming.chat_title);
            let notification = rune_notify::Notification::new(&incoming.message_id, title)
                .body(body)
                .info("conversation_id", conversation_id);
            rune_notify::post(&notification);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Actions, SendTarget};
    use crate::ui::UiState;
    use rune::{Env, Runtime, TextEdit};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake(RefCell<Vec<String>>);
    impl Actions for Fake {
        fn open(&self, id: &str) { self.0.borrow_mut().push(id.into()); }
        fn send(&self, _: SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
    }

    #[test]
    fn clicking_a_notification_opens_its_chat_from_another_screen_and_keeps_the_draft() {
        let mut state = UiState::new(Fake::default());
        state.selected = Some("old".into());
        state.composer = TextEdit::new("unsent");
        state.screens.screen = crate::screens::Screen::Calendar;
        let mut rt = Runtime::new(state, crate::ui::view::<Fake>, (1000.0, 700.0), Env { scale: 1.0, system_dark: false });
        rt.frame(0.0);
        let click = rune_notify::Click { id: "message".into(), info: vec![("conversation_id".into(), "target".into())] };
        rt.update(move |s, tx| { s.apply(clicked(&click).unwrap()); assert!(open_pending(s, tx)); });
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.selected.as_deref(), Some("target"));
        assert_eq!(rt.state.screens.screen, crate::screens::Screen::Chat);
        assert_eq!(*rt.state.actions.0.borrow(), ["target"]);
        assert!(rt.find("composer").is_some());
        rt.update(|s, tx| crate::ui::open_chat(s, "old", tx));
        assert_eq!(rt.state.composer.text, "unsent");
        assert!(clicked(&rune_notify::Click::default()).is_none());
        assert!(clicked(&rune_notify::Click { id: String::new(), info: vec![("conversation_id".into(), " ".into())] }).is_none());
    }
}
