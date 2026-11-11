//! The "more" menu of a message and what it opens: inline edit, quote-reply, copy, pin, forward,
//! mark as unread and delete. Mirrors `crates/app/src/views/message_actions.rs` in the GPUI app.

use std::collections::HashSet;

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

use crate::avatar::rgb;
use crate::types::{Actions, MessageRow};
use crate::ui::{UiState, active_target, ts};

/// Chats offered by the forward picker.
const FORWARD_CHATS: usize = 8;
const MENU_W: f32 = 200.0;

#[derive(Default)]
pub struct MenuState {
    /// Message being edited in place and its field.
    pub(crate) edit_id: Option<String>,
    pub(crate) edit_text: TextEdit,
    /// Messages of the open conversation that carry an edit.
    pub(crate) edited: HashSet<String>,
    /// Message whose forward picker is open.
    pub(crate) forward: Option<String>,
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
}

fn cancel_edit<A: Actions>(s: &mut UiState<A>) {
    s.menu.edit_id = None;
}

fn save_edit<A: Actions>(s: &mut UiState<A>) {
    let (Some(id), Some(conversation)) = (s.menu.edit_id.take(), s.selected.clone()) else { return };
    let new_text = s.menu.edit_text.text.trim().to_owned();
    let unchanged = s.messages.iter().find(|m| m.id == id).is_none_or(|m| m.text == new_text);
    if !new_text.is_empty() && !unchanged {
        s.actions.edit_message(&conversation, &id, &new_text);
    }
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
        .on_input_key(|s: &mut UiState<A>, key, _| {
            if key.key == Key::Escape { cancel_edit(s); true } else { false }
        })
        .on_submit(|s: &mut UiState<A>, _| save_edit(s));
    let button = |key: String, label: &'static str, color: u32, on_press: fn(&mut UiState<A>)| {
        row().key(key).h(26.0).px(10.0).radius(6.0).center().hover(rgb(palette.popup_hover))
            .on_press(on_press)
            .child(text(label).font(ts(12.0, 600.0)).color(rgb(color)))
    };
    Some(col().key(format!("edit-{}", m.id)).w(420.0).max_w(560.0).gap(6.0).px(12.0).py(8.0).radius(8.0)
        .border(1.0, rgb(palette.accent)).fill(rgb(palette.surface))
        .child(field)
        .child(row().gap(6.0).justify_end()
            .child(button(format!("edit-cancel-{}", m.id), "Cancel", palette.text_soft, cancel_edit::<A>))
            .child(button(format!("edit-save-{}", m.id), "Save", palette.accent, save_edit::<A>))))
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
/// message's "more" button. Quick reactions head it; Forward opens a submenu beside it. Both
/// flip to the other side when they would leave the window (`El::anchored`), and keep their order.
/// No open animation: the window lands every change at once (`ui::view`).
pub(crate) fn overlay<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let id = s.more_open.as_deref()?;
    let m = s.search_results.as_ref().unwrap_or(&s.messages).iter().find(|m| m.id == id)?;
    if s.read_only || m.deleted {
        return None;
    }
    let palette = active_palette();
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
    if s.menu.forward.as_deref() == Some(id) {
        let conversation = s.selected.clone().unwrap_or_default();
        let mut picker = col().key(format!("forward-{id}")).w(MENU_W).gap(1.0).padding(4.0).radius(8.0)
            .fill(rgb(palette.popup)).border(1.0, rgb(palette.border_strong))
            .anchored(format!("menu-forward-{id}"), Side::Right, Align::Start, 4.0);
        for chat in s.chats.iter().filter(|chat| chat.id != conversation).take(FORWARD_CHATS) {
            let (target, message, source) = (chat.id.clone(), id.to_owned(), conversation.clone());
            picker = picker.child(item(format!("forward-to-{}", chat.id), &chat.title, palette.foreground, move |s: &mut UiState<A>, _| {
                s.actions.forward_message(&source, &target, &message);
                close(s);
            }));
        }
        layer = layer.child(picker);
    }
    Some(layer)
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
        let (sender, body) = (m.sender.clone(), m.text.clone());
        list = list.child(item(format!("menu-reply-{id}"), "Reply", ink, move |s: &mut UiState<A>, tx| {
            let quoted = body.lines().map(|line| format!("> {line}\n")).collect::<String>();
            s.composer.insert(&format!("> {sender}:\n{quoted}\n"));
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
        let (edit_id, body) = (id.clone(), m.text.clone());
        list = list.child(item(format!("menu-edit-{id}"), "Edit", ink, move |s: &mut UiState<A>, tx| {
            s.menu.edit_text = TextEdit::new(body.clone());
            s.menu.edit_id = Some(edit_id.clone());
            close(s);
            tx.focus(format!("edit-field-{edit_id}"), false);
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
        list = list.child(item(format!("menu-forward-{id}"), "Forward  ›", ink, move |s: &mut UiState<A>, _| {
            s.menu.forward = (s.menu.forward.as_deref() != Some(fwd_id.as_str())).then(|| fwd_id.clone());
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
        fn edit_message(&self, c: &str, id: &str, text: &str) { self.calls.borrow_mut().push(format!("edit {c} {id} {text}")); }
        fn pin_message(&self, c: &str, id: &str, pin: bool) { self.calls.borrow_mut().push(format!("pin {c} {id} {pin}")); }
        fn forward_message(&self, s: &str, t: &str, id: &str) { self.calls.borrow_mut().push(format!("forward {s} {t} {id}")); }
        fn mark_unread_from(&self, c: &str, id: &str) { self.calls.borrow_mut().push(format!("unread {c} {id}")); }
    }

    fn msg(id: &str, mine: bool, text: &str) -> MessageRow {
        MessageRow {
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
        assert!(rt.find("edit-field-m1").is_none());
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
    fn reply_quotes_the_message_into_the_composer() {
        let mut rt = show(vec![msg("m1", false, "hi there")]);
        open_menu(&mut rt, "m1");
        click(&mut rt, "menu-reply-m1");
        assert_eq!(rt.state.composer.text, "> Ada:\n> hi there\n\n");
        assert!(rt.find("menu-m1").is_none());
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
        assert_eq!(last_call(&rt), "forward a b m1");
        assert!(rt.find("menu-m1").is_none());
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
        let picker = presented(&rt, "forward-m1");
        assert!(picker[0] + picker[2] <= menu[0] + 1.0, "the submenu opens on the side with room: {picker:?}");
        click(&mut rt, "forward-to-b");
        assert_eq!(last_call(&rt), "forward a b m1");
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
