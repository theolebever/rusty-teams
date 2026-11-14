//! Composer commands matching the GPUI client's command menu.

use rune::*;
use teams_core::{ForcedAvailability, ForcedKind};
use crate::types::Actions;
use crate::ui::{UiState, text, ts};

const COMMANDS: [&str; 14] = ["available", "away", "brb", "busy", "dnd", "goto", "leavechat", "mute", "offline", "reset", "schedulemessage", "settings", "shortcuts", "unmute"];

#[derive(Default)]
pub struct State { selected: usize, dismissed: bool }

fn candidates<A: Actions>(s: &UiState<A>) -> Vec<&'static str> {
    if s.compose.slash.dismissed || s.composer.anchor != s.composer.focus || s.composer.focus != s.composer.text.len() { return Vec::new(); }
    let Some(query) = s.composer.text.strip_prefix('/') else { return Vec::new() };
    if !query.chars().all(|ch| ch.is_ascii_alphabetic()) { return Vec::new(); }
    let target = crate::ui::active_target(s);
    let chat = target.as_ref().filter(|target| !target.channel && target.conversation_id != teams_core::NOTES_CHAT_ID);
    let details = s.selected.as_ref().and_then(|id| s.details.get(id));
    COMMANDS.into_iter().filter(|name| name.starts_with(&query.to_ascii_lowercase())).filter(|name| {
        if s.read_only && !matches!(*name, "goto" | "settings" | "shortcuts") { return false; }
        match *name {
            "leavechat" => chat.is_some() && details.is_some_and(|details| details.members.len() > 2),
            "mute" => chat.is_some() && !details.is_some_and(|details| details.muted),
            "unmute" => chat.is_some() && details.is_some_and(|details| details.muted),
            "schedulemessage" => target.as_ref().is_some_and(|target| target.conversation_id != teams_core::NOTES_CHAT_ID),
            _ => true,
        }
    }).collect()
}

fn run<A: Actions>(s: &mut UiState<A>, name: &str, tx: &mut Tx) {
    if !candidates(s).contains(&name) { return; }
    s.composer.clear();
    s.compose.slash = Default::default();
    let kind = match name {
        "available" => Some(ForcedKind::Available), "away" => Some(ForcedKind::Away), "brb" => Some(ForcedKind::BeRightBack),
        "busy" => Some(ForcedKind::Busy), "dnd" => Some(ForcedKind::DoNotDisturb), "offline" => Some(ForcedKind::Offline), _ => None,
    };
    if let Some(kind) = kind {
        crate::status_menu::set_forced(s, Some(ForcedAvailability { kind, expires_at: None }));
    } else {
        match name {
            "reset" => crate::status_menu::set_forced(s, None),
            "goto" => crate::switcher::open(s, tx),
            "settings" => crate::screens::navigate(s, crate::screens::Screen::Settings),
            "shortcuts" => s.nav.shortcuts_open = true,
            "schedulemessage" => crate::schedule::open(s),
            "leavechat" => s.chatlist.manage.leave = s.selected.clone(),
            "mute" | "unmute" => {
                if let Some(id) = &s.selected {
                    let muted = name == "mute";
                    s.actions.set_chat_muted(id, muted);
                    if let Some(details) = s.details.get_mut(id) { details.muted = muted; }
                }
            }
            _ => {}
        }
    }
}

pub fn handle_key<A: Actions>(s: &mut UiState<A>, key: &KeyEvent, tx: &mut Tx) -> bool {
    if matches!(key.key, Key::Text(_) | Key::Backspace) { s.compose.slash = Default::default(); return false; }
    let found = candidates(s);
    if found.is_empty() { return false; }
    let selected = s.compose.slash.selected.min(found.len() - 1);
    match key.key {
        Key::Down => s.compose.slash.selected = (selected + 1) % found.len(),
        Key::Up => s.compose.slash.selected = (selected + found.len() - 1) % found.len(),
        Key::Enter | Key::Tab if !key.mods.shift => run(s, found[selected], tx),
        Key::Escape => s.compose.slash.dismissed = true,
        _ => return false,
    }
    true
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let found = candidates(s);
    if found.is_empty() { return None; }
    let mut list = col().key("slash-menu").w_full().max_h(240.0).scroll(ScrollSpec { stick_bottom: false });
    for (index, name) in found.into_iter().enumerate() {
        list = list.child(crate::screens::button(&format!("/{name}"), &format!("slash-{name}"), move |s: &mut UiState<A>, tx| run(s, name, tx))
            .opacity(if index == s.compose.slash.selected { 1.0 } else { 0.7 })
            .label(format!("Command {name}")));
    }
    Some(col().w_full().padding(6.0).child(text("Commands").font(ts(12.0, 600.0))).child(list))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct Fake(RefCell<Vec<Option<ForcedAvailability>>>);
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn set_availability(&self, value: Option<ForcedAvailability>) { self.0.borrow_mut().push(value); }
    }
    #[test]
    fn commands_filter_at_the_start_and_run_without_sending_a_message() {
        let mut s = UiState::new(Fake::default());
        s.composer = TextEdit::new("/bus");
        assert_eq!(candidates(&s), ["busy"]);
        assert!(handle_key(&mut s, &KeyEvent { key: Key::Enter, mods: Mods::default() }, &mut Tx::default()));
        assert!(s.composer.text.is_empty());
        assert_eq!(s.actions.0.borrow()[0].as_ref().map(|value| value.kind), Some(ForcedKind::Busy));
        s.composer = TextEdit::new("ordinary /busy");
        assert!(candidates(&s).is_empty());
        s.read_only = true;
        s.composer = TextEdit::new("/busy");
        assert!(candidates(&s).is_empty());
    }
}
