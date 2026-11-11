//! Find in conversation (Ctrl+F) over the loaded messages, and the "Jump to latest" bar.
//! The header's search box queries the whole cached history on the backend instead and filters
//! the list; this bar only moves between matches already loaded and can load older ones.

use rune::*;
use crate::ui::{c, text, ts, HOVER, INK, LINE, MUTED};

use crate::icons::sym;
use crate::types::{Actions, MessageRow};
use crate::ui::UiState;

#[derive(Default)]
pub struct Find {
    pub open: bool,
    pub edit: TextEdit,
    /// Index into `matches` of the match the list was last scrolled to.
    pub current: usize,
    /// The query `current` belongs to; a different text starts again from the newest match.
    searched: String,
}

/// Indices of the messages whose text contains `query`, case-insensitively, oldest first.
pub fn matches(messages: &[MessageRow], query: &str) -> Vec<usize> {
    matching_indices(messages, query).collect()
}

fn matching_indices<'a>(messages: &'a [MessageRow], query: &str) -> impl Iterator<Item = usize> + 'a {
    let query = query.trim().to_lowercase();
    messages
        .iter()
        .enumerate()
        .filter(move |(_, message)| !query.is_empty() && !message.deleted && !message.system && message.text.to_lowercase().contains(&query))
        .map(|(i, _)| i)
}

pub fn open<A: Actions>(s: &mut UiState<A>, tx: &mut Tx) {
    s.nav.find.open = true;
    tx.focus("find-input", false);
}

pub fn close<A: Actions>(s: &mut UiState<A>) {
    s.nav.find.open = false;
}

/// Moves to the next older (`delta` -1) or newer (+1) match; the first move after typing lands
/// on the newest.
pub fn step<A: Actions>(s: &mut UiState<A>, delta: isize, tx: &mut Tx) {
    let found = matches(&s.messages, &s.nav.find.edit.text);
    let Some(last) = found.len().checked_sub(1) else { return };
    let find = &mut s.nav.find;
    find.current = if find.searched == find.edit.text {
        (find.current.min(last) as isize + delta).rem_euclid(found.len() as isize) as usize
    } else {
        last
    };
    find.searched = find.edit.text.clone();
    tx.scroll_center_in_instant("messages", format!("msg-{}", s.messages[found[find.current]].id));
}

pub fn on_key<A: Actions>(s: &mut UiState<A>, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        Key::Escape => close(s),
        Key::Up => step(s, -1, tx),
        Key::Down => step(s, 1, tx),
        _ => return false,
    }
    true
}

/// Id of the match the list is on, while the bar is open.
pub fn marked<A: Actions>(s: &UiState<A>) -> Option<&str> {
    let find = &s.nav.find;
    if !find.open || find.searched != find.edit.text {
        return None;
    }
    matching_indices(&s.messages, &find.edit.text)
        .nth(find.current)
        .map(|i| s.messages[i].id.as_str())
}

/// Tints the row of the match the list is on.
pub fn mark<A: Actions>(s: &UiState<A>, id: &str, row: El) -> El {
    if marked(s) == Some(id) { row.fill(c("#FFF4CE")) } else { row }
}

pub fn bar<A: Actions + 'static>(s: &UiState<A>) -> El {
    let find = &s.nav.find;
    if !find.open {
        return el();
    }
    let found = matching_indices(&s.messages, &find.edit.text).count();
    let status = if find.edit.text.trim().is_empty() {
        String::new()
    } else if found == 0 {
        "No matches".to_owned()
    } else if find.searched == find.edit.text {
        format!("{} of {found}", find.current.min(found - 1) + 1)
    } else {
        format!("{found} matches")
    };
    let id = s.selected.clone().unwrap_or_default();
    row()
        .key("find-bar")
        .w_full()
        .h(44.0)
        .px(16.0)
        .gap(8.0)
        .center_y()
        .border(1.0, c(LINE))
        .child(sym("search", 14.0).color(c(MUTED)))
        .child(
            text_input(&find.edit, ts(13.0, 400.0))
                .key("find-input")
                .label("Find in chat")
                .placeholder("Find in chat", c(MUTED))
                .grow(1.0)
                .on_edit(|s: &mut UiState<A>| &mut s.nav.find.edit)
                .on_input_key(|s: &mut UiState<A>, ev: &KeyEvent, tx: &mut Tx| {
                    // The field would move its caret on Up and Down, and take Enter as a submit.
                    if ev.key == Key::Enter { step(s, -1, tx); return true; }
                    on_key(s, ev, tx)
                }),
        )
        .child(text(status).font(ts(12.0, 400.0)).color(c(MUTED)))
        .child(button("find-prev", "Older", |s: &mut UiState<A>, tx| step(s, -1, tx)))
        .child(button("find-next", "Newer", |s: &mut UiState<A>, tx| step(s, 1, tx)))
        .child(
            row()
                .key("find-older")
                .h(28.0)
                .px(10.0)
                .center()
                .radius(6.0)
                .hover(c(HOVER))
                .on_press(move |s: &mut UiState<A>| s.actions.load_older(&id))
                .child(text("Load older messages").font(ts(12.0, 600.0)).color(c(INK))),
        )
        .child(button("find-close", "Close", |s: &mut UiState<A>, _| close(s)))
}

fn button<A: Actions + 'static>(key: &str, label: &str, action: fn(&mut UiState<A>, &mut Tx)) -> El {
    row()
        .key(key.to_owned())
        .h(28.0)
        .px(10.0)
        .center()
        .radius(6.0)
        .hover(c(HOVER))
        .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| action(s, tx))
        .child(text(label.to_owned()).font(ts(12.0, 600.0)).color(c(INK)))
}

/// Whether "Jump to latest" shows: the open conversation's last row is out of view.
pub fn jump_visible<A: Actions>(s: &UiState<A>) -> bool {
    s.nav.scrolled_up
}

/// Records which rows the list shows. True when the jump bar appears or goes away.
pub fn rows_visible<A: Actions>(s: &mut UiState<A>, rows: std::ops::Range<usize>) -> bool {
    let up = !s.messages.is_empty() && s.search_results.is_none() && rows.end < s.messages.len();
    std::mem::replace(&mut s.nav.scrolled_up, up) != up
}

pub fn jump_bar<A: Actions + 'static>(s: &UiState<A>) -> El {
    if !jump_visible(s) {
        return el();
    }
    row().key("jump-bar").w_full().h(36.0).center().child(
        row()
            .key("jump-latest")
            .h(28.0)
            .px(12.0)
            .gap(6.0)
            .center()
            .radius(14.0)
            .fill(c(HOVER))
            .on_press_tx(|_: &mut UiState<A>, tx: &mut Tx| tx.scroll_to_bottom("messages"))
            .child(sym("keyboard_arrow_down", 16.0).color(c(INK)))
            .child(text("Jump to latest").font(ts(12.0, 600.0)).color(c(INK))),
    )
}
