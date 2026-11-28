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
    pub repeat_menu: bool,
}
impl Default for State {
    fn default() -> Self {
        let (start, end) = state::call::default_window(Utc::now(), &Local);
        Self { open: false, request: 0, sending: false, error: None, title: TextEdit::default(), date: TextEdit::new(start.date().to_string()), start: TextEdit::new(start.time().format("%H:%M").to_string()), end: TextEdit::new(end.time().format("%H:%M").to_string()), details: TextEdit::default(), people: Default::default(), repeat: chatsvc::Repeat::Never, repeat_menu: false }
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
        // ponytail: typed as YYYY-MM-DD; a picker would need a popup inside this dialog.
        .child(field("Date", "meeting-date", &s.meeting.date, |s| &mut s.meeting.date))
        .child(row().w_full().gap(8.0)
            .child(field("Start", "meeting-start", &s.meeting.start, |s| &mut s.meeting.start))
            .child(field("End", "meeting-end", &s.meeting.end, |s| &mut s.meeting.end)))
        .child(col().w_full().gap(4.0).child(text("Repeat").font(ts(12.0, 600.0)))
            .child(crate::controls::button::<A>("meeting-repeat", &format!("{} ▾", s.meeting.repeat.label()), crate::controls::Variant::Secondary, crate::controls::Size::Md,
                |s: &mut UiState<A>, _| s.meeting.repeat_menu = !s.meeting.repeat_menu))
            .children(s.meeting.repeat_menu.then(|| col().key("meeting-repeat-menu").w_full().gap(2.0).children(chatsvc::Repeat::ALL.into_iter().map(|repeat| {
                let selected = s.meeting.repeat == repeat;
                crate::controls::menu_item::<A>(&format!("meeting-repeat-{repeat:?}"), repeat.label(), selected.then_some("check"), selected, move |s| {
                    s.meeting.repeat = repeat;
                    s.meeting.repeat_menu = false;
                })
            })))))
        .child(field("Details", "meeting-details", &s.meeting.details, |s| &mut s.meeting.details))
        .child(row().key("meeting-teams-link").w_full().gap(6.0).center_y()
            .child(crate::icons::sym("videocam", 14.0).color(crate::avatar::rgb(app_backend::schemes::active_palette().text_muted)))
            .child(text("Teams meeting link is added").font(ts(12.0, 400.0)).color(crate::avatar::rgb(app_backend::schemes::active_palette().text_muted))));
    body = body
        .children(s.meeting.error.as_ref().map(|error| text(error.clone()).key("meeting-error").wrap()))
        .child(row().w_full().gap(8.0).justify_end()
            .child(crate::controls::button::<A>("meeting-cancel", "Cancel", crate::controls::Variant::Secondary, crate::controls::Size::Md, |s: &mut UiState<A>, _| s.meeting.open = false))
            .child(if s.meeting.sending {
                // Keep the press target so a click cannot fall through to the dialog's scrim.
                crate::controls::button::<A>("meeting-send", "Sending…", crate::controls::Variant::Quiet, crate::controls::Size::Md, |_, _| {})
                    .on_press(|_: &mut UiState<A>| {})
            } else {
                crate::controls::button::<A>("meeting-send", "Send", crate::controls::Variant::Primary, crate::controls::Size::Md, |s: &mut UiState<A>, _| submit(s))
            }));
    Some(crate::shortcuts::card("meeting-form", 540.0, body, |s: &mut UiState<A>| s.meeting.open = false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SendTarget;
    use std::cell::Cell;

    #[derive(Default)]
    struct Fake {
        created: Cell<u32>,
    }
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn create_meeting(&self, _: u64, _: app_backend::schedule::MeetingDraft) {
            self.created.set(self.created.get() + 1);
        }
    }

    fn runtime() -> Runtime<UiState<Fake>> {
        let mut rt = Runtime::new(UiState::new(Fake::default()), crate::ui::view::<Fake>, (1000.0, 800.0), Env { system_dark: false, scale: 1.0 });
        rt.update(|s, _| open(s));
        rt.frame(0.0);
        rt
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn label(rt: &Runtime<UiState<Fake>>, key: &str) -> Option<String> {
        rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no element {key}")).label
    }

    #[test]
    fn the_form_uses_gpui_labels_and_a_repeat_menu() {
        let mut rt = runtime();
        for (key, text) in [("meeting-date", "Date"), ("meeting-start", "Start"), ("meeting-end", "End"), ("meeting-send", "Send"), ("meeting-cancel", "Cancel")] {
            assert_eq!(label(&rt, key).as_deref(), Some(text), "{key}");
        }
        assert!(rt.find("meeting-teams-link").is_some());
        assert!(rt.find("meeting-repeat-Daily").is_none(), "the options stay hidden until the menu opens");
        click(&mut rt, "meeting-repeat");
        click(&mut rt, "meeting-repeat-Daily");
        assert_eq!(rt.state.meeting.repeat, chatsvc::Repeat::Daily);
        assert!(rt.find("meeting-repeat-Daily").is_none(), "choosing closes the menu");
    }

    #[test]
    fn cancel_closes_without_sending() {
        let mut rt = runtime();
        rt.update(|s, _| s.meeting.title = TextEdit::new("Planning"));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "meeting-cancel");
        assert!(!rt.state.meeting.open);
        assert_eq!(rt.state.actions.created.get(), 0);
    }

    #[test]
    fn send_creates_the_meeting_once_the_form_is_valid() {
        let mut rt = runtime();
        rt.update(|s, _| { s.meeting.title = TextEdit::new("Planning"); s.meeting.date = TextEdit::new("2099-01-05"); s.meeting.start = TextEdit::new("09:00"); s.meeting.end = TextEdit::new("10:00"); });
        rt.frame(1.0 / 60.0);
        click(&mut rt, "meeting-send");
        assert_eq!(rt.state.actions.created.get(), 1);
    }

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
