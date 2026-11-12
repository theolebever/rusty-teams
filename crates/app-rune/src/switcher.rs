//! Quick switcher (Ctrl+G, Ctrl+Tab): type to filter chats and channels, recent ones first,
//! Up/Down to move, Enter to open.

use rune::*;
use crate::ui::{c, text, ts, HOVER, INK, LINE, MUTED};

use crate::screens::{navigate, Screen};
use crate::types::Actions;
use crate::ui::{open_chat, UiState};

const LIMIT: usize = 8;
const ROW_H: f32 = 36.0;

#[derive(Default)]
pub struct Switcher {
    pub edit: TextEdit,
    pub highlighted: usize,
}

pub fn open<A: Actions>(s: &mut UiState<A>, tx: &mut Tx) {
    s.nav.shortcuts_open = false;
    s.nav.switcher = Some(Switcher::default());
    tx.focus("switcher-input", false);
}

fn close<A: Actions>(s: &mut UiState<A>) {
    s.nav.switcher = None;
}

/// `(id, title)` of the conversations matching `query`, recent ones first, at most `LIMIT`.
pub(crate) fn candidates<A: Actions>(s: &UiState<A>, query: &str) -> Vec<(String, String)> {
    let mut all: Vec<(String, String)> = s.chats.iter().map(|chat| (chat.id.clone(), chat.title.clone())).collect();
    all.extend(s.teams.iter().flat_map(|team| &team.channels).filter(|channel| !channel.hidden).map(|channel| (channel.id.clone(), format!("# {}", channel.name))));
    let rank = |id: &str| s.nav.recent.iter().position(|recent| recent == id).unwrap_or(usize::MAX);
    all.sort_by_key(|(id, _)| rank(id)); // stable: the rest keeps list order
    let query = query.trim().to_lowercase();
    all.into_iter().filter(|(_, title)| title.to_lowercase().contains(&query)).take(LIMIT).collect()
}

fn choose<A: Actions>(s: &mut UiState<A>, tx: &mut Tx) {
    let Some(switcher) = &s.nav.switcher else { return };
    let found = candidates(s, &switcher.edit.text);
    let Some((id, _)) = found.get(switcher.highlighted.min(found.len().saturating_sub(1))) else { return };
    let channel = s.teams.iter().flat_map(|team| &team.channels).any(|channel| &channel.id == id);
    let id = id.clone();
    close(s);
    navigate(s, if channel { Screen::Teams } else { Screen::Chat });
    open_chat(s, &id, tx);
}

pub fn on_key<A: Actions>(s: &mut UiState<A>, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if s.nav.switcher.is_none() {
        return false;
    }
    let delta: isize = match ev.key {
        Key::Down => 1,
        Key::Up => -1,
        Key::Tab => if ev.mods.shift { -1 } else { 1 },
        Key::Enter => {
            choose(s, tx);
            return true;
        }
        Key::Escape => {
            close(s);
            return true;
        }
        _ => return false,
    };
    let len = candidates_len(s);
    let Some(switcher) = &mut s.nav.switcher else { return false };
    if len > 0 {
        switcher.highlighted = (switcher.highlighted as isize + delta).rem_euclid(len as isize) as usize;
    }
    true
}

fn candidates_len<A: Actions>(s: &UiState<A>) -> usize {
    s.nav.switcher.as_ref().map_or(0, |switcher| candidates(s, &switcher.edit.text).len())
}

pub fn view<A: Actions + 'static>(s: &UiState<A>, switcher: &Switcher) -> El {
    let found = candidates(s, &switcher.edit.text);
    let at = switcher.highlighted.min(found.len().saturating_sub(1));
    let mut list = col().key("switcher-list").w_full().px(8.0).pb(8.0);
    if found.is_empty() {
        list = list.child(row().w_full().h(ROW_H).px(8.0).center_y().child(text("No matching chats").font(ts(13.0, 400.0)).color(c(MUTED))));
    }
    for (i, (id, title)) in found.into_iter().enumerate() {
        let target = id.clone();
        let row = row()
            .key(format!("switch-{i}"))
            .w_full()
            .h(ROW_H)
            .px(8.0)
            .center_y()
            .radius(6.0)
            .hover(c(HOVER))
            .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| {
                let at = found_index(&target, s);
                if let Some(switcher) = &mut s.nav.switcher {
                    switcher.highlighted = at;
                }
                choose(s, tx);
            })
            .child(text(title).font(ts(13.0, 400.0)).color(c(INK)).truncate());
        list = list.child(if i == at { row.fill(c(HOVER)) } else { row });
    }
    let body = col()
        .w_full()
        .child(
            row().w_full().h(48.0).px(16.0).center_y().child(
                text_input(&switcher.edit, ts(15.0, 400.0))
                    .key("switcher-input")
                    .label("Go to a chat or channel")
                    .placeholder("Go to a chat or channel", c(MUTED))
                    .grow(1.0)
                    .on_edit(|s: &mut UiState<A>| &mut s.nav.switcher.get_or_insert_with(Default::default).edit)
                    .on_input_key(|s: &mut UiState<A>, ev: &KeyEvent, tx: &mut Tx| on_key(s, ev, tx)),
            ),
        )
        .child(el().w_full().h(1.0).fill(c(LINE)))
        .child(list);
    crate::shortcuts::card("switcher", 520.0, body, close::<A>)
}

/// Position of `id` among the current candidates, 0 when the list changed under the click.
fn found_index<A: Actions>(id: &str, s: &UiState<A>) -> usize {
    s.nav.switcher.as_ref().and_then(|switcher| candidates(s, &switcher.edit.text).iter().position(|(c, _)| c == id)).unwrap_or(0)
}
