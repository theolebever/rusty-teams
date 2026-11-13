//! Meeting reminders: the GPUI meeting toast model (`state::call::reminder::MeetingToasts`)
//! decides what shows; this module draws its oldest toast as one card with Join, Snooze and
//! Dismiss. Snooze and Dismiss are written to the Outlook reminder, and a reminder snoozed or
//! dismissed in Outlook is hidden here (`reminder_view` polled near a meeting, as GPUI).

use std::collections::HashSet;

use chrono::{DateTime, Local, Offset, Utc};
use rune::*;
use state::call::outlook_reminders::ReminderWrite;
use state::call::reminder::{MeetingToast, subtitle};

use crate::screens::{button, color, style};
use crate::types::Actions;
use crate::ui::{UiState, text};

fn shown<A: Actions>(s: &UiState<A>) -> Option<&MeetingToast> {
    s.reminders.entries().first()
}

/// Brings the toasts up to date with the agenda. True when the card changed.
pub fn poll<A: Actions>(s: &mut UiState<A>, now: DateTime<Utc>) -> bool {
    let before = shown(s).map(|toast| toast.id);
    if let Some((start, end)) = s.reminders.outlook_mut().poll_range(&s.screens.agenda, now) { s.actions.load_reminders(start, end); }
    // ponytail: no live-meeting lookup, so a meeting never reaches the Live phase here.
    let update = s.reminders.update(&s.screens.agenda, &HashSet::new(), now, true);
    let after = shown(s).cloned();
    if update.changed && after.as_ref().map(|toast| toast.id) != before {
        if let Some(toast) = &after { announce(s, toast, update.chime); }
    }
    update.changed
}

/// The taskbar flash and system toast of a newly shown reminder, and the meeting chime when
/// the model asks for one (from the meeting's start on).
fn announce<A: Actions>(s: &UiState<A>, toast: &MeetingToast, chime: bool) {
    let settings = &s.settings.prefs.notify;
    let paused = app_backend::notify::pause::is_paused(s.own.paused_until, Utc::now());
    let silent = settings.do_not_disturb || paused || crate::notifications::system_quiet();
    if chime && app_backend::notify::rules::meeting_start_sound(settings, crate::notifications::own_busy(s)) && !silent {
        crate::sound::play(app_backend::notify::sounds::CallTone::MeetingReminder.file_name());
    }
    if app_backend::notify::rules::should_flash(settings, !silent, rune_notify::app_focused()) { crate::taskbar::flash(); }
    if !silent {
        rune_notify::post(&rune_notify::Notification::new(&toast.event_key, toast.subject.clone()).body(card_subtitle(toast)));
    }
}

fn card_subtitle(toast: &MeetingToast) -> String {
    subtitle(toast, Utc::now(), Local::now().offset().fix())
}

fn snooze<A: Actions>(s: &mut UiState<A>) {
    let Some(id) = shown(s).map(|toast| toast.id) else { return };
    let Some(toast) = s.reminders.snooze(id) else { return };
    s.reminders.outlook_mut().note(&toast.event_id, ReminderWrite::Snooze(toast.start));
    s.actions.write_reminder(&toast.event_id, Some(toast.start));
}

fn dismiss<A: Actions>(s: &mut UiState<A>) {
    let Some(id) = shown(s).map(|toast| toast.id) else { return };
    let Some(toast) = s.reminders.dismiss(id) else { return };
    s.reminders.outlook_mut().note(&toast.event_id, ReminderWrite::Dismiss);
    s.actions.write_reminder(&toast.event_id, None);
}

/// Opens the prejoin of the shown reminder's meeting; false when none shows.
pub fn join<A: Actions>(s: &mut UiState<A>) -> bool {
    if s.read_only { return false; }
    // Joining hides the card here only; the Outlook reminder is left as it is.
    let Some(toast) = shown(s).map(|toast| toast.id).and_then(|id| s.reminders.dismiss_for_good(id)) else { return false };
    s.calls.link = TextEdit::new(&toast.join_url);
    s.screens.screen = crate::screens::Screen::Calls;
    true
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let toast = shown(s)?;
    let mut actions = row().gap(8.0);
    if !s.read_only {
        actions = actions.child(button::<A>("Join", "reminder-join", |s, _| { join(s); }));
    }
    let card = col().key("meeting-reminder").w(340.0).padding(16.0).gap(10.0).radius(10.0).fill(color("#FFFFFF")).border(1.0, color("#E0E0E0"))
        .role("alert").label(toast.subject.clone())
        .child(text(toast.subject.clone()).font(style(15.0, 600.0)).color(color("#242424")).truncate())
        .child(text(card_subtitle(toast)).font(style(13.0, 400.0)).color(color("#616161")))
        .child(actions
            .child(button::<A>("Snooze", "reminder-snooze", |s, _| snooze(s)))
            .child(button::<A>("Dismiss", "reminder-dismiss", |s, _| dismiss(s))));
    Some(el().pin(None, None, Some(16.0), Some(16.0)).child(card))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chatsvc::{CalendarEvent, ResponseType};
    use chrono::Duration;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake(RefCell<Vec<(String, bool)>>, RefCell<Vec<(DateTime<Utc>, DateTime<Utc>)>>);
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn write_reminder(&self, event_id: &str, snooze_until: Option<DateTime<Utc>>) { self.0.borrow_mut().push((event_id.into(), snooze_until.is_some())); }
        fn load_reminders(&self, start: DateTime<Utc>, end: DateTime<Utc>) { self.1.borrow_mut().push((start, end)); }
    }

    fn meeting(id: &str, start: DateTime<Utc>) -> CalendarEvent {
        CalendarEvent {
            object_id: id.into(), subject: format!("Meeting {id}"), start, end: start + Duration::minutes(30),
            all_day: false, cancelled: false, online: true, join_url: Some(format!("https://teams.microsoft.com/l/meetup-join/{id}")),
            thread_id: None, organizer_name: None, organizer_address: None, ical_uid: None, show_as: Default::default(),
            dial_in: None, response: ResponseType::Accepted,
        }
    }

    #[test]
    fn a_due_meeting_shows_a_card_that_snoozes_dismisses_and_joins() {
        let now = Utc::now();
        let mut state = UiState::new(Fake::default());
        state.settings.prefs.notify.do_not_disturb = true;
        state.screens.agenda = vec![meeting("later", now + Duration::minutes(30)), meeting("soon", now + Duration::minutes(3)), meeting("next", now + Duration::minutes(4))];
        let mut rt = Runtime::new(state, crate::ui::view::<Fake>, (1000.0, 700.0), Env { system_dark: false, scale: 1.0 });
        rt.update(move |s, _| assert!(poll(s, now)));
        rt.frame(1.0);
        assert_eq!(shown(&rt.state).map(|r| r.event_id.as_str()), Some("soon"));
        let click = |rt: &mut Runtime<UiState<Fake>>, key: &str| {
            let (x, y) = rt.find(key).expect("reminder button");
            rt.pointer_down(x, y); rt.pointer_up(x, y); rt.frame(1.0 / 60.0);
        };
        click(&mut rt, "reminder-snooze");
        rt.update(move |s, _| { poll(s, now); });
        rt.frame(1.0);
        assert_eq!(shown(&rt.state).map(|r| r.event_id.as_str()), Some("next"));
        click(&mut rt, "reminder-join");
        assert_eq!(rt.state.screens.screen, crate::screens::Screen::Calls);
        assert!(rt.state.calls.link.text.ends_with("/next"));
        rt.update(move |s, _| { poll(s, now + Duration::minutes(6)); });
        assert_eq!(shown(&rt.state).map(|r| r.event_id.as_str()), Some("soon"), "snooze ran out");
        assert_eq!(*rt.state.actions.0.borrow(), [("soon".to_owned(), true)]);
    }

    #[test]
    fn a_reminder_dismissed_in_outlook_leaves_and_one_snoozed_there_waits() {
        let now = Utc::now();
        let mut state = UiState::new(Fake::default());
        state.settings.prefs.notify.do_not_disturb = true;
        state.screens.agenda = vec![meeting("gone", now + Duration::minutes(3)), meeting("later", now + Duration::minutes(4))];
        let mut rt = Runtime::new(state, crate::ui::view::<Fake>, (1000.0, 700.0), Env { system_dark: false, scale: 1.0 });
        rt.update(move |s, _| assert!(poll(s, now)));
        let (start, end) = rt.state.actions.1.borrow()[0];
        assert_eq!(shown(&rt.state).map(|r| r.event_id.as_str()), Some("gone"));
        let reminders = vec![chatsvc::Reminder { event_id: "later".into(), fire_time: now + Duration::minutes(2) }];
        rt.update(move |s, _| s.apply(crate::types::Update::OutlookReminders { start, end, result: Ok(reminders) }));
        rt.update(move |s, _| { poll(s, now + Duration::seconds(1)); });
        rt.frame(1.0);
        assert!(rt.find("meeting-reminder").is_none(), "dismissed in Outlook, later snoozed there");
        assert_eq!(rt.state.actions.1.borrow().len(), 1, "polls at most once a minute");
        rt.update(move |s, _| { poll(s, now + Duration::minutes(2)); });
        assert_eq!(shown(&rt.state).map(|r| r.event_id.as_str()), Some("later"));
    }
}
