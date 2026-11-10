//! The Rune view of the Teams client: chat list on the left, the open conversation on the
//! right, the composer under it. Bridge updates come in through `UiState::apply`; user intent
//! goes out through the `Actions` the state owns.

use crate::types::{Actions, ChatRow, MessageRow, Update};
use rune::tok::Family;
use rune::*;
use std::borrow::Cow;

const PURPLE: &str = "#5B5FC7";
const LAVENDER: &str = "#E8EBFA";
const LIST: &str = "#F5F5F5";
const BUBBLE: &str = "#F5F5F5";
const HOVER: &str = "#EBEBEB";
const LINE: &str = "#E0E0E0";
const INK: &str = "#242424";
const MUTED: &str = "#616161";
const RED: &str = "#C4314B";
const WHITE: &str = "#FFFFFF";

const CHAT_ROW_H: f32 = 56.0;
const MESSAGE_ROW_H: f32 = 64.0;

fn c(s: &str) -> Color {
    rune::color::hex(s).expect("hex colour")
}

fn ts(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Segoe UI"))
}

/// Everything the window shows. `apply` folds bridge updates in; `view` reads the rest.
pub struct UiState<A: Actions> {
    pub actions: A,
    pub status: String,
    pub chats: Vec<ChatRow>,
    pub selected: Option<String>,
    /// Messages of `selected`, oldest first.
    pub messages: Vec<MessageRow>,
    pub send_error: Option<String>,
    pub composer: TextEdit,
}

impl<A: Actions> UiState<A> {
    pub fn new(actions: A) -> Self {
        UiState {
            actions,
            status: String::new(),
            chats: Vec::new(),
            selected: None,
            messages: Vec::new(),
            send_error: None,
            composer: TextEdit::new(""),
        }
    }

    /// Applies one bridge update. Messages for a conversation other than `selected` are dropped.
    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Status(s) => self.status = s,
            Update::Chats(rows) => self.chats = rows,
            Update::Messages { conversation_id, rows } => {
                if self.selected.as_deref() == Some(conversation_id.as_str()) {
                    self.messages = rows;
                }
            }
            Update::SendFailed(e) => self.send_error = Some(e),
        }
    }
}

fn open_chat<A: Actions>(s: &mut UiState<A>, id: &str, tx: &mut Tx) {
    s.selected = Some(id.to_owned());
    s.messages.clear();
    s.send_error = None;
    s.actions.open(id);
    tx.scroll_to_bottom_instant("messages");
}

fn send<A: Actions>(s: &mut UiState<A>) {
    let Some(id) = s.selected.clone() else { return };
    let text = s.composer.text.trim().to_owned();
    if text.is_empty() {
        return;
    }
    s.send_error = None;
    s.actions.send(&id, text);
    s.composer.clear();
}

pub fn view<A: Actions + 'static>(s: &UiState<A>, _cx: &mut Cx<UiState<A>>) -> El {
    row().key("root").w_full().h_full().fill(c(WHITE)).child(chat_pane(s)).child(conversation(s))
}

fn chat_pane<A: Actions + 'static>(s: &UiState<A>) -> El {
    let ids: Vec<&str> = s.chats.iter().map(|ch| ch.id.as_str()).collect();
    let rows = col()
        .key("chat-rows")
        .w_full()
        .px(8.0)
        .pb(8.0)
        .growth(GrowthSpec::default())
        .virtual_rows(
            s.chats.len(),
            CHAT_ROW_H + 2.0,
            |s: &UiState<A>, i| Cow::Owned(format!("chat-{}", s.chats[i].id)),
            |s: &UiState<A>, i| chat_row(s, i),
        )
        .virtual_rows_version(ids);
    col()
        .key("chats")
        .w(320.0)
        .h_full()
        .min_h(0.0)
        .fill(c(LIST))
        .child(
            row().w_full().h(32.0).px(16.0).center_y().child(
                text(s.status.clone()).font(ts(12.0, 400.0)).color(c(MUTED)).truncate().grow(1.0).min_w(0.0),
            ),
        )
        .child(
            col()
                .key("chat-list")
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .scroll(ScrollSpec { stick_bottom: false })
                .child(rows),
        )
}

fn chat_row<A: Actions + 'static>(s: &UiState<A>, i: usize) -> El {
    let ch = &s.chats[i];
    let selected = s.selected.as_deref() == Some(ch.id.as_str());
    let id = ch.id.clone();
    let title_weight = if ch.unread { 700.0 } else { 400.0 };
    let line = row()
        .w_full()
        .gap(6.0)
        .center_y()
        .child(text(ch.title.clone()).font(ts(14.0, title_weight)).color(c(INK)).truncate().grow(1.0).min_w(0.0))
        .child(text(ch.when.clone()).font(ts(12.0, 400.0)).color(c(MUTED)));
    let card = col()
        .w_full()
        .h(CHAT_ROW_H)
        .px(10.0)
        .py(8.0)
        .gap(2.0)
        .radius(8.0)
        .fill(c(if selected { WHITE } else { LIST }))
        .hover(c(HOVER))
        .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| open_chat(s, &id, tx))
        .child(line)
        .child(text(ch.preview.clone()).font(ts(13.0, 400.0)).color(c(MUTED)).truncate());
    col().w_full().pb(2.0).child(card)
}

fn conversation<A: Actions + 'static>(s: &UiState<A>) -> El {
    let Some(id) = s.selected.as_deref() else {
        return col()
            .key("conversation")
            .grow(1.0)
            .min_w(0.0)
            .h_full()
            .center()
            .fill(c(WHITE))
            .child(text("Select a chat").font(ts(16.0, 400.0)).color(c(MUTED)));
    };
    let title = s.chats.iter().find(|ch| ch.id == id).map_or(id, |ch| ch.title.as_str());
    col()
        .key("conversation")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .fill(c(WHITE))
        .child(
            row().w_full().h(56.0).px(20.0).center_y().child(
                text(title.to_owned()).font(ts(16.0, 700.0)).color(c(INK)).truncate().grow(1.0).min_w(0.0),
            ),
        )
        .child(el().h(1.0).w_full().fill(c(LINE)))
        .child(message_list(s))
        .child(composer(s))
}

fn message_list<A: Actions + 'static>(s: &UiState<A>) -> El {
    // The version is the id sequence: a reload with the same count but other messages must re-key.
    let ids: Vec<&str> = s.messages.iter().map(|m| m.id.as_str()).collect();
    let rows = col()
        .key("message-rows")
        .w_full()
        .px(20.0)
        .pt(12.0)
        .growth(GrowthSpec::default())
        .virtual_rows(
            s.messages.len(),
            MESSAGE_ROW_H,
            |s: &UiState<A>, i| Cow::Owned(format!("msg-{}", s.messages[i].id)),
            |s: &UiState<A>, i| message_row(s, i),
        )
        .virtual_rows_version(ids);
    col().key("messages").grow(1.0).min_h(0.0).w_full().scroll(ScrollSpec { stick_bottom: true }).child(rows)
}

fn message_row<A: Actions + 'static>(s: &UiState<A>, i: usize) -> El {
    let m = &s.messages[i];
    let head = row()
        .gap(8.0)
        .center_y()
        .child(text(m.sender.clone()).font(ts(12.0, 700.0)).color(c(INK)))
        .child(text(m.when.clone()).font(ts(12.0, 400.0)).color(c(MUTED)));
    let bubble = col()
        .max_w(560.0)
        .radius(12.0)
        .px(12.0)
        .py(8.0)
        .fill(c(if m.mine { LAVENDER } else { BUBBLE }))
        .child(text(m.text.clone()).font(ts(14.0, 400.0)).color(c(INK)).wrap());
    let body = if m.mine {
        row().w_full().justify_end().child(col().items_end().gap(4.0).child(head).child(bubble))
    } else {
        row().w_full().child(col().items_start().gap(4.0).child(head).child(bubble))
    };
    col().w_full().pb(12.0).child(body)
}

fn composer<A: Actions + 'static>(s: &UiState<A>) -> El {
    let field = text_input(&s.composer, ts(14.0, 400.0))
        .key("composer")
        .label("Message")
        .placeholder("Type a message", c(MUTED))
        .color(c(INK))
        .w_full()
        .on_edit(|s: &mut UiState<A>| &mut s.composer)
        .on_submit(|s: &mut UiState<A>, _tx: &mut Tx| send(s));
    let send_button = row()
        .key("send")
        .h(30.0)
        .px(14.0)
        .radius(15.0)
        .fill(c(PURPLE))
        .center()
        .on_press_tx(|s: &mut UiState<A>, tx: &mut Tx| {
            send(s);
            tx.focus("composer", false);
        })
        .child(text("Send").font(ts(13.0, 600.0)).color(c(WHITE)));
    col()
        .w_full()
        .px(16.0)
        .pt(8.0)
        .pb(12.0)
        .gap(6.0)
        .children(
            s.send_error
                .as_ref()
                .map(|e| text(e.clone()).key("send-error").font(ts(12.0, 400.0)).color(c(RED)).wrap()),
        )
        .child(
            col()
                .w_full()
                .padding(8.0)
                .gap(6.0)
                .radius(8.0)
                .fill(c(WHITE))
                .border(1.0, c(LINE))
                .child(auto_grow(field, 1, 8))
                .child(row().w_full().justify_end().child(send_button)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<String>>,
    }

    impl Actions for Fake {
        fn open(&self, conversation_id: &str) {
            self.calls.borrow_mut().push(format!("open {conversation_id}"));
        }
        fn send(&self, conversation_id: &str, text: String) {
            self.calls.borrow_mut().push(format!("send {conversation_id} {text}"));
        }
    }

    fn chat(id: &str, title: &str) -> ChatRow {
        ChatRow {
            id: id.into(),
            title: title.into(),
            preview: format!("{title}: hi"),
            unread: false,
            when: "14:05".into(),
        }
    }

    fn msg(id: &str, sender: &str, text: &str, mine: bool) -> MessageRow {
        MessageRow { id: id.into(), sender: sender.into(), when: "14:06".into(), text: text.into(), mine }
    }

    fn rt() -> Runtime<UiState<Fake>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Fake::default()), view::<Fake>, (1000.0, 700.0), env);
        rt.frame(0.0);
        rt
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn press(rt: &mut Runtime<UiState<Fake>>, key: Key, mods: Mods) {
        rt.key(KeyEvent { key, mods });
        rt.frame(1.0 / 60.0);
    }

    fn calls(rt: &Runtime<UiState<Fake>>) -> Vec<String> {
        rt.state.actions.calls.borrow().clone()
    }

    /// State changes made outside a handler must go through `update` so the view re-runs.
    fn apply(rt: &mut Runtime<UiState<Fake>>, update: Update) {
        rt.update(|s, _| s.apply(update));
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn chats_update_shows_rows() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("a", "Ada"), chat("b", "Bo")]));
        rt.frame(1.0 / 60.0);
        assert!(rt.find("chat-a").is_some() && rt.find("chat-b").is_some());
        assert!(rt.find("chat-c").is_none());
    }

    #[test]
    fn clicking_a_chat_selects_it_and_opens_it() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("a", "Ada"), chat("b", "Bo")]));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "chat-b");
        assert_eq!(rt.state.selected.as_deref(), Some("b"));
        assert_eq!(calls(&rt), ["open b"]);
    }

    #[test]
    fn messages_of_the_selected_chat_show_and_others_are_dropped() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("a", "Ada"), chat("b", "Bo")]));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "chat-b");
        apply(&mut rt, Update::Messages { conversation_id: "a".into(), rows: vec![msg("a1", "Ada", "x", false)] });
        assert!(rt.state.messages.is_empty());
        apply(
            &mut rt,
            Update::Messages {
                conversation_id: "b".into(),
                rows: vec![msg("b1", "Bo", "hi there", false), msg("b2", "Me", "yo", true)],
            },
        );
        assert!(rt.find("msg-b1").is_some() && rt.find("msg-b2").is_some());
        assert!(rt.find("msg-a1").is_none());
    }

    #[test]
    fn typing_then_send_calls_send_and_clears_the_composer() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("b", "Bo")]));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "chat-b");
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("  hello".into()), Mods::default());
        click(&mut rt, "send");
        assert_eq!(calls(&rt), ["open b", "send b hello"]);
        assert!(rt.state.composer.text.is_empty());
    }

    #[test]
    fn enter_sends_and_blank_text_does_not() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("b", "Bo")]));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "chat-b");
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("   ".into()), Mods::default());
        press(&mut rt, Key::Enter, Mods::default());
        assert_eq!(calls(&rt), ["open b"]);
        press(&mut rt, Key::Text("x".into()), Mods::default());
        press(&mut rt, Key::Enter, Mods::default());
        assert_eq!(calls(&rt), ["open b", "send b x"]);
        assert!(rt.state.composer.text.is_empty());
    }

    #[test]
    fn a_send_failure_shows_under_the_composer() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("b", "Bo")]));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "chat-b");
        apply(&mut rt, Update::SendFailed("offline".into()));
        rt.frame(1.0 / 60.0);
        assert!(rt.find("send-error").is_some());
    }
}
