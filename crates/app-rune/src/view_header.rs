//! The conversation header, the in-conversation search field and the pinned-message bar, drawn
//! as the GPUI app draws them (`crates/app/src/views/conversation.rs` `render_header`,
//! `crates/app-ui-kit/src/channel_tabs.rs`, `crates/app/src/views/pin_banner.rs`).

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

use crate::avatar::rgb;
use crate::icons::sym;
use crate::types::{Actions, PinnedRow};
use crate::ui::{c, ts, UiState, HOVER, INK, LINE, MUTED};

const HEADER_H: f32 = 60.0;
const HEADER_PAD: f32 = 20.0;
const HEADER_GAP: f32 = 12.0;
const LEAD: f32 = 36.0;
const BUTTON: f32 = 32.0;
const GLYPH: f32 = 16.0;
const CALL_H: f32 = 30.0;
const DIVIDER_H: f32 = 20.0;
const TAB_PAD: f32 = 10.0;
const TAB_BAR: f32 = 3.0;
const TITLE_MAX_W: f32 = 240.0;
const PIN_H: f32 = 40.0;
const PIN_INSET: f32 = 8.0;

/// The header of conversation `id`: avatar, title with its edit pencil, the Chat/Shared tabs,
/// Meet now, the participant count and the icon buttons; then the search field when open, and
/// the pinned-message bar when this conversation has a pinned message.
pub(crate) fn header<A: Actions + 'static>(s: &UiState<A>, id: &str, compact: bool) -> El {
    let channel = s.channel(id);
    let title = s
        .chats
        .iter()
        .find(|ch| ch.id == id)
        .map(|ch| ch.title.as_str())
        .or(channel.map(|channel| channel.name.as_str()))
        .unwrap_or(id);
    let palette = active_palette();

    let mut bar = row()
        .key("conversation-header")
        .w_full()
        .h(HEADER_H)
        .px(HEADER_PAD)
        .gap(if compact { 4.0 } else { HEADER_GAP })
        .center_y()
        .child(crate::profile::link::<A>(crate::avatar::chat(s, title, id, LEAD), "header-avatar".into(), crate::profile::direct_user(s, id), title))
        .child(
            row()
                .grow(1.0)
                .shrink()
                .max_w(TITLE_MAX_W)
                .min_w(0.0)
                .gap(4.0)
                .center_y()
                .child(text(title.to_owned()).font(ts(16.0, 700.0)).color(c(INK)).truncate().min_w(0.0))
                .children((channel.is_none() && !s.read_only).then(|| crate::controls::icon_button::<A>("rename-chat", "edit", "Rename chat", s.rename_open, |_, _| {})
                    .on_press_tx(|s: &mut UiState<A>, _| {
                        s.rename_open = !s.rename_open;
                        let title = s.chats.iter().find(|c| Some(&c.id) == s.selected.as_ref()).map(|c| c.title.as_str()).unwrap_or_default();
                        s.rename_title = TextEdit::new(title);
                    }))),
        )
        .child(tab(if channel.is_some() { "Posts" } else { "Chat" }, !s.shared_open).key("tab-chat").on_press(|s: &mut UiState<A>| s.shared_open = false))
        .child(tab("Shared", s.shared_open).key("tab-shared").on_press(|s: &mut UiState<A>| s.shared_open = true))
        .child(el().grow(1.0));
    if channel.is_none() && !s.read_only {
        let id = id.to_owned();
        let call = if compact {
            crate::controls::icon_button::<A>("meet-now", "videocam", "Meet now", false, move |s: &mut UiState<A>, _| s.actions.meet_now(&id))
        } else { meet_now::<A>(&id) };
        bar = bar.child(call);
    }
    if let Some(count) = s.member_counts.get(id) {
        bar = bar.child(
            row()
                .key("participants")
                .gap(4.0)
                .center_y()
                .px(6.0)
                .h(BUTTON)
                .radius(8.0)
                .hover(c(HOVER))
                .on_press(|s: &mut UiState<A>| crate::members::toggle(s))
                .child(sym("group", GLYPH).color(rgb(palette.text_soft)))
                .child(text(count.to_string()).font(ts(13.0, 400.0)).color(rgb(palette.text_soft))),
        );
        bar = bar.child(el().w(1.0).h(DIVIDER_H).mx(4.0).fill(c(LINE)));
    }
    bar = bar
        .child(crate::controls::icon_button::<A>("search-toggle", "search", "Search", s.search_open, |s: &mut UiState<A>, _| toggle_search(s)))
        .child(crate::controls::icon_button::<A>("side-panel", "right_panel_open", "Conversation details", s.details_open, |s: &mut UiState<A>, _| {
            s.details_open = !s.details_open;
            if s.details_open && let Some(id) = &s.selected && s.channel(id).is_some() { s.actions.load_channel_description(id); }
        }));

    let mut block = col()
        .key("header-block")
        .w_full()
        .child(bar)
        .child(el().h(1.0).w_full().fill(c(LINE)));
    if s.search_open {
        block = block.child(search_row(s));
    }
    if s.rename_open {
        block = block.child(row().w_full().padding(8.0).gap(8.0)
            .child(text_input(&s.rename_title, ts(14.0, 400.0)).key("rename-title").label("Chat name").grow(1.0)
                .on_edit(|s: &mut UiState<A>| &mut s.rename_title))
            .child(crate::screens::button("Save", "rename-save", |s: &mut UiState<A>, _| {
                if !s.rename_title.text.trim().is_empty() {
                    if let Some(id) = &s.selected { s.actions.rename_chat(id, &s.rename_title.text); }
                    s.rename_open = false;
                }
            })));
    }
    if let Some(pinned) = s.pinned.get(s.pin_index.min(s.pinned.len().saturating_sub(1))).filter(|pinned| pinned.conversation_id == id) {
        let message_id = pinned.message_id.clone();
        let mut bar = pinned_bar(pinned).on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| {
            if s.messages.iter().any(|m| m.id == message_id) {
                tx.scroll_center_in_instant("messages", format!("msg-{message_id}"));
            } else {
                s.status = "Load older messages to view this pinned message".into();
            }
        });
        if s.pinned.len() > 1 {
            bar = bar.child(crate::screens::button(&format!("{} / {} · Next", s.pin_index + 1, s.pinned.len()), "pin-next", |s: &mut UiState<A>, _| {
                if !s.pinned.is_empty() { s.pin_index = (s.pin_index + 1) % s.pinned.len(); }
            }));
        }
        block = block.child(col().w_full().padding(PIN_INSET).child(bar));
    }
    block
}

pub fn details<A: Actions + 'static>(s: &UiState<A>, id: &str) -> El {
    let mut body = col().key("conversation-details").w(320.0).h_full().padding(12.0).gap(10.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .child(text("Details").font(ts(16.0, 600.0)))
        .child(crate::screens::button("Close", "details-close", |s: &mut UiState<A>, _| s.details_open = false));
    if let Some(channel) = s.channel(id) {
        body = body.child(text(channel.name.clone()).font(ts(14.0, 600.0)).wrap());
        if let Some(team) = s.teams.iter().find(|team| team.channels.iter().any(|channel| channel.id == id)) {
            body = body.child(text(team.name.clone()).font(ts(13.0, 400.0)).wrap());
        }
        body = body.child(text(if s.chatlist.channel_details.get(id).is_some_and(|details| details.private) { "Private channel" } else { "Standard channel" }).font(ts(12.0, 400.0)));
        body = body.child(text(match s.channel_descriptions.get(id) {
            Some(Ok(description)) if description.is_empty() => "No description".into(),
            Some(Ok(description)) => description.clone(), Some(Err(error)) => error.clone(), None => "Loading description…".into(),
        }).font(ts(13.0, 400.0)).wrap());
    } else { body = body.child(crate::members::panel(s, id)); }
    body = body.child(crate::screens::button("Shared files and links", "details-shared", |s: &mut UiState<A>, _| { s.shared_open = true; s.details_open = false; }));
    for pin in &s.pinned {
        let message_id = pin.message_id.clone();
        body = body.child(crate::screens::button(&format!("Pinned · {}", pin.text), &format!("details-pin-{}", pin.message_id), move |s: &mut UiState<A>, tx| {
            s.shared_open = false;
            tx.scroll_center_in_instant("messages", format!("msg-{message_id}"));
        }));
    }
    body
}

fn toggle_search<A: Actions>(s: &mut UiState<A>) {
    s.search_open = !s.search_open;
    if !s.search_open {
        s.search.clear();
        s.search_results = None;
    }
}

/// A header tab: label centred in the header height, with a 3 px accent bar under it when active.
fn tab(label: &str, active: bool) -> El {
    let palette = active_palette();
    let bar = el().h(TAB_BAR).w_full().radius(TAB_BAR / 2.0);
    let bar = if active { bar.fill(rgb(palette.accent)) } else { bar };
    col()
        .h_full()
        .px(TAB_PAD)
        .child(
            row().grow(1.0).center_y().child(
                text(label.to_owned())
                    .font(ts(14.0, if active { 600.0 } else { 400.0 }))
                    .color(if active { c(INK) } else { c(MUTED) }),
            ),
        )
        .child(bar)
}

/// The Meet now split button: video glyph with label, and a chevron for the options.
fn meet_now<A: Actions + 'static>(id: &str) -> El {
    let palette = active_palette();
    let id = id.to_owned();
    row()
        .key("meet-now")
        .h(CALL_H)
        .radius(6.0)
        .hover(c(HOVER))
        .child(
            row()
                .key("meet-now-start")
                .h(CALL_H)
                .px(8.0)
                .gap(6.0)
                .center_y()
                .label("Meet now")
                .on_press(move |s: &mut UiState<A>| s.actions.meet_now(&id))
                .child(sym("videocam", GLYPH).color(rgb(palette.text_soft)))
                .child(text("Meet now").font(ts(14.0, 600.0)).color(c(INK))),
        )
        .child(
            row()
                .key("meet-now-options")
                .w(18.0)
                .h(CALL_H)
                .center()
                .label("Meeting options")
                .help("Meeting options")
                .on_press(|s: &mut UiState<A>| s.nav.meet_more = !s.nav.meet_more)
                .child(sym("keyboard_arrow_down", 12.0).color(rgb(palette.text_soft))),
        )
}

pub fn meet_options<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    if !s.nav.meet_more { return None; }
    let p = active_palette();
    Some(col().key("meet-options-menu").w(200.0).padding(6.0).gap(4.0).radius(8.0).fill(rgb(p.popup)).border(1.0, rgb(p.border))
        .anchored("meet-now-options", Side::Bottom, Align::End, 4.0)
        .modal(true, |s: &mut UiState<A>| s.nav.meet_more = false)
        .child(crate::controls::menu_item::<A>("meet-option-start", "Meet now", None, false, |s: &mut UiState<A>| {
            s.nav.meet_more = false;
            if let Some(id) = &s.selected { s.actions.meet_now(id); }
        }))
        .child(crate::controls::menu_item::<A>("meet-option-join-id", "Join with an ID", None, false, |s: &mut UiState<A>| {
            s.nav.meet_more = false;
            s.screens.join_id_open = true;
        })))
}

fn search_row<A: Actions + 'static>(s: &UiState<A>) -> El {
    row()
        .key("search-row")
        .w_full()
        .px(HEADER_PAD)
        .py(6.0)
        .gap(8.0)
        .center_y()
        .child(
            text_input(&s.search, ts(13.0, 400.0))
                .key("search")
                .label("Search messages")
                .placeholder("Search cached messages", c(MUTED))
                .color(c(INK))
                .grow(1.0)
                .on_edit(|s: &mut UiState<A>| &mut s.search)
                .on_submit(|s: &mut UiState<A>, _| {
                    if let Some(id) = &s.selected {
                        s.actions.search(id, s.search.text.trim());
                    }
                }),
        )
}

/// The pinned message under the header: pin glyph, "Pinned", the sender in bold and the text.
fn pinned_bar(pinned: &PinnedRow) -> El {
    let palette = active_palette();
    let mut bar = row()
        .key("pinned-bar")
        .w_full()
        .h(PIN_H)
        .pl(12.0)
        .pr(12.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .fill(c(HOVER))
        .hover(rgb(palette.surface_raised))
        .child(sym("keep", 14.0).color(rgb(palette.accent_text)))
        .child(text("Pinned").font(ts(12.0, 400.0)).color(c(MUTED)));
    if !pinned.sender.is_empty() {
        bar = bar.child(text(pinned.sender.clone()).font(ts(13.0, 600.0)).color(c(INK)));
    }
    bar.child(
        text(pinned.text.clone())
            .font(ts(13.0, 400.0))
            .color(rgb(palette.text_soft))
            .truncate()
            .grow(1.0)
            .min_w(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRow, SendTarget};
    use rune::{Env, Runtime};

    #[derive(Default)]
    struct Quiet { calls: std::cell::RefCell<Vec<String>> }

    impl Actions for Quiet {
        fn rename_chat(&self, id: &str, title: &str) { self.calls.borrow_mut().push(format!("rename {id} {title}")); }
        fn meet_now(&self, id: &str) { self.calls.borrow_mut().push(format!("call {id}")); }
        fn open(&self, _conversation_id: &str) {}
        fn send(&self, _target: SendTarget, _text: String, _files: Vec<teams_core::FileReference>) {}
    }

    fn runtime(selected: &str) -> Runtime<UiState<Quiet>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut state = UiState::new(Quiet::default());
        state.chats = vec![ChatRow {
            id: "c0".into(),
            title: "Release planning".into(),
            preview: String::new(),
            unread: false,
            when: String::new(),
        }];
        state.selected = Some(selected.into());
        let mut rt = Runtime::new(state, crate::ui::view::<Quiet>, (1280.0, 800.0), env);
        rt.frame(0.0);
        rt
    }

    /// Presses the built element at `key`, at its top-left corner inside the box.
    fn click(rt: &mut Runtime<UiState<Quiet>>, key: &str) {
        let suffix = format!("/{key}");
        let [x, y, _, _] = rt
            .elements()
            .into_iter()
            .find(|e| e.path.ends_with(&suffix))
            .unwrap_or_else(|| panic!("no built element {key}"))
            .rect;
        rt.pointer_down(x + 4.0, y + 4.0);
        rt.pointer_up(x + 4.0, y + 4.0);
        rt.frame(1.0 / 60.0);
    }

    fn pin_c0(rt: &mut Runtime<UiState<Quiet>>) {
        rt.update(|s, _| {
            s.pinned = vec![PinnedRow { message_id: "pin".into(), conversation_id: "c0".into(), sender: "Mara".into(), text: "Rollout".into() }];
        });
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn header_icon_buttons_are_named_and_meet_options_are_menu_items() {
        let mut rt = runtime("c0");
        let elements = rt.elements();
        let by_key = |key: &str| {
            elements
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap_or_else(|| panic!("no element {key}"))
        };
        for (key, label) in [("search-toggle", "Search"), ("rename-chat", "Rename chat"), ("side-panel", "Conversation details")] {
            let e = by_key(key);
            assert_eq!(e.role, "button", "{key}");
            assert_eq!(e.label.as_deref(), Some(label), "{key} has no accessible name");
            assert!(e.focusable, "{key}");
            assert!((e.rect[2] - 32.0).abs() < 0.5 && (e.rect[3] - 32.0).abs() < 0.5, "{key} is not 32x32");
            assert!((e.radius - 6.0).abs() < 0.5, "{key} radius {}", e.radius);
        }
        rt.update(|s, _| s.nav.meet_more = true);
        rt.frame(1.0 / 60.0);
        let elements = rt.elements();
        for key in ["meet-option-start", "meet-option-join-id"] {
            let e = elements.iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no element {key}"));
            assert_eq!(e.role, "menuitem", "{key}");
            assert!(e.label.is_some() && e.focusable, "{key} is not an accessible menu row");
            assert_eq!(e.fill.a, 0.0, "{key} is filled at rest");
        }
    }

    #[test]
    fn details_panel_and_multiple_pins_have_independent_controls() {
        let mut rt = runtime("c0");
        pin_c0(&mut rt);
        rt.update(|s, _| s.pinned.push(PinnedRow { message_id: "second".into(), conversation_id: "c0".into(), sender: "Ada".into(), text: "Second".into() }));
        rt.frame(0.1);
        click(&mut rt, "pin-next");
        assert_eq!(rt.state.pin_index, 1);
        click(&mut rt, "pin-next");
        assert_eq!(rt.state.pin_index, 0);
        click(&mut rt, "side-panel");
        assert!(rt.state.details_open);
        assert!(!rt.state.shared_open);
        assert!(rt.find("conversation-details").is_some());
        click(&mut rt, "details-shared");
        assert!(rt.state.shared_open);
        assert!(!rt.state.details_open);
    }

    #[test]
    fn reconnect_hides_content_until_online_even_after_a_notice() {
        let mut rt = runtime("c0");
        rt.update(|s, _| s.apply(crate::types::Update::Status("Connecting…".into())));
        rt.frame(0.1);
        assert!(rt.find("reconnecting").is_some());
        assert!(rt.find("messages").is_none());
        rt.update(|s, _| s.apply(crate::types::Update::Status("Presence unavailable".into())));
        rt.frame(0.2);
        assert!(rt.find("reconnecting").is_some());
        rt.update(|s, _| s.apply(crate::types::Update::Status("Online".into())));
        rt.frame(0.3);
        assert!(rt.find("reconnecting").is_none());
        assert!(rt.find("messages").is_some());
    }

    #[test]
    fn narrow_composer_keeps_actions_below_the_input() {
        let mut rt = runtime("c0");
        rt.viewport = (900.0, 700.0);
        rt.update(|_, _| {});
        rt.frame(0.1);
        rt.frame(0.2);
        let elements = rt.elements();
        let rect = |key: &str| elements.iter().find(|e| e.path.ends_with(&format!("/{key}"))).unwrap().rect;
        let field = rect("composer-field");
        let actions = rect("composer-actions");
        assert!(field[2] >= 200.0, "input must retain usable width: {field:?}");
        assert!(actions[1] >= field[1] + field[3], "actions {actions:?} overlap input {field:?}");
    }

    #[test]
    fn windowed_header_actions_fit_inside_the_conversation() {
        let mut rt = runtime("c0");
        for width in [900.0, 1100.0, 1440.0] {
            rt.viewport = (width, 700.0);
            rt.update(|_, _| {});
            rt.frame(0.1);
            rt.frame(0.2);
            let elements = rt.elements();
            for key in ["meet-now", "search-toggle", "side-panel"] {
                let rect = elements.iter().find(|e| e.path.ends_with(&format!("/{key}"))).unwrap().rect;
                assert!(rect[0] >= 0.0 && rect[0] + rect[2] <= width + 1.0, "{key} exceeds window {width}: {rect:?}");
            }
        }
    }

    #[test]
    fn search_button_reveals_the_field_and_pressing_it_again_hides_it() {
        let mut rt = runtime("c0");
        assert!(rt.find("search").is_none());
        click(&mut rt, "search-toggle");
        assert!(rt.find("search").is_some());
        click(&mut rt, "search-toggle");
        assert!(rt.find("search").is_none());
    }

    #[test]
    fn header_actions_use_the_selected_chat_and_shared_can_be_closed() {
        let mut rt = runtime("c0");
        click(&mut rt, "tab-shared");
        assert!(rt.find("shared").is_some());
        click(&mut rt, "tab-chat");
        assert!(rt.find("messages").is_some());
        click(&mut rt, "rename-chat");
        rt.update(|s, _| s.rename_title = TextEdit::new("New title"));
        rt.frame(1.0);
        click(&mut rt, "rename-save");
        click(&mut rt, "meet-now");
        assert_eq!(*rt.state.actions.calls.borrow(), ["rename c0 New title", "call c0"]);
    }

    #[test]
    fn messages_start_below_the_entire_pinned_header() {
        let mut rt = runtime("c0");
        pin_c0(&mut rt);
        for search_open in [false, true] {
            rt.update(move |s, _| s.search_open = search_open);
            rt.frame(1.0 / 60.0);
            let elements = rt.elements();
            let rect = |key: &str| {
                elements.iter().find(|e| e.path.ends_with(&format!("/{key}"))).unwrap().rect
            };
            let pin = rect("pinned-bar");
            let messages = rect("messages");
            assert!(messages[1] >= pin[1] + pin[3], "messages {messages:?} overlap pin {pin:?}");
        }
    }

    #[test]
    fn pinned_bar_shows_only_in_the_conversation_it_belongs_to() {
        let mut shown = runtime("c0");
        pin_c0(&mut shown);
        assert!(shown.find("pinned-bar").is_some());

        let mut other = runtime("other");
        pin_c0(&mut other);
        assert!(other.find("pinned-bar").is_none());
    }
}
