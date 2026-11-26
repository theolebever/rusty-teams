//! Scheduled sending through Teams, with the same limits and time choices as GPUI.

use chrono::{DateTime, Local, NaiveDate, Utc};
use rune::*;
use crate::types::{Actions, SendTarget};
use crate::ui::{UiState, text, ts};

#[derive(Default)]
pub struct State {
    pub open: bool,
    pub day: Option<NaiveDate>,
    pub time: TextEdit,
    pub pending: Option<(SendTarget, String)>,
    pub error: Option<String>,
    pub loading: bool,
    pub rows: Vec<chatsvc::ScheduledDraft>,
    pub edit_id: Option<String>,
    pub edit: TextEdit,
    pub edit_draft: teams_core::Draft,
    pub changing: Option<String>,
    pub cancel_confirm: Option<String>,
}

pub fn open<A: Actions>(s: &mut UiState<A>) {
    s.schedule.open = !s.schedule.open;
    s.schedule.error = None;
    s.schedule.day = Some(Local::now().date_naive());
    s.schedule.time = TextEdit::new(app_base::schedule_time::format_time(app_base::schedule_time::default_custom_time(Local::now())));
}

fn blocker<A: Actions>(s: &UiState<A>) -> Option<&'static str> {
    let target = crate::ui::active_target(s)?;
    if s.read_only { Some("Read-only mode") }
    else if s.schedule.pending.is_some() { Some("Scheduling…") }
    else if target.conversation_id == teams_core::NOTES_CHAT_ID { Some("Not available in your notes") }
    else if s.composer.text.trim().is_empty() { Some("Nothing to schedule yet") }
    else if !crate::mentions::resolve(&s.composer.text, &s.compose.mentions.chosen).is_empty() { Some("Can't schedule messages with mentions yet") }
    else if !s.compose.gifs.is_empty() || s.compose.inline.iter().any(|(marker, _)| s.composer.text.contains(marker)) || !s.compose.inline_loading.is_empty() || !s.media.files(&target).is_empty() || s.media.is_uploading(&target) { Some("Can't schedule messages with images or files yet") }
    else if s.compose.reply.is_some() { Some("Can't schedule quoted replies yet") }
    else if crate::view_composer::sendable_importance(s, Some(&target)) != store::MessageImportance::Normal { Some("Scheduled messages use standard importance") }
    else if !s.compose.subject.text.trim().is_empty() || !s.compose.headline.text.trim().is_empty() { Some("Can't schedule posts with a subject or headline yet") }
    else { None }
}

pub fn submit<A: Actions>(s: &mut UiState<A>, at: DateTime<Local>) {
    if let Some(reason) = blocker(s) { s.schedule.error = Some(reason.into()); return; }
    if let Err(error) = app_base::schedule_time::validate(at, Local::now()) { s.schedule.error = Some(error.message().into()); return; }
    let Some(target) = crate::ui::active_target(s) else { return };
    let text = s.composer.text.clone();
    s.schedule.pending = Some((target.clone(), text.clone()));
    s.schedule.error = None;
    s.actions.schedule_message(target, text, at.with_timezone(&Utc));
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    if !s.schedule.open { return None; }
    let mut body = col().key("schedule-picker").w_full().gap(6.0).padding(8.0)
        .child(text("Schedule send").font(ts(14.0, 600.0)))
        .child(crate::controls::button::<A>("scheduled-load", "Manage scheduled messages", crate::controls::Variant::Secondary, crate::controls::Size::Sm, |s: &mut UiState<A>, _| {
            if !s.schedule.loading && s.schedule.changing.is_none() { s.schedule.loading = true; s.actions.load_scheduled(); }
        }))
        .child(manager(s));
    for preset in app_base::schedule_time::presets(Local::now()) {
        body = body.child(crate::controls::button::<A>(&format!("schedule-{:?}", preset.kind), &format!("{} · {}", preset.label, preset.time_label), crate::controls::Variant::Secondary, crate::controls::Size::Sm, move |s: &mut UiState<A>, _| submit(s, preset.at)));
    }
    let mut days = row().w_full().wrap().gap(4.0);
    for day in app_base::schedule_time::day_choices(Local::now().date_naive()) {
        days = days.child(crate::controls::button::<A>(
            &format!("schedule-day-{}", day.date),
            &day.label,
            if s.schedule.day == Some(day.date) { crate::controls::Variant::Primary } else { crate::controls::Variant::Secondary },
            crate::controls::Size::Sm,
            move |s: &mut UiState<A>, _| s.schedule.day = Some(day.date),
        ));
    }
    Some(body.child(days)
        .child(text_input(&s.schedule.time, ts(14.0, 400.0)).key("schedule-time").label("Local time (HH:MM)").w_full().on_edit(|s: &mut UiState<A>| &mut s.schedule.time))
        .children(blocker(s).map(|reason| text(reason).font(ts(12.0, 400.0))))
        .children(s.schedule.error.as_ref().map(|error| text(error.clone()).key("schedule-error").font(ts(12.0, 400.0))))
        .child(if s.schedule.pending.is_some() {
            crate::controls::button::<A>("schedule-submit", "Scheduling…", crate::controls::Variant::Quiet, crate::controls::Size::Md, |_, _| {})
                .on_press(|_: &mut UiState<A>| {})
        } else {
            crate::controls::button::<A>("schedule-submit", "Schedule", crate::controls::Variant::Primary, crate::controls::Size::Md, |s: &mut UiState<A>, _| {
                let at = s.schedule.day.and_then(|day| app_base::schedule_time::custom_time(day, &s.schedule.time.text));
                match at { Some(at) => submit(s, at), None => s.schedule.error = Some("Enter a time like 14:30.".into()) }
            })
        }))
}

fn change<A: Actions>(s: &mut UiState<A>, draft: chatsvc::ScheduledDraft, text: Option<String>) {
    if s.read_only || s.schedule.changing.is_some() { return; }
    if text.as_ref().is_some_and(|text| text.trim().is_empty()) { s.schedule.error = Some("Message cannot be empty".into()); return; }
    s.schedule.error = None;
    s.schedule.changing = Some(draft.id.clone());
    let html = text.map(|text| {
        let mut edited = s.schedule.edit_draft.clone();
        edited.apply_edit(&text, s.schedule.edit.focus, None);
        edited.to_html()
    });
    s.actions.change_scheduled(draft, html);
}

pub fn changed<A: Actions>(s: &mut UiState<A>, id: &str, result: Result<Option<chatsvc::ScheduledDraft>, String>) {
    if s.schedule.changing.as_deref() != Some(id) { return; }
    s.schedule.changing = None;
    match result {
        Err(error) => s.schedule.error = Some(error),
        Ok(row) => {
            s.schedule.rows.retain(|row| row.id != id);
            if let Some(row) = row { s.schedule.rows.push(row); s.schedule.rows.sort_by_key(|row| row.send_at); }
            s.schedule.edit_id = None;
            s.schedule.cancel_confirm = None;
        }
    }
}

fn manager<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut list = col().key("scheduled-list").w_full().gap(4.0).max_h(220.0).scroll(ScrollSpec { stick_bottom: false });
    if s.schedule.loading { list = list.child(text("Loading scheduled messages…").font(ts(12.0, 400.0))); }
    for draft in &s.schedule.rows {
        let mut entry = col().w_full().gap(4.0)
            .child(text(format!("{} · {}", draft.send_at.with_timezone(&Local).format("%d/%m %H:%M"), crate::bridge::html_to_text(&draft.html))).font(ts(12.0, 400.0)).wrap());
        if s.schedule.edit_id.as_deref() == Some(&draft.id) {
            let draft = draft.clone();
            entry = entry.child(text_input(&s.schedule.edit, ts(13.0, 400.0)).key("scheduled-edit").label("Scheduled message").w_full().on_edit(|s: &mut UiState<A>| &mut s.schedule.edit))
                .child(crate::controls::button::<A>("scheduled-save", "Save changes", crate::controls::Variant::Primary, crate::controls::Size::Sm, move |s: &mut UiState<A>, _| change(s, draft.clone(), Some(s.schedule.edit.text.clone()))));
        } else {
            let draft = draft.clone();
            entry = entry.child(crate::controls::button::<A>(&format!("scheduled-edit-{}", draft.id), "Edit", crate::controls::Variant::Secondary, crate::controls::Size::Sm, move |s: &mut UiState<A>, _| {
                if s.schedule.changing.is_none() {
                    s.schedule.edit_id = Some(draft.id.clone());
                    s.schedule.edit_draft = teams_core::Draft::from_spans(&teams_core::html_to_spans(&draft.html));
                    s.schedule.edit = TextEdit::new(s.schedule.edit_draft.text());
                }
            }));
        }
        let draft = draft.clone();
        let label = if s.schedule.cancel_confirm.as_deref() == Some(&draft.id) { "Confirm cancellation" } else { "Cancel scheduled send" };
        entry = entry.child(crate::controls::button::<A>(
            &format!("scheduled-cancel-{}", draft.id),
            label,
            if s.schedule.cancel_confirm.as_deref() == Some(&draft.id) { crate::controls::Variant::Danger } else { crate::controls::Variant::Secondary },
            crate::controls::Size::Sm,
            move |s: &mut UiState<A>, _| {
            if s.schedule.cancel_confirm.as_deref() == Some(&draft.id) { change(s, draft.clone(), None); }
            else { s.schedule.cancel_confirm = Some(draft.id.clone()); }
        }));
        list = list.child(entry);
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake { scheduled: RefCell<Vec<String>>, saved: RefCell<Vec<String>>, changes: RefCell<Vec<Option<String>>> }
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn schedule_message(&self, _: SendTarget, text: String, _: DateTime<Utc>) { self.scheduled.borrow_mut().push(text); }
        fn save_draft(&self, _: &str, text: &str) { self.saved.borrow_mut().push(text.into()); }
        fn change_scheduled(&self, _: chatsvc::ScheduledDraft, html: Option<String>) { self.changes.borrow_mut().push(html); }
    }

    #[test]
    fn scheduled_edits_preserve_formatting_and_failures_keep_the_editor() {
        let mut s = UiState::new(Fake::default());
        let draft = chatsvc::ScheduledDraft { id: "draft".into(), client_message_id: "client".into(), inner_thread_id: "chat".into(), send_at: Utc::now() + chrono::Duration::hours(1), html: "<p><b>Hello</b> world</p>".into(), delivery_state: None };
        s.schedule.rows.push(draft.clone());
        s.schedule.edit_id = Some(draft.id.clone());
        s.schedule.edit_draft = teams_core::Draft::from_spans(&teams_core::html_to_spans(&draft.html));
        s.schedule.edit = TextEdit::new(s.schedule.edit_draft.text());
        change(&mut s, draft.clone(), Some("Hello world!".into()));
        assert!(s.actions.changes.borrow()[0].as_ref().unwrap().contains("<b>Hello</b>"));
        change(&mut s, draft.clone(), None);
        assert_eq!(s.actions.changes.borrow().len(), 1, "one operation at a time");
        changed(&mut s, "draft", Err("offline".into()));
        assert_eq!(s.schedule.edit_id.as_deref(), Some("draft"));
        assert_eq!(s.schedule.rows.len(), 1);
        change(&mut s, draft, None);
        changed(&mut s, "stale", Ok(None));
        assert_eq!(s.schedule.rows.len(), 1);
        changed(&mut s, "draft", Ok(None));
        assert!(s.schedule.rows.is_empty());
    }

    #[test]
    fn scheduling_validates_time_and_keeps_new_edits_on_confirmation() {
        let mut s = UiState::new(Fake::default());
        s.selected = Some("chat".into());
        s.composer = TextEdit::new("hello");
        submit(&mut s, Local::now() - chrono::Duration::minutes(1));
        assert!(s.actions.scheduled.borrow().is_empty());
        submit(&mut s, Local::now() + chrono::Duration::hours(1));
        assert_eq!(*s.actions.scheduled.borrow(), ["hello"]);
        assert_eq!(s.composer.text, "hello", "keep text until confirmed");
        s.composer = TextEdit::new("new draft");
        let (target, text) = s.schedule.pending.clone().expect("pending schedule");
        s.apply(crate::types::Update::Scheduled { target, text, result: Ok(()) });
        assert_eq!(s.composer.text, "new draft");
        assert!(s.actions.saved.borrow().is_empty(), "do not erase the new persisted draft");
    }

    #[test]
    fn failed_scheduling_keeps_the_draft_and_can_be_retried() {
        let mut s = UiState::new(Fake::default());
        s.selected = Some("chat".into());
        s.composer = TextEdit::new("hello");
        submit(&mut s, Local::now() + chrono::Duration::hours(1));
        let (target, text) = s.schedule.pending.clone().expect("pending schedule");
        s.apply(crate::types::Update::Scheduled { target, text, result: Err("offline".into()) });
        assert_eq!(s.composer.text, "hello");
        assert!(s.schedule.pending.is_none());
        assert_eq!(s.schedule.error.as_deref(), Some("offline"));
        submit(&mut s, Local::now() + chrono::Duration::hours(1));
        assert_eq!(s.actions.scheduled.borrow().len(), 2);
    }
}
