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
    let screen = if s.channel(&id).is_some() { crate::screens::Screen::Teams } else { crate::screens::Screen::Chat };
    crate::screens::navigate(s, screen);
    crate::ui::open_chat(s, &id, tx);
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
}

impl Notifications {
    pub fn new() -> Self {
        Self { tracker: IncomingTracker::new(Utc::now()), late: LateArrivals::default(), throttle: rules::SoundThrottle::default() }
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
            let decision = rules::decide(&settings, &incoming, environment);
            if !decision.toast { continue; }
            if decision.sound && self.throttle.allow(conversation_id, std::time::Instant::now()) {
                let sounds = &settings.sounds;
                crate::sound::play(if decision.urgent_sound { sounds.urgent_sound_type } else { sounds.notification_sound_type }.file_name());
            }
            if rules::should_flash(&settings, true, rune_notify::app_focused()) { crate::taskbar::flash(); }
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
        mentioned
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
