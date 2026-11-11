//! The composer under the open conversation, drawn as the GPUI app draws it
//! (`crates/app/src/views/composer.rs`): one rounded field with the format, emoji, attach, AI and
//! add icons and the send icon inline on its right. Attachments, a thread banner and send errors
//! stack above it.

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

use crate::avatar::rgb;
use crate::icons::sym;
use crate::media;
use crate::types::Actions;
use crate::ui::{INK, MUTED, UiState, active_target, c, select_thread, send, ts};

const ICON: f32 = 20.0;
const BUTTON: f32 = 32.0;
const INSET: f32 = 6.0;
const ACCENT: &str = "#5B5FC7";

/// Composer state beyond the text: mentions, pickers and the GIFs waiting to be sent.
#[derive(Default)]
pub struct Compose {
    pub mentions: crate::mentions::State,
    pub emoji: crate::emoji_picker::State,
    pub gif: crate::gif_picker::State,
    pub gifs: Vec<chatsvc::Gif>,
    /// Conversation and text last handed to `Actions::save_draft`.
    saved: Option<(String, String)>,
}

impl Compose {
    /// What is sent with `text`; the mentions and GIFs are used up.
    pub fn take(&mut self, text: String) -> crate::types::Composed {
        let mentions = crate::mentions::resolve(&text, &self.mentions.chosen);
        self.mentions.chosen.clear();
        self.gif.open = false;
        crate::types::Composed { text, mentions, gifs: std::mem::take(&mut self.gifs) }
    }
}

/// Stores the open conversation's unsent text when it changed. Called on every poll.
pub(crate) fn persist_draft<A: Actions>(s: &mut UiState<A>) {
    let Some(id) = s.selected.clone().filter(|_| s.thread_root_id.is_none()) else { return };
    let now = (id, s.composer.text.clone());
    if s.compose.saved.as_ref() != Some(&now) {
        s.actions.save_draft(&now.0, &now.1);
        s.compose.saved = Some(now);
    }
}

/// Ctrl+B, I, U, then the mention list's keys.
fn composer_key<A: Actions>(s: &mut UiState<A>, key: &KeyEvent) -> bool {
    for (letter, before, after) in [('b', "**", "**"), ('i', "*", "*"), ('u', "<u>", "</u>")] {
        if key.is_command(letter) {
            format_selection(&mut s.composer, before, after);
            return true;
        }
    }
    crate::mentions::handle_key(s, key)
}

fn format_selection(edit: &mut TextEdit, before: &str, after: &str) {
    let (start, end) = edit.range();
    let selected = if start == end { "text" } else { &edit.text[start..end] };
    let insertion = format!("{before}{selected}{after}");
    edit.insert(&insertion);
    edit.set_selection(start + before.len(), start + insertion.len() - after.len());
}

pub(crate) fn composer<A: Actions + 'static>(s: &UiState<A>, stacked: bool) -> El {
    let palette = active_palette();
    let target = active_target(s);
    let has_text = !s.composer.text.trim().is_empty();
    let hover = rgb(palette.popup_hover);

    let field = text_input(&s.composer, ts(14.0, 400.0))
        .key("composer")
        .label("Message")
        .placeholder("Type a message", c(MUTED))
        .color(c(INK))
        .w_full()
        .on_edit(|s: &mut UiState<A>| &mut s.composer)
        .on_input_key(|s: &mut UiState<A>, key: &KeyEvent, _tx: &mut Tx| composer_key(s, key))
        .on_submit(|s: &mut UiState<A>, _tx: &mut Tx| send(s));
    let field = row()
        .key("composer-field")
        .grow(1.0)
        .min_w(0.0)
        .min_h(BUTTON)
        .center_y()
        .child(auto_grow(field, 1, 8).grow(1.0).min_w(0.0));

    let tool = |name: &'static str, key: &'static str| {
        row()
            .key(key)
            .size(BUTTON, BUTTON)
            .radius(8.0)
            .center()
            .hover(hover)
            .child(sym(name, ICON).color(c(MUTED)))
    };
    let attach = target.as_ref().map(|target| tool("attach_file", "attach-file").on_press_tx(media::open_picker::<A>(target)));
    let toolbar = row()
        .key("composer-tools")
        .gap(INSET)
        .center_y()
        .child(tool("text_format", "composer-format").on_press(|s: &mut UiState<A>| s.format_open = !s.format_open))
        .child(tool("mood", "composer-emoji").on_press_tx(|s: &mut UiState<A>, tx: &mut Tx| {
            s.emoji_open = !s.emoji_open;
            s.compose.gif.open = false;
            if s.emoji_open { tx.focus("emoji-search", false); }
        }))
        .child(row().key("composer-gif").size(BUTTON, BUTTON).radius(8.0).center().hover(hover)
            .on_press_tx(|s: &mut UiState<A>, tx: &mut Tx| {
                crate::gif_picker::toggle(s);
                if s.compose.gif.open { tx.focus("gif-search", false); }
            })
            .child(text("GIF").font(ts(12.0, 700.0)).color(c(MUTED))))
        .children(attach)
        .child(tool("auto_fix_high", "composer-ai").on_press(|s: &mut UiState<A>| {
            s.agent.prompt = TextEdit::new(&s.composer.text);
            s.screens.screen = crate::screens::Screen::Agents;
        }))
        .children(target.as_ref().map(|target| tool("add", "composer-add").on_press_tx(media::open_picker::<A>(target))));

    let send_button = row()
        .key("send")
        .size(BUTTON, BUTTON)
        .radius(8.0)
        .center()
        .hover(hover)
        .on_press_tx(|s: &mut UiState<A>, tx: &mut Tx| {
            send(s);
            tx.focus("composer", false);
        })
        .child(sym("send", ICON).color(if has_text { c(ACCENT) } else { c(MUTED) }));

    let actions = row().key("composer-actions").gap(INSET).center_y()
        .child(toolbar).child(el().grow(1.0)).child(send_button);
    let contents = if stacked {
        col().w_full().gap(INSET).child(field).child(actions.w_full())
    } else {
        row().w_full().gap(INSET).center_y().child(field)
            .child(actions)
    };
    let bar = col()
        .key("composer-bar")
        .w_full()
        .px(INSET)
        .py(INSET)
        .gap(INSET)
        .center_y()
        .radius(6.0)
        .border(1.0, rgb(palette.border_strong))
        .fill(rgb(palette.surface))
        .child(contents);

    col()
        .w_full()
        .px(20.0)
        .pt(8.0)
        .pb(16.0)
        .gap(6.0)
        .children(s.format_open.then(|| {
            let mut tools = row().key("format-tools").gap(6.0);
            for (label, before, after) in [("Bold", "**", "**"), ("Italic", "*", "*"), ("Underline", "<u>", "</u>"), ("Strike", "~~", "~~"), ("Code", "`", "`"), ("List", "\n- ", "")] {
                tools = tools.child(crate::screens::button(label, &format!("format-{label}"), move |s: &mut UiState<A>, tx| {
                    format_selection(&mut s.composer, before, after); tx.focus("composer", false);
                }));
            }
            tools
        }))
        .children(s.emoji_open.then(|| crate::emoji_picker::picker(s)))
        .children(s.compose.gif.open.then(|| crate::gif_picker::picker(s)))
        .children(crate::gif_picker::chips(s))
        .children(target.clone().map(|target| media::attachments(s, &target)))
        .children(s.thread_root_id.as_ref().map(|_| row().key("close-reply").h(28.0).px(8.0).hover(c(crate::ui::HOVER))
            .on_press_tx(|s: &mut UiState<A>, _| select_thread(s, None))
            .child(text("Replying in thread · Close").font(ts(12.0, 600.0)).color(c(INK)))))
        .children(s.send_error.as_ref().map(|e| {
            text(e.clone())
                .key("send-error")
                .font(ts(12.0, 400.0))
                .color(c("#C4314B"))
                .wrap()
        }))
        .children(crate::mentions::list(s))
        .child(bar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRow, SendTarget, Update};
    use rune::{Env, Runtime};

    /// Records what the composer asks of the bridge.
    #[derive(Default)]
    struct Quiet {
        calls: std::cell::RefCell<Vec<String>>,
        stored_draft: Option<String>,
    }

    impl Actions for Quiet {
        fn open(&self, _conversation_id: &str) {}
        fn send(&self, _target: SendTarget, _text: String, _files: Vec<teams_core::FileReference>) {}
        fn send_composed(&self, _target: SendTarget, composed: crate::types::Composed, _files: Vec<teams_core::FileReference>) {
            let ids: Vec<String> = composed.mentions.iter().map(|m| format!("{:?}", m.target)).collect();
            self.calls.borrow_mut().push(format!("send [{}] mentions {ids:?} gifs {}", composed.text, composed.gifs.len()));
        }
        fn search_gifs(&self, query: &str) { self.calls.borrow_mut().push(format!("gifs [{query}]")); }
        fn save_draft(&self, id: &str, text: &str) { self.calls.borrow_mut().push(format!("draft {id} [{text}]")); }
        fn load_draft(&self, _id: &str) -> Option<String> { self.stored_draft.clone() }
    }

    fn rt() -> Runtime<UiState<Quiet>> {
        rt_with(Quiet::default())
    }

    fn rt_with(actions: Quiet) -> Runtime<UiState<Quiet>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(actions), crate::ui::view::<Quiet>, (1000.0, 700.0), env);
        rt.frame(0.0);
        rt.update(|s, _| {
            s.apply(Update::Chats(vec![ChatRow {
                id: "b".into(),
                title: "Bo".into(),
                preview: "hi".into(),
                unread: false,
                when: "14:05".into(),
            }]))
        });
        rt.frame(1.0 / 60.0);
        let (x, y) = rt.find("chat-b").expect("chat row");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        rt
    }

    /// Laid-out rect `[x, y, w, h]` of the built element at `key`.
    fn rect(rt: &Runtime<UiState<Quiet>>, key: &str) -> [f32; 4] {
        let suffix = format!("/{key}");
        rt.elements()
            .into_iter()
            .find(|e| e.path.ends_with(&suffix))
            .unwrap_or_else(|| panic!("no built element {key}"))
            .rect
    }

    #[test]
    fn formatting_wraps_selected_unicode_and_keeps_it_selected() {
        let mut edit = TextEdit::new("Hi été!");
        edit.set_selection(3, 8);
        format_selection(&mut edit, "**", "**");
        assert_eq!(edit.text, "Hi **été**!");
        let (start, end) = edit.range();
        assert_eq!(&edit.text[start..end], "été");
    }

    #[test]
    fn emoji_and_format_tools_insert_without_sending_or_losing_the_draft() {
        let mut rt = rt();
        let click = |rt: &mut Runtime<UiState<Quiet>>, key: &str| {
            let (x, y) = rt.find(key).expect("tool is visible");
            rt.pointer_down(x, y); rt.pointer_up(x, y); rt.frame(1.0);
        };
        click(&mut rt, "composer-format");
        click(&mut rt, "format-Bold");
        assert_eq!(rt.state.composer.text, "**text**");
        click(&mut rt, "composer-emoji");
        click(&mut rt, "emoji-tab-1");
        click(&mut rt, "emoji-👍");
        assert!(rt.state.composer.text.contains("👍"));
        let draft = rt.state.composer.text.clone();
        click(&mut rt, "composer-ai");
        assert_eq!(rt.state.agent.prompt.text, draft);
        assert_eq!(rt.state.composer.text, draft);
        assert!(!rt.state.agent.running);
    }

    #[test]
    fn attach_icon_and_send_sit_inline_right_of_the_field() {
        let mut rt = rt();
        rt.viewport = (1440.0, 900.0);
        rt.update(|_, _| {});
        rt.frame(0.1);
        rt.frame(0.2);
        let [field_x, _, field_w, _] = rect(&rt, "composer");
        let [attach_x, attach_y, attach_w, attach_h] = rect(&rt, "attach-file");
        let [send_x, send_y, send_w, send_h] = rect(&rt, "send");
        assert!(attach_x >= field_x + field_w, "attach icon at {attach_x} sits inside the field");
        assert!(send_x > attach_x, "send icon should follow the attach icon");
        assert_eq!([attach_w, attach_h], [BUTTON, BUTTON]);
        assert_eq!([send_w, send_h], [BUTTON, BUTTON]);
        assert!((attach_y - send_y).abs() < 0.5, "toolbar icons share one row");
    }

    fn click(rt: &mut Runtime<UiState<Quiet>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn press(rt: &mut Runtime<UiState<Quiet>>, key: Key) {
        rt.key(KeyEvent { key, mods: Mods::default() });
        rt.frame(1.0 / 60.0);
    }

    fn calls(rt: &Runtime<UiState<Quiet>>) -> Vec<String> {
        rt.state.actions.calls.borrow().clone()
    }

    fn member(id: &str, name: &str) -> store::MemberRecord {
        store::MemberRecord { user_id: Some(id.into()), display_name: name.into() }
    }

    #[test]
    fn typing_an_at_lists_members_and_enter_picks_one_that_is_sent_as_a_mention() {
        let mut rt = rt();
        let details = crate::types::ChatDetails { members: vec![member("u1", "Ada Lovelace"), member("u2", "Bo Peep")], ..Default::default() };
        rt.update(|s, _| s.apply(Update::Details([("b".to_owned(), details)].into())));
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("hey @b".into()));
        assert!(rt.find("mention-0").is_some(), "list opens");
        assert!(rt.find("mention-1").is_none(), "only Bo Peep matches");
        press(&mut rt, Key::Enter);
        assert_eq!(rt.state.composer.text, "hey @Bo Peep ");
        assert!(calls(&rt).iter().all(|c| !c.starts_with("send")), "Enter picked, it did not send");
        assert!(rt.find("mention-0").is_none());
        press(&mut rt, Key::Text("hi".into()));
        press(&mut rt, Key::Enter);
        let sent = calls(&rt).into_iter().find(|c| c.starts_with("send")).expect("sent");
        assert!(sent.contains("[hey @Bo Peep hi]") && sent.contains("u2"), "{sent}");
    }

    #[test]
    fn arrows_move_the_mention_highlight_and_escape_closes_the_list() {
        let mut rt = rt();
        let details = crate::types::ChatDetails { members: vec![member("u1", "Ada"), member("u2", "Alan")], ..Default::default() };
        rt.update(|s, _| s.apply(Update::Details([("b".to_owned(), details)].into())));
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("@a".into()));
        press(&mut rt, Key::Down);
        press(&mut rt, Key::Tab);
        assert_eq!(rt.state.composer.text, "@Alan ");
        press(&mut rt, Key::Text("@a".into()));
        press(&mut rt, Key::Escape);
        assert!(rt.find("mention-0").is_none());
    }

    #[test]
    fn the_emoji_picker_searches_and_inserts() {
        let mut rt = rt();
        click(&mut rt, "composer-emoji");
        assert!(rt.find("emoji-search").is_some() && rt.find("emoji-tab-8").is_some());
        rt.update(|s, _| s.compose.emoji.query = TextEdit::new("daumen"));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "emoji-👍");
        assert_eq!(rt.state.composer.text, "👍");
        assert!(!rt.state.emoji_open);
    }

    #[test]
    fn a_picked_gif_waits_as_a_chip_and_goes_out_with_the_message() {
        let mut rt = rt();
        click(&mut rt, "composer-gif");
        assert_eq!(calls(&rt), ["gifs []"]);
        let gif = chatsvc::Gif { url: "https://g.test/a.gif".into(), preview_url: String::new(), title: "wave".into(), width: 10, height: 10, preview_width: 10, preview_height: 10 };
        rt.update(|s, _| s.apply(Update::Gifs { query: "old".into(), result: Ok(vec![]) }));
        rt.update(|s, _| s.apply(Update::Gifs { query: String::new(), result: Ok(vec![gif]) }));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "gif-0");
        assert!(rt.find("gif-chip-0").is_some() && rt.find("gif-picker").is_none());
        click(&mut rt, "send");
        assert!(calls(&rt).contains(&"send [] mentions [] gifs 1".to_owned()), "{:?}", calls(&rt));
        assert!(rt.find("gif-chip-0").is_none());
    }

    #[test]
    fn ctrl_b_and_the_underline_button_wrap_the_selection() {
        let mut rt = rt();
        click(&mut rt, "composer");
        press(&mut rt, Key::Command('b'));
        assert_eq!(rt.state.composer.text, "**text**");
        click(&mut rt, "composer-format");
        click(&mut rt, "format-Underline");
        assert!(rt.state.composer.text.contains("<u>"), "{}", rt.state.composer.text);
    }

    #[test]
    fn drafts_are_saved_when_they_change_and_come_back_on_open() {
        let mut rt = rt_with(Quiet { stored_draft: Some("from last run".into()), ..Default::default() });
        assert_eq!(rt.state.composer.text, "from last run");
        rt.update(|s, _| persist_draft(s));
        rt.update(|s, _| persist_draft(s));
        rt.update(|s, _| s.composer = TextEdit::new("typed"));
        rt.update(|s, _| persist_draft(s));
        let saved: Vec<String> = calls(&rt).into_iter().filter(|c| c.starts_with("draft")).collect();
        assert_eq!(saved, ["draft b [from last run]", "draft b [typed]"]);
    }
}
