//! Desktop notification filtering shared with the existing client.

use app_backend::backend::Engine;
use app_backend::notify::{incoming::{IncomingTracker, LateArrivals}, rules::{self, Environment, Preview}, settings};
use chrono::Utc;

pub const ACTIVATION_SCHEME: &str = "rusty-teams-rune";
pub const ACTIVATION_PREFIX: &str = "rusty-teams-rune://notification/";

pub fn clicked(click: &rune_notify::Click) -> Option<crate::types::Update> {
    if let Some(id) = click.get("conversation_id").filter(|id| !id.trim().is_empty()) {
        Some(match click.get("action") {
            Some("reply") => crate::types::Update::NotificationReply(id.into()),
            Some("read") => crate::types::Update::NotificationRead(id.into()),
            _ => crate::types::Update::OpenChat(id.into()),
        })
    } else {
        click.get("user_id").filter(|id| !id.trim().is_empty()).map(|id| crate::types::Update::OpenDirect(id.to_owned()))
    }
}

/// Drains the same pending open request used by a person's profile card.
pub fn open_pending<A: crate::types::Actions>(s: &mut crate::ui::UiState<A>, tx: &mut rune::Tx) -> bool {
    if !s.toast.reconnecting && !s.read_only {
        for id in s.profile.read_requests.drain(..) { s.actions.set_chat_read(&id, true); }
    }
    let Some(id) = s.profile.chat_request.take() else { return false };
    let screen = if s.channel(&id).is_some() { crate::screens::Screen::Teams } else { crate::screens::Screen::Chat };
    crate::screens::navigate(s, screen);
    crate::ui::open_chat(s, &id, tx);
    if std::mem::take(&mut s.profile.reply_focus) { tx.focus("composer", false); }
    true
}

pub fn environment(engine: &Engine, chat_in_foreground: bool, native_in_call: bool) -> Environment {
    let store = engine.store();
    let presence = store.meta("me_user_id").ok().flatten().and_then(|id| engine.presence(&id));
    let paused = app_backend::notify::pause::is_paused(app_backend::notify::pause::load(store), Utc::now());
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
pub(crate) fn system_quiet() -> bool {
    use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME, QUNS_RUNNING_D3D_FULL_SCREEN};
    // SAFETY: the API has no arguments and reports the current shell notification state.
    unsafe { SHQueryUserNotificationState() }.map_or(true, |state| matches!(state, QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE | QUNS_QUIET_TIME))
}

#[cfg(not(windows))]
pub(crate) fn system_quiet() -> bool { false }

/// What the taskbar button counts, as GPUI's `taskbar_badge_count`: unmuted unread chats, the
/// Activity bell and, with the adaptive rail on, each channel with an unread @mention.
pub fn badge_count<A: crate::types::Actions>(s: &crate::ui::UiState<A>) -> usize {
    let activity = &s.screens.activity;
    let muted = |id: &str| s.details.get(id).is_some_and(|details| details.muted);
    let chats = s.chats.iter().filter(|chat| chat.unread && !muted(&chat.id)).count();
    let bell = activity.items.iter().filter(|item| activity.counts_for_bell(item)).count();
    let channels = if s.section_settings.adaptive_rail() {
        let ids = s.teams.iter().flat_map(|team| &team.channels).map(|channel| channel.id.as_str()).collect();
        state::notify::taskbar::channels_with_mentions(&state::activity::server::feed_of(activity, 0), &ids)
    } else {
        0
    };
    chats + bell + channels
}

/// Rings while a call comes in (`rules::ringtone_for`) and flashes the taskbar when it starts;
/// loops the ringback or reconnecting tone of the current call (`CallView::tone`) and plays its
/// joined, ended and lobby chimes.
pub fn ring<A: crate::types::Actions>(s: &mut crate::ui::UiState<A>) {
    for tone in s.calls.tones.drain(..) { crate::sound::play(tone.file_name()); }
    let tone = s.calls.tone();
    if s.call_tone.as_ref().map(|(playing, _)| *playing) != tone {
        s.call_tone = tone.map(|tone| (tone, crate::sound::start(tone.file_name(), true)));
    }
    let ringing = s.calls.ring.is_some();
    if ringing == std::mem::replace(&mut s.was_ringing, ringing) { return; }
    s.ringtone = None;
    if !ringing { return; }
    let settings = &s.settings.prefs.notify;
    let in_call = s.calls.state.as_ref().is_some_and(calling::CallState::is_active);
    // GPUI counts being in a call as busy, so "sound when busy" off silences the in-call beep.
    if let Some(tone) = rules::ringtone_for(settings, in_call, in_call || own_busy(s)).filter(|_| !settings.do_not_disturb) {
        s.ringtone = Some(crate::sound::start(tone.file_name(), true));
    }
    if rules::should_flash(settings, true, rune_notify::app_focused()) { crate::taskbar::flash(); }
}

/// Whether the user's own presence is Busy or Do not disturb.
pub(crate) fn own_busy<A: crate::types::Actions>(s: &crate::ui::UiState<A>) -> bool {
    matches!(s.own.status.availability.as_str(), "Busy" | "DoNotDisturb")
}

pub struct Notifications {
    tracker: IncomingTracker,
    late: LateArrivals,
    throttle: rules::SoundThrottle,
    urgent: state::notify::urgent_repeat::UrgentRepeats,
    presence: std::collections::HashMap<String, state::data::PresenceKind>,
}

impl Notifications {
    pub fn new() -> Self {
        Self { tracker: IncomingTracker::new(Utc::now()), late: LateArrivals::default(), throttle: rules::SoundThrottle::default(), urgent: Default::default(), presence: Default::default() }
    }

    pub fn presence_changed(&mut self, engine: &Engine) {
        use state::data::{Presence, PresenceKind as P};
        use teams_core::Availability as A;
        let followed = state::presence_follow::load_followed(engine.store());
        self.presence.retain(|id, _| followed.iter().any(|person| person.user_id == *id));
        let env = environment(engine, false, false);
        let settings = settings::load(engine.store());
        for person in followed {
            let Some(status) = engine.presence(&person.user_id) else { continue };
            let next = match status.availability { A::Available => P::Available, A::Busy => P::Busy, A::DoNotDisturb => P::DoNotDisturb, A::Away => P::Away, A::Offline => P::Offline, A::Unknown => P::Unknown };
            let previous = self.presence.insert(person.user_id.clone(), next).map(Presence::Live).unwrap_or(Presence::Loading);
            let Some(body) = state::presence_follow::status_change_text(previous, next) else { continue };
            if settings.do_not_disturb || env.paused || env.system_quiet || env.own_do_not_disturb { continue; }
            rune_notify::post(&rune_notify::Notification::new(format!("presence-{}", person.user_id), &person.name).body(body).info("user_id", &person.user_id));
        }
    }

    pub fn mark_late(&mut self, conversation_id: &str, message_ids: Vec<String>) {
        self.late.insert(conversation_id, message_ids);
    }

    /// Posts the toasts for new messages of a chat; true when one of them @mentions the user.
    pub fn messages(&mut self, engine: &Engine, conversation_id: &str, environment: Environment) -> bool {
        let store = engine.store();
        let Ok(sidebar) = engine.sidebar() else { return false };
        let mut mentioned = false;
        let me = store.meta("me_user_id").ok().flatten();
        let mut settings = settings::load(store);
        settings.account = teams_core::stored_chat_settings(store).settings().notifications;
        for mut incoming in self.tracker.collect(store, &sidebar, me.as_deref(), &self.late, conversation_id) {
            incoming.chat.desktop = engine.chat_desktop_notifications(conversation_id);
            mentioned |= incoming.mentions_me && !environment.chat_in_foreground;
            if !environment.chat_in_foreground { self.urgent.track(&incoming, Utc::now()); }
            self.present(&incoming, &settings, environment);
        }
        mentioned
    }

    pub fn repeat_urgent(&mut self, engine: &Engine, foreground: Option<&str>, in_call: bool) {
        if self.urgent.is_empty() { return; }
        if let Some(id) = foreground { self.urgent.stop_conversation(id); }
        // Keep pending repeats when the sidebar cannot be read; a transient failure is not a read receipt.
        if let Ok(sidebar) = engine.sidebar() {
            self.urgent.stop_read(|id| sidebar.chats.iter().find(|chat| chat.id == id)
                .and_then(|chat| chat.last_read_at.filter(|_| !chat.unread)));
        }
        let mut settings = settings::load(engine.store());
        settings.account = teams_core::stored_chat_settings(engine.store()).settings().notifications;
        for mut incoming in self.urgent.due(Utc::now()) {
            if let Ok(sidebar) = engine.sidebar()
                && let Some(chat) = sidebar.chats.iter().find(|chat| chat.id == incoming.conversation_id) {
                    incoming.muted = chat.muted;
                }
            incoming.chat.desktop = engine.chat_desktop_notifications(&incoming.conversation_id);
            self.present(&incoming, &settings, environment(engine, false, in_call));
        }
    }

    fn present(&mut self, incoming: &rules::Incoming, settings: &rules::Settings, environment: Environment) {
            let conversation_id = &incoming.conversation_id;
            let decision = rules::decide(settings, incoming, environment);
            if !decision.toast { return; }
            if decision.sound && self.throttle.allow(conversation_id, std::time::Instant::now()) {
                let sounds = &settings.sounds;
                crate::sound::play(if decision.urgent_sound { sounds.urgent_sound_type } else { sounds.notification_sound_type }.file_name());
            }
            if rules::should_flash(settings, true, rune_notify::app_focused()) { crate::taskbar::flash(); }
            let body = if !rules::shows_preview(settings) {
                String::new()
            } else {
                match &incoming.preview {
                    Preview::Text(text) => text.replace('\n', " "),
                    Preview::Image => "Sent a picture".to_owned(),
                }
            };
            let title = format!("{} · {}", incoming.sender_name, incoming.chat_title);
            if let Err(error) = crate::desktop_toast::post(&incoming.message_id, conversation_id, &title, &body) {
                app_backend::diagnostics::record(format!("Actionable notification unavailable: {error}"));
                let notification = rune_notify::Notification::new(&incoming.message_id, title).body(body).info("conversation_id", conversation_id);
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
        fn set_chat_read(&self, id: &str, read: bool) { self.0.borrow_mut().push(format!("read {id} {read}")); }
    }

    #[test]
    fn toast_read_waits_for_connection_and_reply_selects_the_conversation() {
        let mut state = UiState::new(Fake::default());
        state.toast.reconnecting = true;
        let read = rune_notify::Click { id: "message".into(), info: vec![("conversation_id".into(), "chat".into()), ("action".into(), "read".into())] };
        state.apply(clicked(&read).unwrap());
        let mut rt = Runtime::new(state, crate::ui::view::<Fake>, (1000.0, 700.0), Env { scale: 1.0, system_dark: false });
        rt.update(|s, tx| { open_pending(s, tx); });
        assert!(rt.state.actions.0.borrow().is_empty());
        rt.update(|s, tx| { s.toast.reconnecting = false; open_pending(s, tx); });
        assert_eq!(*rt.state.actions.0.borrow(), ["read chat true"]);
        assert!(rt.state.selected.is_none());
        let reply = rune_notify::Click { id: "message".into(), info: vec![("conversation_id".into(), "chat".into()), ("action".into(), "reply".into())] };
        rt.update(move |s, tx| { s.apply(clicked(&reply).unwrap()); assert!(open_pending(s, tx)); });
        assert_eq!(rt.state.selected.as_deref(), Some("chat"));
        assert!(!rt.state.profile.reply_focus);
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

    #[test]
    fn the_badge_skips_muted_chats_and_counts_channel_mentions_with_the_adaptive_rail() {
        use crate::types::{ChannelRow, ChatDetails, ChatRow, TeamRow, Update};
        let mut s = UiState::new(Fake::default());
        let chat = |id: &str| ChatRow { id: id.into(), title: id.into(), preview: String::new(), unread: true, when: String::new() };
        s.apply(Update::Chats(vec![chat("open"), chat("muted")]));
        s.details.insert("muted".into(), ChatDetails { muted: true, ..Default::default() });
        s.apply(Update::Teams(vec![TeamRow { id: "team".into(), name: "Team".into(), channels: vec![ChannelRow { hidden: false, id: "channel".into(), name: "General".into(), unread: true }] }]));
        let mention = teams_core::ActivityItem { message_id: "m".into(), source_thread_id: "channel".into(), activity_type: "mention".into(), activity_subtype: "person".into(), version_ms: 1, ..Default::default() };
        s.apply(Update::Activity(teams_core::ActivitySnapshot { items: vec![mention], ..Default::default() }));
        assert_eq!(badge_count(&s), 1);
        s.apply(Update::SectionSettings(teams_core::ChatSectionSettings { adaptive_rail: Some(true), ..Default::default() }));
        assert_eq!(badge_count(&s), 2);
    }
}
