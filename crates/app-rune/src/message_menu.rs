//! The "more" menu of a message and what it opens: inline edit, reply, copy, pin, forward,
//! mark as unread and delete. Mirrors `crates/app/src/views/message_actions.rs` in the GPUI app.

use std::collections::HashSet;

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

use crate::avatar::rgb;
use crate::types::{Actions, MessageRow};
use crate::ui::{UiState, active_target, ts};

const MENU_W: f32 = 200.0;

#[derive(Default)]
pub struct MenuState {
    /// Message being edited in place and its field.
    pub(crate) edit_id: Option<String>,
    pub(crate) edit_text: TextEdit,
    edit_draft: teams_core::Draft,
    edit_original: String,
    pub(crate) edit_saving: bool,
    pub(crate) edit_error: Option<String>,
    /// Messages of the open conversation that carry an edit.
    pub(crate) edited: HashSet<String>,
    /// Message whose forward picker is open.
    pub(crate) forward: Option<String>,
    forward_search: TextEdit,
    forward_comment: TextEdit,
    forward_target: Option<(String, String)>,
    forward_highlighted: usize,
    pub(crate) forward_sending: bool,
    pub(crate) forward_error: Option<String>,
    /// Window point of a right-click: the menu opens there instead of at the "more" button.
    pub(crate) at: Option<(f32, f32)>,
}

/// Opens the menu of `id` at the pointer, as a right-click does.
pub(crate) fn open_at<A: Actions>(s: &mut UiState<A>, id: &str, x: f32, y: f32) {
    close(s);
    s.more_open = Some(id.to_owned());
    s.menu.at = Some((x, y));
}

pub(crate) fn close<A: Actions>(s: &mut UiState<A>) {
    s.more_open = None;
    s.menu.at = None;
    s.picker.close();
    s.confirm_delete = None;
    s.menu.forward = None;
    s.menu.forward_search.clear();
    s.menu.forward_comment.clear();
    s.menu.forward_target = None;
    s.menu.forward_highlighted = 0;
    s.menu.forward_sending = false;
    s.menu.forward_error = None;
}

fn cancel_edit<A: Actions>(s: &mut UiState<A>) {
    s.menu.edit_id = None;
    s.menu.edit_saving = false;
    s.menu.edit_error = None;
}

pub(crate) fn begin_edit<A: Actions>(s: &mut UiState<A>, m: &MessageRow, tx: &mut Tx) {
    if s.read_only || !m.mine || m.deleted || m.system { return; }
    let draft = m.media.rich.edit_draft.clone().unwrap_or_else(|| {
        let spans: Vec<_> = m.media.rich.spans.iter().filter(|span| !matches!(span, teams_core::Span::Quote { .. })).cloned().collect();
        if spans.is_empty() { teams_core::Draft::plain(&m.text) } else { teams_core::Draft::from_spans(&spans).trimmed() }
    });
    s.menu.edit_text = TextEdit::new(draft.text());
    s.menu.edit_original = draft.to_html();
    s.menu.edit_draft = draft;
    s.menu.edit_id = Some(m.id.clone());
    s.menu.edit_saving = false;
    s.menu.edit_error = None;
    close(s);
    tx.focus(format!("edit-field-{}", m.id), false);
}

fn edit_changed<A: Actions>(s: &mut UiState<A>) {
    s.menu.edit_draft.apply_edit(&s.menu.edit_text.text, s.menu.edit_text.focus, None);
}

fn save_edit<A: Actions>(s: &mut UiState<A>) {
    if s.read_only || s.menu.edit_saving { return; }
    let (Some(id), Some(conversation)) = (s.menu.edit_id.clone(), s.selected.clone()) else { return };
    edit_changed(s);
    let draft = s.menu.edit_draft.trimmed();
    let html = draft.to_html();
    if !draft.is_blank() && html != s.menu.edit_original {
        s.menu.edit_saving = true;
        s.menu.edit_error = None;
        s.actions.edit_message(&conversation, &id, &html);
    } else if !draft.is_blank() { cancel_edit(s); }
}

/// The edit field drawn in place of the bubble while `m` is being edited.
pub(crate) fn edit_field<A: Actions + 'static>(s: &UiState<A>, m: &MessageRow) -> Option<El> {
    if s.menu.edit_id.as_deref() != Some(m.id.as_str()) {
        return None;
    }
    let palette = active_palette();
    let field = text_input(&s.menu.edit_text, ts(14.0, 400.0))
        .key(format!("edit-field-{}", m.id))
        .label("Edit message")
        .color(rgb(palette.foreground))
        .w_full()
        .on_edit(|s: &mut UiState<A>| &mut s.menu.edit_text)
        .on_input_change(|s: &mut UiState<A>, _| edit_changed(s))
        .on_input_key(|s: &mut UiState<A>, key, _| {
            if key.key == Key::Escape { cancel_edit(s); true } else { false }
        })
        .on_submit(|s: &mut UiState<A>, _| save_edit(s));
    let field = if s.menu.edit_saving {
        text(s.menu.edit_text.text.clone()).key(format!("edit-field-{}", m.id)).font(ts(14.0, 400.0)).color(rgb(palette.foreground)).wrap()
    } else { field };
    let button = |key: String, label: &'static str, color: u32, on_press: fn(&mut UiState<A>)| {
        row().key(key).h(26.0).px(10.0).radius(6.0).center().hover(rgb(palette.popup_hover))
            .on_press(on_press)
            .child(text(label).font(ts(12.0, 600.0)).color(rgb(color)))
    };
    Some(col().key(format!("edit-{}", m.id)).w(420.0).max_w(560.0).gap(6.0).px(12.0).py(8.0).radius(8.0)
        .border(1.0, rgb(palette.accent)).fill(rgb(palette.surface))
        .child(field)
        .children(s.menu.edit_error.as_ref().map(|error| text(error.clone()).key("edit-error").font(ts(12.0, 400.0)).color(rgb(palette.red)).wrap()))
        .child(row().gap(6.0).justify_end()
            .child(button(format!("edit-cancel-{}", m.id), "Cancel", palette.text_soft, cancel_edit::<A>))
            .child(button(format!("edit-save-{}", m.id), if s.menu.edit_saving { "Saving…" } else { "Save" }, palette.accent, save_edit::<A>))))
}

/// "Edited" under the text of a message that carries an edit.
pub(crate) fn edited_mark<A: Actions>(s: &UiState<A>, m: &MessageRow) -> Option<El> {
    (!m.deleted && s.menu.edited.contains(&m.id))
        .then(|| text("Edited").key(format!("edited-{}", m.id)).font(ts(11.0, 400.0)).color(rgb(active_palette().text_muted)))
}

fn item<A: Actions + 'static>(key: String, label: &str, color: u32, on_press: impl Fn(&mut UiState<A>, &mut Tx) + 'static) -> El {
    row().key(key).h(28.0).px(8.0).radius(6.0).center_y().hover(rgb(active_palette().popup_hover))
        .on_press_tx(on_press)
        .child(text(label.to_owned()).font(ts(12.0, 400.0)).color(rgb(color)))
}

/// The open message menu, floating over the window: at the right-click point, or under the
/// message's "more" button. Quick reactions head it; Forward opens a modal dialog.
/// The menu flips when it would leave the window (`El::anchored`).
/// No open animation: the window lands every change at once (`ui::view`).
pub(crate) fn overlay<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    if let Some(id) = &s.menu.forward { return Some(forward_dialog(s, id)); }
    let id = s.more_open.as_deref()?;
    let m = s.search_results.as_ref().unwrap_or(&s.messages).iter().find(|m| m.id == id)?;
    if s.read_only || m.deleted {
        return None;
    }
    let mut layer = el().key("message-menu-layer").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .child(el().key("message-menu-backdrop").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).on_press(close::<A>));
    let anchor = match s.menu.at {
        Some((x, y)) => {
            layer = layer.child(el().key("message-menu-at").size(1.0, 1.0).pin(Some(x), Some(y), None, None));
            "message-menu-at".to_owned()
        }
        None => format!("more-{id}"),
    };
    layer = layer.child(col().key("message-menu").w(MENU_W).gap(6.0)
        .anchored(anchor, Side::Bottom, Align::Start, 4.0)
        .child(crate::reactions::quick_bar(s, m))
        .child(list(s, m)));
    Some(layer)
}

fn choose_forward<A: Actions>(s: &mut UiState<A>, target: (String, String), tx: &mut Tx) {
    if s.menu.forward_sending { return; }
    s.menu.forward_target = Some(target);
    s.menu.forward_error = None;
    tx.focus("forward-comment", false);
}

fn send_forward<A: Actions>(s: &mut UiState<A>) {
    if s.read_only || s.menu.forward_sending { return; }
    let (Some(source), Some(id), Some((target, _))) = (&s.selected, &s.menu.forward, &s.menu.forward_target) else { return };
    s.menu.forward_sending = true;
    s.menu.forward_error = None;
    s.actions.forward_message(source, target, id, s.menu.forward_comment.text.trim());
}

fn forward_dialog<A: Actions + 'static>(s: &UiState<A>, id: &str) -> El {
    let palette = active_palette();
    let mut body = col().key(format!("forward-{id}")).w_full().padding(16.0).gap(12.0)
        .child(text("Forward this message").font(ts(18.0, 600.0)).color(rgb(palette.foreground)))
        .child(text("You can forward to any chat or channel.").font(ts(12.0, 400.0)).color(rgb(palette.text_muted)));
    if let Some(message) = s.messages.iter().find(|m| m.id == id) {
        body = body.child(text(message.text.clone()).font(ts(13.0, 400.0)).color(rgb(palette.foreground)).truncate());
    }
    if let Some((_, title)) = &s.menu.forward_target {
        body = body.child(row().w_full().gap(8.0).center_y()
            .child(text(title.clone()).font(ts(14.0, 600.0)).color(rgb(palette.foreground)).grow(1.0).min_w(0.0).truncate())
            .child(crate::screens::button("Change", "forward-change", |s: &mut UiState<A>, tx| {
                if !s.menu.forward_sending { s.menu.forward_target = None; tx.focus("forward-search", false); }
            })));
    } else {
        let found = crate::switcher::candidates(s, &s.menu.forward_search.text);
        let highlighted = s.menu.forward_highlighted.min(found.len().saturating_sub(1));
        body = body.child(text_input(&s.menu.forward_search, ts(14.0, 400.0)).key("forward-search")
            .label("Search chats and channels").placeholder("Search chats and channels", rgb(palette.text_muted)).w_full()
            .on_edit(|s: &mut UiState<A>| &mut s.menu.forward_search)
            .on_input_change(|s: &mut UiState<A>, _| s.menu.forward_highlighted = 0)
            .on_input_key(|s: &mut UiState<A>, key, tx| {
                let found = crate::switcher::candidates(s, &s.menu.forward_search.text);
                match key.key {
                    Key::Enter => {
                        if let Some(target) = found.get(s.menu.forward_highlighted.min(found.len().saturating_sub(1))) { choose_forward(s, target.clone(), tx); }
                    }
                    Key::Down => s.menu.forward_highlighted = (s.menu.forward_highlighted + 1).min(found.len().saturating_sub(1)),
                    Key::Up => s.menu.forward_highlighted = s.menu.forward_highlighted.saturating_sub(1),
                    _ => return false,
                }
                true
            }));
        let mut results = col().key("forward-results").w_full();
        if found.is_empty() {
            results = results.child(text("No matching chats or channels").font(ts(13.0, 400.0)).color(rgb(palette.text_muted)));
        }
        for (i, target) in found.into_iter().enumerate() {
            let choice = target.clone();
            let candidate = item(format!("forward-to-{}", target.0), &target.1, palette.foreground,
                move |s: &mut UiState<A>, tx| choose_forward(s, choice.clone(), tx));
            results = results.child(if i == highlighted { candidate.fill(rgb(palette.popup_hover)) } else { candidate });
        }
        body = body.child(results);
    }
    body = body.child(text_input(&s.menu.forward_comment, ts(14.0, 400.0)).key("forward-comment")
        .label("Add a message (optional)").placeholder("Add a message (optional)", rgb(palette.text_muted)).w_full()
        .on_edit(|s: &mut UiState<A>| &mut s.menu.forward_comment)
        .on_submit(|s: &mut UiState<A>, _| send_forward(s)))
        .children(s.menu.forward_error.as_ref().map(|error| text(error.clone()).key("forward-error").font(ts(12.0, 400.0)).color(rgb(palette.red)).wrap()))
        .child(row().w_full().gap(8.0).justify_end()
            .child(crate::screens::button("Cancel", "forward-cancel", |s: &mut UiState<A>, _| close(s)))
            .child(crate::screens::button(if s.menu.forward_sending { "Forwarding…" } else { "Forward" }, "forward-send", |s: &mut UiState<A>, _| send_forward(s))
                .opacity(if s.menu.forward_target.is_some() && !s.menu.forward_sending { 1.0 } else { 0.4 })));
    crate::shortcuts::card("forward-dialog", 480.0, body, close::<A>)
}

fn list<A: Actions + 'static>(s: &UiState<A>, m: &MessageRow) -> El {
    let palette = active_palette();
    let ink = palette.foreground;
    let id = m.id.clone();
    let conversation = s.selected.clone().unwrap_or_default();
    let channel = active_target(s).is_some_and(|target| target.channel);
    let mut list = col().key(format!("menu-{id}")).w_full().gap(1.0).padding(4.0).radius(8.0)
        .fill(rgb(palette.popup)).border(1.0, rgb(palette.border_strong));

    if !channel {
        let reply = crate::types::ReplyReference { message_id: id.clone(), sender: m.sender.clone(), text: m.text.clone() };
        list = list.child(item(format!("menu-reply-{id}"), "Reply", ink, move |s: &mut UiState<A>, tx| {
            s.compose.reply = Some(reply.clone());
            close(s);
            tx.focus("composer", false);
        }));
    }
    let body = m.text.clone();
    list = list.child(item(format!("menu-copy-{id}"), "Copy text", ink, move |s: &mut UiState<A>, tx| {
        tx.copy(body.clone());
        close(s);
    }));
    if m.mine {
        let message = m.clone();
        list = list.child(item(format!("menu-edit-{id}"), "Edit", ink, move |s: &mut UiState<A>, tx| {
            begin_edit(s, &message, tx);
        }));
    }
    // ponytail: the pin bar carries no message id, so "pinned" is matched by text; add the id to `PinnedRow` if duplicates matter.
    let pinned = s.pinned.as_ref().is_some_and(|pin| pin.text == m.text);
    {
        let (pin_id, conversation) = (id.clone(), conversation.clone());
        list = list.child(item(format!("menu-pin-{id}"), if pinned { "Unpin" } else { "Pin" }, ink, move |s: &mut UiState<A>, _| {
            s.actions.pin_message(&conversation, &pin_id, !pinned);
            close(s);
        }));
    }
    {
        let fwd_id = id.clone();
        list = list.child(item(format!("menu-forward-{id}"), "Forward", ink, move |s: &mut UiState<A>, tx| {
            close(s);
            s.menu.forward = Some(fwd_id.clone());
            tx.focus("forward-search", false);
        }));
    }
    if !channel {
        let unread_id = id.clone();
        list = list.child(item(format!("menu-unread-{id}"), "Mark as unread", ink, move |s: &mut UiState<A>, _| {
            s.actions.mark_unread_from(&conversation, &unread_id);
            close(s);
        }));
    }
    if m.can_delete {
        let conversation = s.selected.clone().unwrap_or_default();
        let confirming = s.confirm_delete.as_deref() == Some(id.as_str());
        let delete_id = id.clone();
        list = list.child(item(format!("delete-{id}"), if confirming { "Confirm delete" } else { "Delete" }, palette.red, move |s: &mut UiState<A>, _| {
            if confirming {
                s.actions.delete_message(&conversation, &delete_id);
                close(s);
            } else {
                s.confirm_delete = Some(delete_id.clone());
            }
        }));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRow, SendTarget, Update};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake { calls: RefCell<Vec<String>> }

    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn send_composed(&self, target: SendTarget, composed: crate::types::Composed, _: Vec<teams_core::FileReference>) {
            self.calls.borrow_mut().push(format!("reply {} {} {}", target.conversation_id, composed.reply.map(|r| r.message_id).unwrap_or_default(), composed.text));
        }
        fn edit_message(&self, c: &str, id: &str, text: &str) { self.calls.borrow_mut().push(format!("edit {c} {id} {text}")); }
        fn pin_message(&self, c: &str, id: &str, pin: bool) { self.calls.borrow_mut().push(format!("pin {c} {id} {pin}")); }
        fn forward_message(&self, s: &str, t: &str, id: &str, comment: &str) { self.calls.borrow_mut().push(format!("forward {s} {t} {id} [{comment}]")); }
        fn mark_unread_from(&self, c: &str, id: &str) { self.calls.borrow_mut().push(format!("unread {c} {id}")); }
    }

    fn msg(id: &str, mine: bool, text: &str) -> MessageRow {
        MessageRow {
            forwarded: false, importance: Default::default(),
            sender_id: None, system: false, id: id.into(), reply_to_id: None, deleted: false, can_delete: false,
            reactions: vec![], sender: if mine { "Me".into() } else { "Ada".into() }, when: "14:06".into(),
            text: text.into(), mine, media: Default::default(),
        }
    }

    fn show(rows: Vec<MessageRow>) -> Runtime<UiState<Fake>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Fake::default()), crate::ui::view::<Fake>, (1000.0, 700.0), env);
        rt.frame(0.0);
        rt.update(move |s, _| {
            s.selected = Some("a".into());
            s.chats = vec![ChatRow { id: "b".into(), title: "Bo".into(), preview: String::new(), unread: false, when: String::new() }];
            s.apply(Update::Messages { conversation_id: "a".into(), rows });
        });
        rt.frame(1.0 / 60.0);
        rt
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn open_menu(rt: &mut Runtime<UiState<Fake>>, id: &str) {
        click(rt, &format!("more-{id}"));
        assert!(rt.find(&format!("menu-{id}")).is_some());
    }

    fn last_call(rt: &Runtime<UiState<Fake>>) -> String {
        rt.state.actions.calls.borrow().last().cloned().unwrap_or_default()
    }

    #[test]
    fn edit_replaces_the_bubble_and_enter_saves() {
        let mut rt = show(vec![msg("m1", true, "hello")]);
        open_menu(&mut rt, "m1");
        click(&mut rt, "menu-edit-m1");
        assert!(rt.find("edit-field-m1").is_some());
        assert!(rt.find("bubble-m1").is_none());
        rt.update(|s, _| s.menu.edit_text = TextEdit::new("hello there"));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "edit-field-m1");
        rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        assert_eq!(last_call(&rt), "edit a m1 hello there");
        assert!(rt.state.menu.edit_saving);
        rt.update(|s, _| s.apply(Update::EditFinished { conversation_id: "a".into(), message_id: "m1".into(), result: Ok(()) }));
        rt.frame(1.0 / 60.0);
        assert!(rt.find("edit-field-m1").is_none());
    }

    #[test]
    fn failed_edit_keeps_the_text_and_allows_retry() {
        let mut rt = show(vec![msg("m1", true, "hello")]);
        open_menu(&mut rt, "m1");
        click(&mut rt, "menu-edit-m1");
        click(&mut rt, "edit-field-m1");
        rt.key(KeyEvent { key: Key::Text(" there".into()), mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        let edited = rt.state.menu.edit_text.text.clone();
        click(&mut rt, "edit-save-m1");
        click(&mut rt, "edit-save-m1");
        assert_eq!(rt.state.actions.calls.borrow().len(), 1);
        rt.update(|s, _| s.apply(Update::EditFinished { conversation_id: "a".into(), message_id: "m1".into(), result: Err("offline".into()) }));
        rt.frame(1.0 / 60.0);
        assert!(rt.find("edit-error").is_some());
        assert_eq!(rt.state.menu.edit_text.text, edited);
        click(&mut rt, "edit-save-m1");
        assert_eq!(rt.state.actions.calls.borrow().len(), 2);
    }

    #[test]
    fn editing_a_channel_post_uses_the_body_without_duplicating_its_subject() {
        let record = store::MessageRecord {
            conversation_id: "a".into(), message_id: "post".into(), sender_id: Some("me".into()),
            body_html: "<b>Body</b>".into(), subject: Some("Subject".into()), ..Default::default()
        };
        let rows = crate::bridge::message_rows(&[record], "me", chrono::Local::now());
        let mut rt = show(rows);
        open_menu(&mut rt, "post");
        click(&mut rt, "menu-edit-post");
        assert_eq!(rt.state.menu.edit_text.text, "Body");
        assert_eq!(rt.state.menu.edit_draft.to_html(), "<b>Body</b>");
    }

    #[test]
    fn up_in_an_empty_composer_edits_the_last_own_message_and_keeps_formatting() {
        let mut formatted = msg("own", true, "hello Ada");
        formatted.media.rich.spans = teams_core::html_to_spans("<b>hello</b> <at id=\"0\">Ada</at>");
        let mut rt = show(vec![formatted, msg("deleted", true, "gone"), msg("other", false, "later")]);
        rt.update(|s, _| s.messages[1].deleted = true);
        click(&mut rt, "composer");
        rt.key(KeyEvent { key: Key::Up, mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        assert!(rt.find("edit-field-own").is_some());
        assert_eq!(rt.state.menu.edit_text.text, "hello @Ada");
        click(&mut rt, "edit-field-own");
        rt.key(KeyEvent { key: Key::End, mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        rt.key(KeyEvent { key: Key::Text("!".into()), mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.menu.edit_text.text, "hello @Ada!");
        rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        assert_eq!(last_call(&rt), "edit a own <b>hello</b> @Ada!");
        let (html, mentions) = teams_core::apply_mentions("<b>hello</b> @Ada!", &[teams_core::MentionInput::user("ada", "Ada")]);
        assert_eq!(html, "<b>hello</b> <at id=\"0\">Ada</at>!");
        assert_eq!(mentions.len(), 1);
        rt.update(|s, _| s.apply(Update::EditFinished { conversation_id: "a".into(), message_id: "own".into(), result: Ok(()) }));
        rt.update(|s, _| s.composer = TextEdit::new("draft"));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "composer");
        rt.key(KeyEvent { key: Key::Up, mods: Mods::default() });
        assert!(rt.state.menu.edit_id.is_none());
    }

    #[test]
    fn escape_cancels_the_edit_without_a_call() {
        let mut rt = show(vec![msg("m1", true, "hello")]);
        open_menu(&mut rt, "m1");
        click(&mut rt, "menu-edit-m1");
        click(&mut rt, "edit-field-m1");
        rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        assert!(rt.find("edit-field-m1").is_none());
        assert!(rt.state.actions.calls.borrow().is_empty());
    }

    #[test]
    fn edit_is_offered_on_own_messages_only_and_edited_ones_are_marked() {
        let mut rt = show(vec![msg("m1", false, "hi"), msg("m2", true, "yo")]);
        rt.update(|s, _| s.apply(Update::Edited { conversation_id: "a".into(), ids: vec!["m2".into()] }));
        rt.frame(1.0 / 60.0);
        assert!(rt.find("edited-m2").is_some());
        assert!(rt.find("edited-m1").is_none());
        open_menu(&mut rt, "m1");
        assert!(rt.find("menu-edit-m1").is_none());
    }

    #[test]
    fn reply_uses_a_cancellable_reference_and_sends_its_message_id() {
        let mut rt = show(vec![msg("m1", false, "hi there")]);
        rt.update(|s, _| s.composer = TextEdit::new("my answer"));
        open_menu(&mut rt, "m1");
        click(&mut rt, "menu-reply-m1");
        assert_eq!(rt.state.composer.text, "my answer");
        assert!(rt.find("reply-reference").is_some());
        assert!(rt.find("menu-m1").is_none());
        click(&mut rt, "reply-cancel");
        assert!(rt.find("reply-reference").is_none());
        open_menu(&mut rt, "m1");
        click(&mut rt, "menu-reply-m1");
        rt.update(|s, tx| crate::ui::open_chat(s, "b", tx));
        assert!(rt.state.compose.reply.is_none());
        rt.update(|s, tx| crate::ui::open_chat(s, "a", tx));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "send");
        assert_eq!(last_call(&rt), "reply a m1 my answer");
        assert!(rt.find("reply-reference").is_none());
        rt.update(|s, _| s.apply(Update::SendFailed {
            target: SendTarget { conversation_id: "a".into(), thread_root_id: None, channel: false },
            text: "my answer".into(), reply: Some(crate::types::ReplyReference { message_id: "m1".into(), sender: "Ada".into(), text: "hi there".into() }), files: vec![], error: "offline".into(),
        }));
        rt.frame(1.0 / 60.0);
        assert!(rt.find("reply-reference").is_some());
        assert_eq!(rt.state.composer.text, "my answer");
    }

    #[test]
    fn copy_text_closes_the_menu() {
        let mut rt = show(vec![msg("m1", false, "hi")]);
        open_menu(&mut rt, "m1");
        click(&mut rt, "menu-copy-m1");
        assert!(rt.find("menu-m1").is_none());
    }

    #[test]
    fn pin_and_unpin_follow_the_pinned_bar() {
        let mut rt = show(vec![msg("m1", false, "hi")]);
        open_menu(&mut rt, "m1");
        click(&mut rt, "menu-pin-m1");
        assert_eq!(last_call(&rt), "pin a m1 true");
        rt.update(|s, _| s.pinned = Some(crate::types::PinnedRow { conversation_id: "a".into(), sender: "Ada".into(), text: "hi".into() }));
        open_menu(&mut rt, "m1");
        click(&mut rt, "menu-pin-m1");
        assert_eq!(last_call(&rt), "pin a m1 false");
    }

    #[test]
    fn forward_picks_another_chat() {
        let mut rt = show(vec![msg("m1", false, "hi")]);
        open_menu(&mut rt, "m1");
        click(&mut rt, "menu-forward-m1");
        click(&mut rt, "forward-to-b");
        assert!(rt.state.actions.calls.borrow().is_empty());
        click(&mut rt, "forward-send");
        assert_eq!(last_call(&rt), "forward a b m1 []");
        rt.update(|s, _| s.apply(Update::Forwarded { conversation_id: "a".into(), message_id: "m1".into(), result: Ok(()) }));
        rt.frame(1.0 / 60.0);
        assert!(rt.find("forward-dialog").is_none());
        assert!(rt.find("menu-m1").is_none());
    }

    #[test]
    fn forward_search_reaches_later_chats_and_channels_and_keeps_comment_on_failure() {
        let mut rt = show(vec![msg("m1", false, "hi")]);
        rt.update(|s, _| {
            s.chats.extend((0..12).map(|i| ChatRow { id: format!("chat-{i}"), title: format!("Chat {i}"), preview: String::new(), unread: false, when: String::new() }));
            s.teams = vec![crate::types::TeamRow { id: "team".into(), name: "Team".into(), channels: vec![crate::types::ChannelRow { id: "channel".into(), name: "General".into(), unread: false, hidden: false }] }];
        });
        open_menu(&mut rt, "m1");
        click(&mut rt, "menu-forward-m1");
        click(&mut rt, "forward-send");
        assert!(rt.state.actions.calls.borrow().is_empty());
        click(&mut rt, "forward-search");
        rt.key(KeyEvent { key: Key::Text("Chat 11".into()), mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        assert!(rt.find("forward-to-chat-11").is_some());
        assert!(rt.find("forward-to-b").is_none());
        rt.update(|s, _| s.menu.forward_search = TextEdit::new("General"));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "forward-to-channel");
        click(&mut rt, "forward-comment");
        rt.key(KeyEvent { key: Key::Text("  Please review  ".into()), mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        click(&mut rt, "forward-send");
        click(&mut rt, "forward-send");
        assert_eq!(*rt.state.actions.calls.borrow(), ["forward a channel m1 [Please review]"]);
        rt.update(|s, _| s.apply(Update::Forwarded { conversation_id: "a".into(), message_id: "m1".into(), result: Err("offline".into()) }));
        rt.frame(1.0 / 60.0);
        assert!(rt.find("forward-error").is_some());
        assert_eq!(rt.state.menu.forward_comment.text.trim(), "Please review");
        click(&mut rt, "forward-send");
        assert_eq!(rt.state.actions.calls.borrow().len(), 2);
        click(&mut rt, "forward-cancel");
        assert!(rt.find("forward-dialog").is_none());
    }

    fn presented(rt: &Runtime<UiState<Fake>>, key: &str) -> [f32; 4] {
        rt.presented(key).unwrap_or_else(|| panic!("no element {key}")).0
    }

    #[test]
    fn right_click_opens_the_menu_at_the_pointer_and_flips_it_above_near_the_bottom() {
        let mut rt = show(vec![msg("m1", false, "hi")]);
        let (x, y) = rt.find("bubble-m1").unwrap();
        rt.secondary_down(x, y);
        rt.secondary_up(x, y);
        rt.frame(1.0 / 60.0);
        let menu = presented(&rt, "message-menu");
        assert!((menu[0] - x).abs() < 1.0 && menu[1] >= y && menu[1] - y < 8.0, "menu at {menu:?} for a click at ({x}, {y})");
        let bar = presented(&rt, "menu-react-m1");
        assert!(bar[1] < presented(&rt, "menu-m1")[1], "quick reactions head the list");

        // Near the bottom right corner it goes above and left of the pointer, reactions still on top.
        rt.update(|s, _| crate::message_menu::open_at(s, "m1", 990.0, 690.0));
        rt.frame(1.0 / 60.0);
        let menu = presented(&rt, "message-menu");
        assert!(menu[1] + menu[3] <= 690.0 && menu[0] + menu[2] <= 1000.0, "flipped into the window: {menu:?}");
        assert!(presented(&rt, "menu-react-m1")[1] < presented(&rt, "menu-m1")[1], "not upside down");

        click(&mut rt, "menu-forward-m1");
        assert!(rt.find("forward-dialog").is_some());
        assert!(rt.find("message-menu").is_none());
        click(&mut rt, "forward-to-b");
        click(&mut rt, "forward-send");
        assert_eq!(last_call(&rt), "forward a b m1 []");
        assert!(rt.find("message-menu").is_none());
    }

    #[test]
    fn a_click_outside_closes_the_menu() {
        let mut rt = show(vec![msg("m1", false, "hi")]);
        open_menu(&mut rt, "m1");
        rt.pointer_down(500.0, 20.0);
        rt.pointer_up(500.0, 20.0);
        rt.frame(1.0 / 60.0);
        assert!(rt.find("menu-m1").is_none());
    }

    #[test]
    fn mark_unread_asks_the_bridge() {
        let mut rt = show(vec![msg("m1", false, "hi")]);
        open_menu(&mut rt, "m1");
        click(&mut rt, "menu-unread-m1");
        assert_eq!(last_call(&rt), "unread a m1");
    }
}
