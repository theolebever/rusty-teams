//! Own presence: the title-bar account button, its menu (availability, duration, reset, status
//! message) and the status message dialog, as `crates/app/src/views/status_menu.rs` and
//! `status_message_dialog.rs` do. State changes are optimistic; the bridge confirms through
//! `Update::OwnStatus`.

use app_backend::schemes::active_palette;
use chrono::{DateTime, Datelike, Duration, Local, NaiveTime, TimeZone, Utc};
use rune::*;
use teams_core::{ForcedAvailability, ForcedKind, PresenceStatus, StatusNote};

use crate::avatar::{Presence, person, rgb};
use crate::icons::sym;
use crate::types::Actions;
use crate::ui::{UiState, text, ts};

pub const NOTE_LIMIT: usize = 280;
const MENU_WIDTH: f32 = 260.0;
const CHOICES: [ForcedKind; 6] = [
    ForcedKind::Available,
    ForcedKind::Busy,
    ForcedKind::DoNotDisturb,
    ForcedKind::BeRightBack,
    ForcedKind::Away,
    ForcedKind::Offline,
];
const STATUS_SPANS: [Span; 5] = [Span::HalfHour, Span::Hour, Span::TwoHours, Span::Today, Span::Week];
const NOTE_SPANS: [Span; 5] = [Span::Never, Span::Hour, Span::FourHours, Span::Today, Span::Week];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    Never,
    HalfHour,
    Hour,
    TwoHours,
    FourHours,
    Today,
    Week,
}

impl Span {
    fn label(self) -> &'static str {
        match self {
            Span::Never => "Never",
            Span::HalfHour => "30 minutes",
            Span::Hour => "1 hour",
            Span::TwoHours => "2 hours",
            Span::FourHours => "4 hours",
            Span::Today => "Today",
            Span::Week => "This week",
        }
    }

    pub fn expires_at<Z: TimeZone>(self, now: DateTime<Z>) -> Option<DateTime<Utc>> {
        let after = |span: Duration| Some((now.clone() + span).with_timezone(&Utc));
        match self {
            Span::Never => None,
            Span::HalfHour => after(Duration::minutes(30)),
            Span::Hour => after(Duration::hours(1)),
            Span::TwoHours => after(Duration::hours(2)),
            Span::FourHours => after(Duration::hours(4)),
            Span::Today => end_of_day(&now, 0),
            // Saturday night, as the GPUI app counts the week from Sunday.
            Span::Week => end_of_day(&now, 6 - i64::from(now.weekday().num_days_from_sunday())),
        }
    }
}

fn end_of_day<Z: TimeZone>(now: &DateTime<Z>, days_ahead: i64) -> Option<DateTime<Utc>> {
    let last = NaiveTime::from_hms_milli_opt(23, 59, 59, 999).unwrap_or(NaiveTime::MIN);
    let date = now.date_naive() + Duration::days(days_ahead);
    let local = now.timezone().from_local_datetime(&date.and_time(last)).latest();
    Some(local.map_or_else(|| (now.clone() + Duration::days(days_ahead + 1)).with_timezone(&Utc), |moment| moment.with_timezone(&Utc)))
}

pub struct Me {
    pub user_id: String,
    pub name: String,
    pub email: String,
}

pub struct State {
    pub paused_until: Option<DateTime<Utc>>,
    submenu: Option<&'static str>,
    pub me: Option<Me>,
    pub status: PresenceStatus,
    pub menu: bool,
    pub dialog: bool,
    pub note: TextEdit,
    pub note_span: Span,
    pub show_when_messaged: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            paused_until: None,
            submenu: None,
            me: None,
            status: PresenceStatus::default(),
            menu: false,
            dialog: false,
            note: TextEdit::default(),
            note_span: Span::Today,
            show_when_messaged: false,
        }
    }
}

pub fn label(kind: ForcedKind) -> &'static str {
    match kind {
        ForcedKind::Available => "Available",
        ForcedKind::Busy => "Busy",
        ForcedKind::DoNotDisturb => "Do not disturb",
        ForcedKind::BeRightBack => "Be right back",
        ForcedKind::Away => "Appear away",
        ForcedKind::Offline => "Appear offline",
    }
}

fn presence_of_kind(kind: ForcedKind) -> Presence {
    match kind {
        ForcedKind::Available => Presence::Available,
        ForcedKind::Busy => Presence::Busy,
        ForcedKind::DoNotDisturb => Presence::DoNotDisturb,
        ForcedKind::BeRightBack | ForcedKind::Away => Presence::Away,
        ForcedKind::Offline => Presence::Offline,
    }
}

/// The dot for a service availability, `None` while unknown.
pub fn presence_of(availability: teams_core::Availability) -> Option<Presence> {
    use teams_core::Availability as A;
    Some(match availability {
        A::Available => Presence::Available,
        A::Away => Presence::Away,
        A::Busy => Presence::Busy,
        A::DoNotDisturb => Presence::DoNotDisturb,
        A::Offline => Presence::Offline,
        A::Unknown => return None,
    })
}

fn own_presence(status: &PresenceStatus) -> Option<Presence> {
    match status.forced {
        Some(forced) => Some(presence_of_kind(forced.kind)),
        None => presence_of(teams_core::Availability::from_service(&status.availability)),
    }
}

/// The forced kind a duration applies to: any forced state but Available.
fn duration_target(status: &PresenceStatus) -> Option<ForcedKind> {
    status.forced.map(|forced| forced.kind).filter(|kind| *kind != ForcedKind::Available)
}

fn set_forced<A: Actions>(s: &mut UiState<A>, forced: Option<ForcedAvailability>) {
    let status = &mut s.own.status;
    status.forced = forced;
    if let Some(forced) = forced {
        status.availability = forced.kind.code().to_owned();
    }
    s.actions.set_availability(forced);
    s.own.menu = false;
}

fn save_note<A: Actions>(s: &mut UiState<A>) {
    let text: String = s.own.note.text.trim().chars().take(NOTE_LIMIT).collect();
    let note = (!text.is_empty()).then(|| StatusNote {
        text,
        show_when_messaged: s.own.show_when_messaged,
        expires_at: s.own.note_span.expires_at(Local::now()),
    });
    s.own.status.note = note.clone();
    s.actions.set_status_note(note);
    s.own.dialog = false;
}

/// The title-bar button: the user's photo with their presence dot.
pub fn account_button<A: Actions + 'static>(s: &UiState<A>) -> El {
    let (name, id) = s.own.me.as_ref().map_or(("You", None), |me| (me.name.as_str(), Some(me.user_id.as_str())));
    row()
        .key("title-account")
        .no_window_drag()
        .size(36.0, 36.0)
        .radius(18.0)
        .center()
        .hover(rgb(active_palette().row_hover))
        .on_press(|s: &mut UiState<A>| s.own.menu = !s.own.menu)
        .label("Account and status")
        .child(person(s, name, id, 28.0, own_presence(&s.own.status)))
}

/// The open menu and dialog, to be added at the root so they draw above everything.
pub fn layer<A: Actions + 'static>(s: &UiState<A>) -> Vec<El> {
    let mut layers = Vec::new();
    if s.own.menu {
        layers.push(menu(s));
    }
    if s.own.dialog {
        layers.push(dialog(s));
    }
    layers
}

fn item<A: Actions + 'static>(key: &str, glyph: Option<(&str, Color)>, label: &str, checked: bool, press: impl Fn(&mut UiState<A>) + 'static) -> El {
    let palette = active_palette();
    let mut row = row()
        .key(key.to_owned())
        .w_full()
        .h(32.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .radius(4.0)
        .hover(rgb(palette.popup_hover))
        .on_press(press);
    row = row.child(match glyph {
        Some((name, color)) => sym(name, 14.0).color(color),
        None => el().size(14.0, 14.0),
    });
    row = row.child(text(label.to_owned()).font(ts(13.0, 400.0)).color(rgb(palette.foreground)).grow(1.0).truncate());
    if checked {
        row = row.child(sym("check", 14.0).color(rgb(palette.foreground)));
    }
    row
}

fn status_glyph(presence: Presence) -> (&'static str, Color) {
    let palette = active_palette();
    match presence {
        Presence::Available => ("status_available", rgb(palette.green)),
        Presence::Away => ("status_away", rgb(palette.amber)),
        Presence::Busy => ("status_dot", rgb(palette.red)),
        Presence::DoNotDisturb => ("status_dnd", rgb(palette.red)),
        Presence::Offline => ("status_offline", rgb(palette.text_muted)),
    }
}

fn menu<A: Actions + 'static>(s: &UiState<A>) -> El {
    let palette = active_palette();
    let muted = rgb(palette.text_muted);
    if let Some(page) = s.own.submenu {
        let mut menu = col().key("status-menu").w(MENU_WIDTH).padding(6.0).gap(2.0).radius(8.0)
            .fill(rgb(palette.popup)).border(1.0, rgb(palette.border)).anchored("title-account", Side::Bottom, Align::End, 4.0)
            .modal(true, |s: &mut UiState<A>| { s.own.menu = false; s.own.submenu = None; })
            .child(item("status-back", Some(("keyboard_arrow_left", muted)), "Back", false, |s: &mut UiState<A>| s.own.submenu = None));
        if page == "pause" {
            for (index, span) in STATUS_SPANS.into_iter().enumerate() {
                menu = menu.child(item(&format!("pause-for-{index}"), None, span.label(), false, move |s: &mut UiState<A>| {
                    s.actions.set_notification_pause(span.expires_at(Local::now()));
                    s.own.menu = false; s.own.submenu = None;
                }));
            }
            if s.own.paused_until.is_some_and(|until| Utc::now() < until) {
                menu = menu.child(item("pause-resume", None, "Resume notifications", false, |s: &mut UiState<A>| {
                    s.actions.set_notification_pause(None); s.own.menu = false; s.own.submenu = None;
                }));
            }
        } else {
            for (key, label, kind) in [("office", "Office", Some(teams_core::WorkLocationKind::Office)), ("remote", "Remote", Some(teams_core::WorkLocationKind::Remote)), ("clear", "Clear work location", None)] {
                menu = menu.child(item(&format!("work-location-{key}"), None, label, s.own.status.work_location.map(|location| location.kind) == kind, move |s: &mut UiState<A>| {
                    let location = kind.and_then(|kind| Span::Today.expires_at(Local::now()).map(|until| (kind, until)));
                    s.actions.set_work_location(location); s.own.menu = false; s.own.submenu = None;
                }));
            }
        }
        return menu;
    }
    let mut header = col().grow(1.0).min_w(0.0);
    let (name, id, email) = s.own.me.as_ref().map_or(("", None, ""), |me| (me.name.as_str(), Some(me.user_id.as_str()), me.email.as_str()));
    header = header
        .child(text(name.to_owned()).font(ts(14.0, 600.0)).color(rgb(palette.foreground)).truncate())
        .child(text(email.to_owned()).font(ts(12.0, 400.0)).color(muted).truncate());
    if let Some(note) = &s.own.status.note {
        header = header.child(text(note.text.clone()).key("status-current-note").font(ts(12.0, 400.0)).color(rgb(palette.text_soft)).truncate());
    }
    let mut menu = col()
        .key("status-menu")
        .w(MENU_WIDTH)
        .padding(6.0)
        .gap(2.0)
        .radius(8.0)
        .fill(rgb(palette.popup))
        .border(1.0, rgb(palette.border))
        .anchored("title-account", Side::Bottom, Align::End, 4.0)
        .modal(true, |s: &mut UiState<A>| s.own.menu = false)
        .child(row().w_full().padding(6.0).gap(10.0).center_y().child(person(s, name, id, 40.0, own_presence(&s.own.status))).child(header))
        .child(el().w_full().h(1.0).fill(rgb(palette.border)));
    let current = s.own.status.forced.map(|forced| forced.kind);
    for kind in CHOICES {
        let (glyph, color) = status_glyph(presence_of_kind(kind));
        menu = menu.child(item(&format!("status-{}", kind.code()), Some((glyph, color)), label(kind), current == Some(kind), move |s: &mut UiState<A>| {
            set_forced(s, Some(ForcedAvailability { kind, expires_at: None }));
        }));
    }
    if let Some(kind) = duration_target(&s.own.status) {
        menu = menu.child(text("Duration").font(ts(11.0, 600.0)).color(muted).px(8.0).pt(6.0));
        for (index, span) in STATUS_SPANS.into_iter().enumerate() {
            menu = menu.child(item(&format!("status-for-{index}"), None, span.label(), false, move |s: &mut UiState<A>| {
                set_forced(s, Some(ForcedAvailability { kind, expires_at: span.expires_at(Local::now()) }));
            }));
        }
    }
    let paused = s.own.paused_until.filter(|until| Utc::now() < *until);
    let pause_label = paused.map(|until| format!("Paused until {}", until.with_timezone(&Local).format("%H:%M"))).unwrap_or_else(|| "Pause notifications".into());
    menu = menu.child(item("status-pause", Some(("notifications_off", muted)), &pause_label, false, |s: &mut UiState<A>| s.own.submenu = Some("pause")));
    if !s.read_only {
        menu = menu.child(item("status-work-location", Some(("location_on", muted)), "Set work location for today", false, |s: &mut UiState<A>| s.own.submenu = Some("location")));
    }
    menu.child(el().w_full().h(1.0).fill(rgb(palette.border)))
        .child(item("status-reset", Some(("close", muted)), "Reset status", false, |s: &mut UiState<A>| set_forced(s, None)))
        .child(item("status-message", Some(("edit", muted)), "Set status message", false, |s: &mut UiState<A>| {
            s.own.note = TextEdit::new(s.own.status.note.as_ref().map(|note| note.text.clone()).unwrap_or_default());
            s.own.show_when_messaged = s.own.status.note.as_ref().is_some_and(|note| note.show_when_messaged);
            s.own.menu = false;
            s.own.dialog = true;
        }))
}

fn chip<A: Actions + 'static>(key: String, label: &str, on: bool, press: impl Fn(&mut UiState<A>) + 'static) -> El {
    let palette = active_palette();
    let (fill, ink) = if on { (palette.accent, palette.on_accent) } else { (palette.popup_hover, palette.foreground) };
    row().key(key).h(28.0).px(10.0).center().radius(14.0).fill(rgb(fill)).on_press(press)
        .child(text(label.to_owned()).font(ts(12.0, 600.0)).color(rgb(ink)))
}

fn dialog<A: Actions + 'static>(s: &UiState<A>) -> El {
    let palette = active_palette();
    let mut spans = row().gap(6.0);
    for (index, span) in NOTE_SPANS.into_iter().enumerate() {
        spans = spans.child(chip(format!("note-span-{index}"), span.label(), s.own.note_span == span, move |s: &mut UiState<A>| s.own.note_span = span));
    }
    let when = s.own.show_when_messaged;
    let sheet = col()
        .key("status-dialog")
        .w(420.0)
        .padding(20.0)
        .gap(12.0)
        .radius(10.0)
        .fill(rgb(palette.popup))
        .role("dialog")
        .label("Status message")
        .modal(true, |s: &mut UiState<A>| s.own.dialog = false)
        .child(text("Status message").font(ts(16.0, 600.0)).color(rgb(palette.foreground)))
        .child(text_input(&s.own.note, ts(14.0, 400.0))
            .key("status-note")
            .label("Status message")
            .placeholder("What's your status?", rgb(palette.text_muted))
            .color(rgb(palette.foreground))
            .w_full()
            .padding(8.0)
            .radius(6.0)
            .fill(rgb(palette.surface))
            .on_edit(|s: &mut UiState<A>| &mut s.own.note))
        .child(text("Clear status message after").font(ts(12.0, 400.0)).color(rgb(palette.text_muted)))
        .child(spans)
        .child(item("note-show-when", Some((if when { "check" } else { "add" }, rgb(palette.foreground))), "Show when people message me", false,
            |s: &mut UiState<A>| s.own.show_when_messaged = !s.own.show_when_messaged))
        .child(row().w_full().gap(8.0).justify_end()
            .child(crate::screens::button("Clear", "note-clear", |s: &mut UiState<A>, _| {
                s.own.note = TextEdit::new("");
                save_note(s);
            }))
            .child(crate::screens::button("Done", "note-save", |s: &mut UiState<A>, _| save_note(s))));
    el().key("status-scrim").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(Color::BLACK.alpha(0.35)).center().child(sheet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    #[test]
    fn spans_end_on_the_local_calendar() {
        let zone = FixedOffset::east_opt(2 * 3600).unwrap();
        // Wednesday 14 October 2026, 14:00 local.
        let now = zone.with_ymd_and_hms(2026, 10, 14, 14, 0, 0).unwrap();
        assert_eq!(Span::Hour.expires_at(now), Some(Utc.with_ymd_and_hms(2026, 10, 14, 13, 0, 0).unwrap()));
        assert_eq!(Span::Today.expires_at(now).unwrap().date_naive(), chrono::NaiveDate::from_ymd_opt(2026, 10, 14).unwrap());
        assert_eq!(Span::Week.expires_at(now).unwrap().date_naive(), chrono::NaiveDate::from_ymd_opt(2026, 10, 17).unwrap());
        assert_eq!(Span::Never.expires_at(now), None);
    }
}

/// Runtime tests for the presence, profile and toast views, which share one fake backend.
#[cfg(test)]
mod runtime_tests {
    use crate::types::{Actions, ChatRow, MessageRow, SendTarget, Update};
    use teams_core::{ForcedAvailability, StatusNote};
    use crate::ui::{UiState, view};
    use rune::{Env, Key, KeyEvent, Mods, Runtime};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<String>>,
    }

    impl Actions for Fake {
        fn open(&self, id: &str) { self.calls.borrow_mut().push(format!("open {id}")); }
        fn send(&self, _: SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn set_availability(&self, forced: Option<ForcedAvailability>) {
            self.calls.borrow_mut().push(format!("availability {:?}", forced.map(|forced| forced.kind)));
        }
        fn set_status_note(&self, note: Option<StatusNote>) {
            self.calls.borrow_mut().push(format!("note {:?}", note.map(|note| note.text)));
        }
        fn load_profile(&self, id: &str) { self.calls.borrow_mut().push(format!("profile {id}")); }
        fn open_direct(&self, id: &str) { self.calls.borrow_mut().push(format!("direct {id}")); }
        fn call_person(&self, id: &str, name: &str) { self.calls.borrow_mut().push(format!("call {id} {name}")); }
    }

    fn rt() -> Runtime<UiState<Fake>> {
        let mut rt = Runtime::new(UiState::new(Fake::default()), view::<Fake>, (1000.0, 700.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        rt
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn apply(rt: &mut Runtime<UiState<Fake>>, update: Update) {
        rt.update(|s, _| s.apply(update));
        rt.frame(1.0 / 60.0);
    }

    fn calls(rt: &Runtime<UiState<Fake>>) -> Vec<String> {
        rt.state.actions.calls.borrow().clone()
    }

    fn open_chat_with_message(rt: &mut Runtime<UiState<Fake>>) {
        apply(rt, Update::Chats(vec![ChatRow { id: "a".into(), title: "Ada".into(), preview: String::new(), unread: false, when: String::new() }]));
        click(rt, "chat-a");
        let row = MessageRow {
            sender_id: Some("u1".into()), system: false, id: "m".into(), reply_to_id: None, deleted: false, can_delete: false,
            reactions: vec![], sender: "Ada".into(), when: "14:06".into(), text: "hi".into(), mine: false, media: Default::default(),
        };
        apply(rt, Update::Messages { conversation_id: "a".into(), rows: vec![row] });
    }

    #[test]
    fn the_account_menu_sets_availability_and_resets() {
        let mut rt = rt();
        apply(&mut rt, Update::Me { user_id: "me".into(), name: "Me Myself".into(), email: "me@x.com".into() });
        assert!(rt.find("status-menu").is_none());
        click(&mut rt, "title-account");
        assert!(rt.find("status-menu").is_some());
        click(&mut rt, "status-Busy");
        assert_eq!(calls(&rt), ["availability Some(Busy)"]);
        assert!(rt.find("status-menu").is_none(), "picking closes the menu");
        assert_eq!(rt.state.own.status.availability, "Busy");
        click(&mut rt, "title-account");
        assert!(rt.find("status-for-0").is_some(), "a forced state offers durations");
        click(&mut rt, "status-reset");
        assert_eq!(calls(&rt)[1], "availability None");
    }

    #[test]
    fn the_status_message_dialog_saves_text() {
        let mut rt = rt();
        click(&mut rt, "title-account");
        click(&mut rt, "status-message");
        assert!(rt.find("status-dialog").is_some());
        click(&mut rt, "status-note");
        rt.key(KeyEvent { key: Key::Text("Heads down".into()), mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        click(&mut rt, "note-save");
        assert_eq!(calls(&rt), ["note Some(\"Heads down\")"]);
        assert!(rt.find("status-dialog").is_none());
        assert_eq!(rt.state.own.status.note.as_ref().map(|note| note.text.as_str()), Some("Heads down"));
    }

    #[test]
    fn clicking_a_sender_opens_the_profile_card_with_chat_and_call() {
        let mut rt = rt();
        open_chat_with_message(&mut rt);
        click(&mut rt, "avatar-m");
        assert!(rt.find("profile-card").is_some());
        assert_eq!(calls(&rt).last().map(String::as_str), Some("profile u1"));
        assert!(rt.find("profile-loading").is_some());
        let profile = teams_core::PersonProfile {
            user_id: "u1".into(), display_name: Some("Ada Lovelace".into()), job_title: Some("Engineer".into()), department: None,
            company: None, office_location: None, email: Some("ada@x.com".into()), work_phones: vec![], mobile_phone: None,
            manager: None, direct_reports: vec![], time_zone: None, availability: None, status_message: None,
            out_of_office: None, work_location: None,
        };
        apply(&mut rt, Update::Profile { user_id: "u1".into(), result: Ok(profile) });
        assert!(rt.find("profile-job").is_some() && rt.find("profile-email").is_some() && rt.find("profile-loading").is_none());
        click(&mut rt, "profile-call");
        assert_eq!(calls(&rt).last().map(String::as_str), Some("call u1 Ada Lovelace"));
        assert!(rt.find("profile-card").is_none());
        click(&mut rt, "name-m");
        click(&mut rt, "profile-chat");
        assert_eq!(calls(&rt).last().map(String::as_str), Some("direct u1"));
    }

    #[test]
    fn status_notices_show_a_toast_and_failure_shows_reconnection() {
        let mut rt = rt();
        apply(&mut rt, Update::Status("Online".into()));
        assert!(rt.find("toast").is_none());
        apply(&mut rt, Update::Status("Could not rename chat: offline".into()));
        assert!(rt.find("toast").is_some());
        click(&mut rt, "toast-close");
        assert!(rt.find("toast").is_none());
        apply(&mut rt, Update::Status("Failed: no network".into()));
        assert!(rt.find("connection-banner").is_none(), "not before it lasted");
        rt.update(|s, _| { s.toast.tick(std::time::Instant::now() + std::time::Duration::from_secs(5)); });
        rt.frame(1.0 / 60.0);
        assert!(rt.find("reconnecting").is_some());
        assert!(rt.find("messages").is_none());
    }
}
