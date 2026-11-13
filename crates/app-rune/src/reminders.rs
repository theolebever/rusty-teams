//! Meeting reminders: from five minutes before an online meeting until five minutes after its
//! start, a card offers Join, Snooze and Dismiss, as the GPUI meeting toast does
//! (`app_state::call::reminder`). Snooze and Dismiss are written to the Outlook reminder.
//! ponytail: no `reminder_view` poll, so a reminder snoozed in Outlook still shows here.

use std::collections::HashMap;

use chatsvc::{CalendarEvent, ResponseType};
use chrono::{DateTime, Duration, Local, Utc};
use rune::*;

use crate::screens::{button, color, style};
use crate::types::Actions;
use crate::ui::{UiState, text};

const LEAD_MINUTES: i64 = 5;
const GRACE_MINUTES: i64 = 5;
const SNOOZE_MINUTES: i64 = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct Reminder {
    key: String,
    event_id: String,
    subject: String,
    start: DateTime<Utc>,
    join_url: String,
}

#[derive(Default)]
pub struct State {
    pub shown: Option<Reminder>,
    /// Event keys answered here, with when they may show again (`None`: never).
    quiet: HashMap<String, Option<DateTime<Utc>>>,
}

fn key(event: &CalendarEvent) -> String {
    format!("{}@{}", event.object_id, event.start.timestamp())
}

fn due(event: &CalendarEvent, now: DateTime<Utc>) -> bool {
    let eligible = event.online && !event.cancelled && !event.all_day && event.join_url.is_some() && event.response != ResponseType::Declined;
    eligible && now >= event.start - Duration::minutes(LEAD_MINUTES) && now < event.end.min(event.start + Duration::minutes(GRACE_MINUTES))
}

/// Shows the next due reminder and hides one that is over. True when the card changed.
pub fn poll<A: Actions>(s: &mut UiState<A>, now: DateTime<Utc>) -> bool {
    let quiet = &s.reminders.quiet;
    let next = s.screens.agenda.iter()
        .filter(|event| due(event, now))
        .find(|event| quiet.get(&key(event)).is_none_or(|until| until.is_some_and(|until| now >= until)))
        .map(|event| Reminder {
            key: key(event),
            event_id: event.object_id.clone(),
            subject: event.subject.clone(),
            start: event.start,
            join_url: event.join_url.clone().unwrap_or_default(),
        });
    if next.as_ref().map(|r| &r.key) == s.reminders.shown.as_ref().map(|r| &r.key) { return false; }
    if let Some(reminder) = &next { announce(s, reminder); }
    s.reminders.shown = next;
    true
}

/// The meeting chime, taskbar flash and system toast of a newly shown reminder.
fn announce<A: Actions>(s: &UiState<A>, reminder: &Reminder) {
    let settings = &s.settings.prefs.notify;
    let busy = matches!(s.own.status.availability.as_str(), "Busy" | "DoNotDisturb");
    let paused = s.own.paused_until.is_some_and(|until| Utc::now() < until);
    let silent = settings.do_not_disturb || paused || crate::notifications::system_quiet();
    if app_backend::notify::rules::meeting_start_sound(settings, busy) && !silent {
        crate::sound::play(app_backend::notify::sounds::CallTone::MeetingReminder.file_name());
    }
    if !rune_notify::app_focused() { crate::taskbar::flash(); }
    if !silent {
        rune_notify::post(&rune_notify::Notification::new(&reminder.key, reminder.subject.clone()).body(subtitle(reminder, Utc::now())));
    }
}

fn subtitle(reminder: &Reminder, now: DateTime<Utc>) -> String {
    let clock = reminder.start.with_timezone(&Local).format("%H:%M");
    if now >= reminder.start { format!("{clock} - started") } else { format!("{clock} - starting soon") }
}

fn answer<A: Actions>(s: &mut UiState<A>, until: Option<DateTime<Utc>>) {
    let Some(reminder) = s.reminders.shown.take() else { return };
    s.reminders.quiet.insert(reminder.key, until);
    s.actions.write_reminder(&reminder.event_id, until);
}

/// Opens the prejoin of the shown reminder's meeting; false when none shows.
pub fn join<A: Actions>(s: &mut UiState<A>) -> bool {
    if s.read_only { return false; }
    // Joining hides the card here only; the Outlook reminder is left as it is.
    let Some(reminder) = s.reminders.shown.take() else { return false };
    s.calls.link = TextEdit::new(&reminder.join_url);
    s.screens.screen = crate::screens::Screen::Calls;
    s.reminders.quiet.insert(reminder.key, None);
    true
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let reminder = s.reminders.shown.as_ref()?;
    let mut actions = row().gap(8.0);
    if !s.read_only {
        actions = actions.child(button::<A>("Join", "reminder-join", |s, _| { join(s); }));
    }
    let card = col().key("meeting-reminder").w(340.0).padding(16.0).gap(10.0).radius(10.0).fill(color("#FFFFFF")).border(1.0, color("#E0E0E0"))
        .role("alert").label(reminder.subject.clone())
        .child(text(reminder.subject.clone()).font(style(15.0, 600.0)).color(color("#242424")).truncate())
        .child(text(subtitle(reminder, Utc::now())).font(style(13.0, 400.0)).color(color("#616161")))
        .child(actions
            .child(button::<A>("Snooze", "reminder-snooze", |s, _| answer(s, Some(Utc::now() + Duration::minutes(SNOOZE_MINUTES)))))
            .child(button::<A>("Dismiss", "reminder-dismiss", |s, _| answer(s, None))));
    Some(el().pin(None, None, Some(16.0), Some(16.0)).child(card))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake(RefCell<Vec<(String, bool)>>);
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn write_reminder(&self, event_id: &str, snooze_until: Option<DateTime<Utc>>) { self.0.borrow_mut().push((event_id.into(), snooze_until.is_some())); }
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
        assert_eq!(rt.state.reminders.shown.as_ref().map(|r| r.event_id.as_str()), Some("soon"));
        let click = |rt: &mut Runtime<UiState<Fake>>, key: &str| {
            let (x, y) = rt.find(key).expect("reminder button");
            rt.pointer_down(x, y); rt.pointer_up(x, y); rt.frame(1.0 / 60.0);
        };
        click(&mut rt, "reminder-snooze");
        rt.update(move |s, _| { poll(s, now); });
        rt.frame(1.0);
        assert_eq!(rt.state.reminders.shown.as_ref().map(|r| r.event_id.as_str()), Some("next"));
        click(&mut rt, "reminder-join");
        assert_eq!(rt.state.screens.screen, crate::screens::Screen::Calls);
        assert!(rt.state.calls.link.text.ends_with("/next"));
        rt.update(move |s, _| { poll(s, now + Duration::minutes(6)); });
        assert_eq!(rt.state.reminders.shown.as_ref().map(|r| r.event_id.as_str()), Some("soon"), "snooze ran out");
        assert_eq!(*rt.state.actions.0.borrow(), [("soon".to_owned(), true)]);
    }
}
