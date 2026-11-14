//! New meetings reuse GPUI's form validation and the shared calendar launcher.

use chrono::{Local, NaiveDate, NaiveTime, Utc};
use rune::*;
use crate::types::Actions;
use crate::ui::{UiState, text, ts};

pub struct State {
    pub open: bool,
    pub request: u64,
    pub sending: bool,
    pub error: Option<String>,
    pub title: TextEdit,
    pub date: TextEdit,
    pub start: TextEdit,
    pub end: TextEdit,
    pub details: TextEdit,
    pub people: crate::new_chat::Draft,
    pub repeat: chatsvc::Repeat,
}
impl Default for State {
    fn default() -> Self {
        let (start, end) = state::call::default_window(Utc::now(), &Local);
        Self { open: false, request: 0, sending: false, error: None, title: TextEdit::default(), date: TextEdit::new(start.date().to_string()), start: TextEdit::new(start.time().format("%H:%M").to_string()), end: TextEdit::new(end.time().format("%H:%M").to_string()), details: TextEdit::default(), people: Default::default(), repeat: chatsvc::Repeat::Never }
    }
}

pub fn open<A: Actions>(s: &mut UiState<A>) {
    if s.read_only { return; }
    s.meeting = State { open: true, request: s.meeting.request.wrapping_add(1), ..Default::default() };
}

fn draft(state: &State) -> Result<app_backend::schedule::MeetingDraft, String> {
    let time = |text: &str| NaiveTime::parse_from_str(text.trim(), "%H:%M").map_err(|_| "Enter a time like 14:30.".to_owned());
    state::call::draft_from_form(state::call::FormValues {
        subject: state.title.text.clone(),
        date: NaiveDate::parse_from_str(state.date.text.trim(), "%Y-%m-%d").ok(),
        start: time(&state.start.text)?, end: time(&state.end.text)?, repeat: state.repeat, details: state.details.text.clone(),
        people: state.people.chips.iter().map(|person| app_backend::schedule::Invitee { user_id: person.user_id.clone(), name: person.name.clone(), mail: person.mail.clone() }).collect(),
    }, &Local, None)
}

fn submit<A: Actions>(s: &mut UiState<A>) {
    if s.read_only || s.meeting.sending { return; }
    match draft(&s.meeting) {
        Ok(draft) => { s.meeting.sending = true; s.meeting.error = None; s.actions.create_meeting(s.meeting.request, draft); }
        Err(error) => s.meeting.error = Some(error),
    }
}

pub fn dialog<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    if !s.meeting.open { return None; }
    let field = |name: &str, key: &str, edit: &TextEdit, select: fn(&mut UiState<A>) -> &mut TextEdit| {
        col().w_full().gap(4.0).child(text(name.to_owned()).font(ts(12.0, 600.0)))
            .child(text_input(edit, ts(14.0, 400.0)).key(key.to_owned()).label(name.to_owned()).w_full().on_edit(select))
    };
    let mut body = col().w_full().gap(10.0).padding(16.0)
        .child(text("New meeting").font(ts(18.0, 600.0)))
        .child(field("Title", "meeting-title", &s.meeting.title, |s| &mut s.meeting.title))
        .child(crate::new_chat::picker(s, &s.meeting.people, "meeting-people", |s| &mut s.meeting.people, &[]))
        .child(field("Date (YYYY-MM-DD)", "meeting-date", &s.meeting.date, |s| &mut s.meeting.date))
        .child(row().w_full().gap(8.0)
            .child(field("Start (HH:MM)", "meeting-start", &s.meeting.start, |s| &mut s.meeting.start))
            .child(field("End (HH:MM)", "meeting-end", &s.meeting.end, |s| &mut s.meeting.end)))
        .child(field("Details", "meeting-details", &s.meeting.details, |s| &mut s.meeting.details));
    let mut repeats = row().w_full().wrap().gap(4.0);
    for repeat in chatsvc::Repeat::ALL {
        repeats = repeats.child(crate::screens::button(repeat.label(), &format!("meeting-repeat-{repeat:?}"), move |s: &mut UiState<A>, _| s.meeting.repeat = repeat)
            .opacity(if s.meeting.repeat == repeat { 1.0 } else { 0.5 }));
    }
    body = body.child(repeats)
        .children(s.meeting.error.as_ref().map(|error| text(error.clone()).key("meeting-error").wrap()))
        .child(crate::screens::button(if s.meeting.sending { "Sending…" } else { "Send invitation" }, "meeting-send", |s: &mut UiState<A>, _| submit(s)));
    Some(crate::shortcuts::card("meeting-form", 540.0, body, |s: &mut UiState<A>| s.meeting.open = false))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_form_checks_title_and_time_order_using_shared_validation() {
        let mut form = State::default();
        assert!(draft(&form).is_err());
        form.title = TextEdit::new("Planning");
        form.date = TextEdit::new("2026-11-18");
        form.start = TextEdit::new("14:00");
        form.end = TextEdit::new("13:00");
        assert!(draft(&form).is_err());
        form.end = TextEdit::new("15:00");
        let draft = draft(&form).expect("valid meeting");
        assert_eq!(draft.subject, "Planning");
        assert_eq!(draft.end - draft.start, chrono::Duration::hours(1));
    }
}
