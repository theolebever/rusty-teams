//! Keyboard shortcuts as the GPUI client defines them (`crates/app/src/views/shortcuts.rs`),
//! limited to what Rune has a screen for. `on_key` is the window's fallback key handler: it only
//! sees keys the focused control did not take, so composer formatting and field editing win.

use rune::*;
use crate::ui::{c, text, ts, HOVER, INK, LINE, MUTED, WHITE};

use crate::screens::{navigate, Screen};
use crate::types::Actions;
use crate::ui::{open_chat, UiState};

/// State of the keyboard-driven surfaces. `Default` is the closed, scrolled-to-end state.
#[derive(Default)]
pub struct NavState {
    pub meet_more: bool,
    pub settings_more: bool,
    pub switcher: Option<crate::switcher::Switcher>,
    pub find: crate::find::Find,
    pub shortcuts_open: bool,
    /// Set by Ctrl+N; the new-chat view takes the flag with `take_new_chat_request`.
    pub new_chat_requested: bool,
    /// The last message row is out of view, so "Jump to latest" shows.
    pub scrolled_up: bool,
    /// Conversation ids, most recent first.
    pub(crate) recent: Vec<String>,
}

impl NavState {
    pub fn touch(&mut self, id: &str) {
        self.recent.retain(|r| r != id);
        self.recent.insert(0, id.to_owned());
        self.recent.truncate(20);
    }

    pub fn take_new_chat_request(&mut self) -> bool {
        std::mem::take(&mut self.new_chat_requested)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    General,
    Navigation,
    Messages,
    Calls,
}

impl Group {
    const ALL: [Group; 4] = [Group::General, Group::Navigation, Group::Messages, Group::Calls];

    fn title(self) -> &'static str {
        match self {
            Group::General => "General",
            Group::Navigation => "Navigation",
            Group::Messages => "Messages and composer",
            Group::Calls => "Calls",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    ZoomIn,
    ZoomOut,
    ZoomReset,
    FocusSearch,
    FilterChats,
    Switcher,
    NewChat,
    Shortcuts,
    Screen(Screen),
    PreviousChat,
    NextChat,
    PreviousUnread,
    NextUnread,
    Compose,
    Find,
    JumpNewest,
    ToggleMute,
}

struct Shortcut {
    group: Group,
    label: &'static str,
    chord: &'static str,
    action: Action,
}

const fn sc(group: Group, label: &'static str, chord: &'static str, action: Action) -> Shortcut {
    Shortcut { group, label, chord, action }
}

// `mod` is Ctrl here and Cmd on macOS. Ctrl+4 (Saved) has no Rune screen.
const SHORTCUTS: &[Shortcut] = &[
    sc(Group::General, "Zoom in", "mod+plus", Action::ZoomIn),
    sc(Group::General, "Zoom in", "mod+equal", Action::ZoomIn),
    sc(Group::General, "Zoom out", "mod+minus", Action::ZoomOut),
    sc(Group::General, "Reset zoom", "mod+0", Action::ZoomReset),
    sc(Group::General, "Search chats and messages", "mod+k", Action::FocusSearch),
    sc(Group::General, "Search chats and messages", "mod+e", Action::FocusSearch),
    sc(Group::General, "Filter chats", "mod+shift+f", Action::FilterChats),
    sc(Group::General, "New chat", "mod+n", Action::NewChat),
    sc(Group::General, "Keyboard shortcuts", "mod+.", Action::Shortcuts),
    sc(Group::General, "Settings", "mod+comma", Action::Screen(Screen::Settings)),
    sc(Group::Navigation, "Open Activity", "mod+1", Action::Screen(Screen::Activity)),
    sc(Group::Navigation, "Show Chats", "mod+2", Action::Screen(Screen::Chat)),
    sc(Group::Navigation, "Show Teams", "mod+3", Action::Screen(Screen::Teams)),
    sc(Group::Navigation, "Open Calendar", "mod+shift+4", Action::Screen(Screen::Calendar)),
    sc(Group::Navigation, "Show Calls", "mod+5", Action::Screen(Screen::Calls)),
    sc(Group::Navigation, "Open Agents", "mod+6", Action::Screen(Screen::Agents)),
    sc(Group::Navigation, "Previous chat", "alt+up", Action::PreviousChat),
    sc(Group::Navigation, "Next chat", "alt+down", Action::NextChat),
    sc(Group::Navigation, "Previous unread chat", "ctrl+alt+up", Action::PreviousUnread),
    sc(Group::Navigation, "Next unread chat", "ctrl+alt+down", Action::NextUnread),
    sc(Group::Navigation, "Go to a specific chat or channel", "mod+g", Action::Switcher),
    sc(Group::Navigation, "Go to a specific chat or channel", "ctrl+tab", Action::Switcher),
    sc(Group::Navigation, "Go to compose box", "mod+r", Action::Compose),
    sc(Group::Messages, "Find in chat", "mod+f", Action::Find),
    sc(Group::Messages, "Jump to newest message", "mod+j", Action::JumpNewest),
    sc(Group::Calls, "Mute or unmute", "mod+shift+m", Action::ToggleMute),
];

/// "mod+shift+4" as "Ctrl+Shift+4".
fn display(chord: &str) -> String {
    chord
        .split('+')
        .map(|part| match part {
            "mod" => "Ctrl".to_owned(),
            "comma" => ",".to_owned(),
            other => {
                let mut chars = other.chars();
                chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// The window's key fallback: wire it as `AppSpec::on_key` and call it from other handlers.
pub fn on_key<A: Actions + 'static>(s: &mut UiState<A>, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if s.nav.switcher.is_some() && crate::switcher::on_key(s, ev, tx) {
        return true;
    }
    if s.nav.find.open && crate::find::on_key(s, ev, tx) {
        return true;
    }
    let Some(shortcut) = SHORTCUTS.iter().find(|shortcut| Chord::parse(shortcut.chord).is_ok_and(|chord| chord.matches_tx(ev, tx))) else {
        return false;
    };
    run(s, shortcut.action, tx)
}

fn run<A: Actions + 'static>(s: &mut UiState<A>, action: Action, tx: &mut Tx) -> bool {
    match action {
        Action::ZoomIn => zoom(s, 1),
        Action::ZoomOut => zoom(s, -1),
        Action::ZoomReset => zoom(s, 0),
        Action::FocusSearch => {
            close_popups(s);
            navigate(s, Screen::Chat);
            tx.focus("chat-search-input", false);
        }
        Action::Switcher => crate::switcher::open(s, tx),
        Action::FilterChats => {
            close_popups(s);
            navigate(s, Screen::Chat);
            crate::view_chatlist::open_filter(s, tx);
        }
        Action::NewChat => {
            close_popups(s);
            navigate(s, Screen::Chat);
            crate::new_chat::open(s);
            s.nav.new_chat_requested = true;
        }
        Action::Shortcuts => s.nav.shortcuts_open = !s.nav.shortcuts_open,
        Action::Screen(screen) => {
            close_popups(s);
            navigate(s, screen);
        }
        Action::PreviousChat => step_chat(s, -1, false, tx),
        Action::NextChat => step_chat(s, 1, false, tx),
        Action::PreviousUnread => step_chat(s, -1, true, tx),
        Action::NextUnread => step_chat(s, 1, true, tx),
        Action::Compose => {
            if s.read_only || s.selected.is_none() {
                return false;
            }
            tx.focus("composer-field", false);
        }
        Action::Find => {
            if s.selected.is_none() || !matches!(s.screens.screen, Screen::Chat | Screen::Teams) {
                return false;
            }
            crate::find::open(s, tx);
        }
        Action::JumpNewest => tx.scroll_to_bottom("messages"),
        Action::ToggleMute => {
            if !s.calls.state.as_ref().is_some_and(calling::CallState::is_active) {
                return false;
            }
            s.actions.call_command(calling::CallCommand::Mute(calling::MuteCommand::Toggle));
        }
    }
    true
}

pub(crate) fn zoom<A: Actions>(s: &mut UiState<A>, direction: i32) {
    use app_backend::appearance;
    s.appearance.zoom = match direction {
        1 => appearance::zoom_in(s.appearance.zoom),
        -1 => appearance::zoom_out(s.appearance.zoom),
        _ => appearance::DEFAULT_ZOOM,
    };
    s.actions.set_appearance(s.appearance);
    s.toast.show(format!("{}%", appearance::zoom_percent(s.appearance.zoom)));
}

fn close_popups<A: Actions>(s: &mut UiState<A>) {
    s.nav.settings_more = false;
    s.nav.switcher = None;
    s.nav.shortcuts_open = false;
}

/// Opens the previous or next chat of the list, only unread ones when `unread`. From a selection
/// outside the list it starts at the first (or last) chat.
fn step_chat<A: Actions>(s: &mut UiState<A>, delta: isize, unread: bool, tx: &mut Tx) {
    let ids: Vec<String> = crate::view_chatlist::filtered_chats(s)
        .into_iter()
        .map(|i| &s.chats[i])
        .filter(|chat| !unread || chat.unread || Some(chat.id.as_str()) == s.selected.as_deref())
        .map(|chat| chat.id.clone())
        .collect();
    let at = ids.iter().position(|id| Some(id) == s.selected.as_ref());
    let len = ids.len() as isize;
    if len == 0 {
        return;
    }
    let next = match at {
        Some(at) => (at as isize + delta).rem_euclid(len),
        None if delta > 0 => 0,
        None => len - 1,
    };
    let id = &ids[next as usize];
    if Some(id) == s.selected.as_ref() {
        return;
    }
    navigate(s, Screen::Chat);
    open_chat(s, id, tx);
}

/// The shortcuts dialog and the switcher, drawn over everything at the root of the window.
pub fn overlays<A: Actions + 'static>(s: &UiState<A>) -> Vec<El> {
    let mut out = Vec::new();
    if s.nav.shortcuts_open {
        out.push(dialog::<A>());
    }
    if let Some(switcher) = &s.nav.switcher {
        out.push(crate::switcher::view(s, switcher));
    }
    out
}

/// A card pinned near the top of the window; clicking outside or Escape closes it.
pub(crate) fn card<A: Actions + 'static>(key: &str, width: f32, body: El, close: fn(&mut UiState<A>)) -> El {
    col()
        .key(key.to_owned())
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .items_center()
        .pt(72.0)
        .modal(true, close)
        .child(col().w(width).max_h(520.0).radius(8.0).border(1.0, c(LINE)).fill(c(WHITE)).child(body))
}

fn dialog<A: Actions + 'static>() -> El {
    let mut list = col().w_full().px(16.0).pb(12.0).gap(2.0);
    for group in Group::ALL {
        list = list.child(text(group.title()).font(ts(12.0, 700.0)).color(c(MUTED)).pt(10.0).pb(4.0));
        for shortcut in SHORTCUTS.iter().filter(|shortcut| shortcut.group == group) {
            list = list.child(
                row()
                    .w_full()
                    .h(28.0)
                    .center_y()
                    .child(text(shortcut.label).font(ts(13.0, 400.0)).color(c(INK)).grow(1.0))
                    .child(text(display(shortcut.chord)).font(ts(12.0, 600.0)).color(c(MUTED))),
            );
        }
    }
    let body = col()
        .w_full()
        .min_h(0.0)
        .child(
            row()
                .w_full()
                .h(48.0)
                .px(16.0)
                .center_y()
                .child(text("Keyboard shortcuts").font(ts(16.0, 600.0)).color(c(INK)).grow(1.0))
                .child(
                    row()
                        .key("shortcuts-close")
                        .size(28.0, 28.0)
                        .center()
                        .radius(6.0)
                        .hover(c(HOVER))
                        .on_press(|s: &mut UiState<A>| s.nav.shortcuts_open = false)
                        .child(crate::icons::sym("close", 16.0).color(c(MUTED))),
                ),
        )
        .child(col().key("shortcuts-list").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(list));
    card("shortcuts-dialog", 520.0, body, |s: &mut UiState<A>| s.nav.shortcuts_open = false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRow, MessageRow, SendTarget, Update};
    use rune::{Env, Runtime};
    use std::cell::RefCell;

    #[derive(Default)]
    pub(crate) struct Fake {
        pub calls: RefCell<Vec<String>>,
    }

    impl Actions for Fake {
        fn open(&self, id: &str) {
            self.calls.borrow_mut().push(format!("open {id}"));
        }
        fn send(&self, _: SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn load_older(&self, id: &str) {
            self.calls.borrow_mut().push(format!("older {id}"));
        }
        fn call_command(&self, command: calling::CallCommand) {
            self.calls.borrow_mut().push(format!("call {command:?}"));
        }
    }

    fn chat(id: &str, title: &str, unread: bool) -> ChatRow {
        ChatRow { id: id.into(), title: title.into(), preview: String::new(), unread, when: String::new() }
    }

    fn msg(i: usize, text: &str) -> MessageRow {
        MessageRow {
            sender_id: None,
            system: false,
            id: format!("m{i}"),
            reply_to_id: None,
            deleted: false,
            can_delete: false,
            reactions: vec![],
            sender: "Ada".into(),
            when: "14:06".into(),
            text: text.into(),
            mine: false,
            media: Default::default(),
        }
    }

    pub(crate) fn rt() -> Runtime<UiState<Fake>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Fake::default()), crate::ui::view::<Fake>, (1000.0, 700.0), env);
        rt.on_key = Some(on_key::<Fake>);
        rt.update(|s, _| {
            s.apply(Update::Chats(vec![chat("a", "Alpha", false), chat("b", "Bravo", true), chat("c", "Charlie", false)]));
            s.selected = Some("a".into());
            s.apply(Update::Messages {
                conversation_id: "a".into(),
                rows: (0..80).map(|i| msg(i, &format!("note {i} {}", if i % 20 == 5 { "needle" } else { "hay" }))).collect(),
            });
        });
        rt.frame(0.0);
        rt.frame(1.0 / 60.0);
        rt
    }

    pub(crate) fn press(rt: &mut Runtime<UiState<Fake>>, key: Key, mods: Mods) {
        rt.key(KeyEvent { key, mods });
        rt.frame(1.0 / 60.0);
        rt.frame(2.0 / 60.0);
    }

    pub(crate) fn ctrl() -> Mods {
        Mods { cmd: true, ctrl: true, ..Default::default() }
    }

    pub(crate) fn type_text(rt: &mut Runtime<UiState<Fake>>, text: &str) {
        press(rt, Key::Text(text.into()), Mods::default());
    }

    #[test]
    fn find_matches_unicode_and_ignores_deleted_and_system_messages() {
        let rows = vec![
            msg(0, "ÉCOLE"),
            MessageRow { deleted: true, ..msg(1, "école") },
            MessageRow { system: true, ..msg(2, "école") },
            msg(3, "une école"),
        ];
        assert_eq!(crate::find::matches(&rows, " École "), vec![0, 3]);
        assert!(crate::find::matches(&rows, "  ").is_empty());
    }

    #[test]
    fn every_chord_parses() {
        for shortcut in SHORTCUTS {
            assert!(Chord::parse(shortcut.chord).is_ok(), "{}", shortcut.chord);
        }
        assert_eq!(display("mod+shift+4"), "Ctrl+Shift+4");
        assert_eq!(display("mod+comma"), "Ctrl+,");
    }

    #[test]
    fn ctrl_digits_switch_screens() {
        let mut rt = rt();
        press(&mut rt, Key::Command('5'), ctrl());
        assert_eq!(rt.state.screens.screen, Screen::Calls);
        press(&mut rt, Key::Command('1'), ctrl());
        assert_eq!(rt.state.screens.screen, Screen::Activity);
        press(&mut rt, Key::Command('2'), ctrl());
        assert_eq!(rt.state.screens.screen, Screen::Chat);
    }

    #[test]
    fn ctrl_k_focuses_the_search_box() {
        let mut rt = rt();
        press(&mut rt, Key::Command('k'), ctrl());
        type_text(&mut rt, "bra");
        assert_eq!(rt.state.global_search.edit.text, "bra");
    }

    #[test]
    fn alt_arrows_walk_the_chat_list() {
        let mut rt = rt();
        let alt = Mods { alt: true, ..Default::default() };
        press(&mut rt, Key::Down, alt);
        assert_eq!(rt.state.selected.as_deref(), Some("b"));
        press(&mut rt, Key::Up, alt);
        press(&mut rt, Key::Up, alt);
        assert_eq!(rt.state.selected.as_deref(), Some("c"));
        assert!(rt.state.actions.calls.borrow().contains(&"open b".to_string()));
    }

    #[test]
    fn ctrl_n_raises_the_new_chat_flag_once() {
        let mut rt = rt();
        press(&mut rt, Key::Command('n'), ctrl());
        assert!(rt.state.chatlist.manage.new_chat_open);
        assert!(rt.find("new-chat-query").is_some());
        assert!(rt.state.nav.take_new_chat_request());
        assert!(!rt.state.nav.take_new_chat_request());
    }

    #[test]
    fn mute_shortcut_needs_a_call() {
        let mut rt = rt();
        let mods = Mods { shift: true, ..ctrl() };
        press(&mut rt, Key::Command('m'), mods);
        assert!(!rt.state.actions.calls.borrow().iter().any(|c| c.starts_with("call")));
    }

    #[test]
    fn shortcuts_dialog_opens_and_escape_closes_it() {
        let mut rt = rt();
        press(&mut rt, Key::Command('.'), ctrl());
        assert!(rt.state.nav.shortcuts_open);
        assert!(rt.find("shortcuts-dialog").is_some());
        press(&mut rt, Key::Escape, Mods::default());
        assert!(!rt.state.nav.shortcuts_open);
        assert!(rt.find("shortcuts-dialog").is_none());
    }

    #[test]
    fn switcher_filters_and_opens_the_highlighted_chat() {
        let mut rt = rt();
        press(&mut rt, Key::Command('g'), ctrl());
        assert!(rt.find("switcher-input").is_some());
        type_text(&mut rt, "ch");
        press(&mut rt, Key::Enter, Mods::default());
        assert!(rt.state.nav.switcher.is_none());
        assert_eq!(rt.state.selected.as_deref(), Some("c"));
    }

    #[test]
    fn switcher_arrows_move_the_highlight() {
        let mut rt = rt();
        press(&mut rt, Key::Command('g'), ctrl());
        press(&mut rt, Key::Down, Mods::default());
        press(&mut rt, Key::Enter, Mods::default());
        assert_eq!(rt.state.selected.as_deref(), Some("b"));
    }

    #[test]
    fn find_steps_through_matches_and_escape_closes() {
        let mut rt = rt();
        press(&mut rt, Key::Command('f'), ctrl());
        assert!(rt.state.nav.find.open);
        type_text(&mut rt, "needle");
        assert_eq!(crate::find::matches(&rt.state.messages, "needle"), vec![5, 25, 45, 65]);
        press(&mut rt, Key::Enter, Mods::default());
        assert_eq!(rt.state.nav.find.current, 3, "the first move lands on the newest match");
        press(&mut rt, Key::Up, Mods::default());
        assert_eq!(rt.state.nav.find.current, 2);
        press(&mut rt, Key::Down, Mods::default());
        assert_eq!(rt.state.nav.find.current, 3);
        click(&mut rt, "find-older");
        assert!(rt.state.actions.calls.borrow().contains(&"older a".to_string()));
        press(&mut rt, Key::Escape, Mods::default());
        assert!(!rt.state.nav.find.open);
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn jump_to_latest_shows_after_scrolling_up_and_hides_at_the_end() {
        let mut rt = rt();
        assert!(rt.find("jump-latest").is_none());
        rt.update(|_, tx| tx.scroll_to_y("messages", 0.0));
        rt.frame(3.0 / 60.0);
        rt.frame(4.0 / 60.0);
        assert!(rt.state.nav.scrolled_up);
        assert!(rt.find("jump-latest").is_some());
        click(&mut rt, "jump-latest");
        rt.frame(0.5);
        rt.frame(1.0);
        assert!(!rt.state.nav.scrolled_up);
        assert!(rt.find("jump-latest").is_none());
    }
}
