//! The emoji picker under the composer: a search box, category tabs and a scrolling grid. The
//! data is the GPUI app's own `emoji.tsv` (glyph, English codes, German aliases), in Unicode group order.

use std::ops::Range;
use std::sync::OnceLock;

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

use crate::avatar::rgb;
use crate::types::Actions;
use crate::ui::{LINE, UiState, c, ts};

const INDEX: &str = include_str!("../../app/assets/emoji/emoji.tsv");
/// First emoji of each group, in file order; a tab spans up to the next one.
const GROUPS: [(&str, &str); 9] = [
    ("😀", "Smileys"), ("👋", "People"), ("🐵", "Animals"), ("🍇", "Food"), ("🌍", "Travel"),
    ("🎃", "Activities"), ("👓", "Objects"), ("🏧", "Symbols"), ("🏁", "Flags"),
];
const COLUMNS: usize = 9;
const CELL: f32 = 32.0;
const GRID_HEIGHT: f32 = 216.0;
const SEARCH_LIMIT: usize = 90;

struct Emoji {
    glyph: &'static str,
    words: String,
}

fn index() -> &'static [Emoji] {
    static PARSED: OnceLock<Vec<Emoji>> = OnceLock::new();
    PARSED.get_or_init(|| {
        INDEX
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let glyph = fields.next()?;
                let words = format!("{} {}", fields.next()?.replace('_', " "), fields.next().unwrap_or_default().replace('|', " "));
                Some(Emoji { glyph, words: words.to_lowercase() })
            })
            .collect()
    })
}

/// Index range of each group.
fn groups() -> Vec<(&'static str, Range<usize>)> {
    let all = index();
    let starts: Vec<usize> = GROUPS.iter().map(|(glyph, _)| all.iter().position(|e| e.glyph == *glyph).unwrap_or(0)).collect();
    (0..GROUPS.len())
        .map(|i| (GROUPS[i].0, starts[i]..starts.get(i + 1).copied().unwrap_or(all.len())))
        .collect()
}

pub fn search(query: &str) -> Vec<&'static str> {
    let query = query.trim().to_lowercase();
    index().iter().filter(|e| e.words.contains(&query)).take(SEARCH_LIMIT).map(|e| e.glyph).collect()
}

#[derive(Default)]
pub struct State {
    pub query: TextEdit,
    pub tab: usize,
    highlighted: usize,
    dismissed: Option<usize>,
}

fn active(edit: &TextEdit) -> Option<(usize, &str)> {
    if edit.anchor != edit.focus { return None; }
    let before = edit.text.get(..edit.focus)?;
    let at = before.rfind(':')?;
    let query = &before[at + 1..];
    let boundary = before[..at].chars().next_back().is_none_or(|ch| ch.is_whitespace() || "([{".contains(ch));
    (boundary && !query.is_empty() && query.len() <= 40 && query.chars().all(|ch| ch.is_alphanumeric() || "_+-".contains(ch))).then_some((at, query))
}

fn suggestions<A: Actions>(s: &UiState<A>) -> Option<(usize, Vec<&'static str>)> {
    let (at, query) = active(&s.composer)?;
    if s.compose.emoji.dismissed == Some(at) { return None; }
    let found: Vec<_> = search(&query.replace('_', " ")).into_iter().take(6).collect();
    (!found.is_empty()).then_some((at, found))
}

fn choose<A: Actions>(s: &mut UiState<A>, index: usize) {
    let Some((at, found)) = suggestions(s) else { return };
    let Some(glyph) = found.get(index) else { return };
    let glyph = app_base::emoticons::with_tone(glyph, s.settings.prefs.skin_tone);
    s.composer.set_selection(at, s.composer.focus);
    s.composer.insert(&format!("{glyph} "));
    s.compose.emoji.highlighted = 0;
}

pub fn handle_key<A: Actions>(s: &mut UiState<A>, key: &KeyEvent) -> bool {
    if matches!(key.key, Key::Text(_) | Key::Backspace) {
        s.compose.emoji.dismissed = None;
        s.compose.emoji.highlighted = 0;
        return false;
    }
    let Some((at, found)) = suggestions(s) else { return false };
    let selected = s.compose.emoji.highlighted.min(found.len() - 1);
    match key.key {
        Key::Down => s.compose.emoji.highlighted = (selected + 1) % found.len(),
        Key::Up => s.compose.emoji.highlighted = (selected + found.len() - 1) % found.len(),
        Key::Enter | Key::Tab if !key.mods.shift => choose(s, selected),
        Key::Escape => s.compose.emoji.dismissed = Some(at),
        _ => return false,
    }
    true
}

pub fn popup<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let (_, found) = suggestions(s)?;
    let palette = active_palette();
    let mut list = col().key("emoji-suggestions").w_full().padding(4.0).radius(6.0).fill(rgb(palette.surface));
    for (index, glyph) in found.into_iter().enumerate() {
        list = list.child(row().key(format!("emoji-suggestion-{index}")).w_full().h(32.0).px(8.0).center_y()
            .fill(rgb(if index == s.compose.emoji.highlighted { palette.popup_hover } else { palette.surface }))
            .hover(rgb(palette.popup_hover)).on_press_tx(move |s: &mut UiState<A>, tx| { choose(s, index); tx.focus("composer", false); })
            .child(text(app_base::emoticons::with_tone(glyph, s.settings.prefs.skin_tone)).font(ts(20.0, 400.0))));
    }
    Some(list)
}

pub fn picker<A: Actions + 'static>(s: &UiState<A>) -> El {
    let palette = active_palette();
    let hover = rgb(palette.popup_hover);
    let query = s.compose.emoji.query.text.trim();
    let groups = groups();
    let glyphs: Vec<&str> = if query.is_empty() {
        let range = groups[s.compose.emoji.tab.min(groups.len() - 1)].1.clone();
        index()[range].iter().map(|e| e.glyph).collect()
    } else {
        search(query)
    };

    let mut tabs = row().key("emoji-tabs").gap(2.0);
    for (i, (glyph, _)) in groups.iter().enumerate() {
        let selected = query.is_empty() && i == s.compose.emoji.tab;
        tabs = tabs.child(
            row()
                .key(format!("emoji-tab-{i}"))
                .size(CELL, CELL)
                .radius(6.0)
                .center()
                .fill(if selected { hover } else { rgb(palette.surface) })
                .hover(hover)
                .on_press(move |s: &mut UiState<A>| {
                    s.compose.emoji.tab = i;
                    s.compose.emoji.query.clear();
                })
                .child(text(*glyph).font(ts(16.0, 400.0))),
        );
    }

    let mut grid = col().key("emoji-grid").w_full().h(GRID_HEIGHT).scroll(ScrollSpec { stick_bottom: false });
    for chunk in glyphs.chunks(COLUMNS) {
        let mut line = row();
        for glyph in chunk {
            let glyph: &'static str = glyph;
            line = line.child(
                row()
                    .key(format!("emoji-{glyph}"))
                    .size(CELL, CELL)
                    .radius(6.0)
                    .center()
                    .hover(hover)
                    .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| {
                        let glyph = app_base::emoticons::with_tone(glyph, s.settings.prefs.skin_tone);
                        if !crate::reactions::pick(s, &glyph) { s.composer.insert(&glyph); }
                        s.emoji_open = false;
                        tx.focus("composer", false);
                    })
                    .child(text(app_base::emoticons::with_tone(glyph, s.settings.prefs.skin_tone)).font(ts(20.0, 400.0))),
            );
        }
        grid = grid.child(line);
    }
    if glyphs.is_empty() {
        grid = grid.child(text("No emoji found").font(ts(13.0, 400.0)).color(c(crate::ui::MUTED)));
    }

    let search_box = row()
        .w_full()
        .h(32.0)
        .px(8.0)
        .center_y()
        .radius(6.0)
        .border(1.0, c(LINE))
        .child(
            text_input(&s.compose.emoji.query, ts(13.0, 400.0))
                .key("emoji-search")
                .label("Search emoji")
                .placeholder("Search emoji", c(crate::ui::MUTED))
                .color(c(crate::ui::INK))
                .single_line()
                .w_full()
                .on_edit(|s: &mut UiState<A>| &mut s.compose.emoji.query),
        );

    let mut tones = row().key("emoji-tones").gap(4.0);
    for tone in 0..=app_base::emoticons::SKIN_TONE_COUNT {
        tones = tones.child(row().key(format!("emoji-tone-{tone}")).size(CELL, CELL).center().radius(6.0)
            .label(format!("Skin tone {tone}"))
            .fill(if s.settings.prefs.skin_tone == tone { hover } else { rgb(palette.surface) })
            .hover(hover).on_press(move |s: &mut UiState<A>| crate::settings::change(s, crate::settings::Change::SkinTone(tone)))
            .child(text(app_base::emoticons::with_tone("👍", tone)).font(ts(20.0, 400.0))));
    }
    col()
        .key("emoji-picker")
        .w(CELL * COLUMNS as f32 + 16.0)
        .padding(8.0)
        .gap(6.0)
        .radius(6.0)
        .border(1.0, rgb(palette.border_strong))
        .fill(rgb(palette.surface))
        .child(search_box)
        .child(tabs)
        .child(tones)
        .child(grid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_cover_the_file_in_order_and_search_reads_codes_and_german_aliases() {
        let groups = groups();
        assert!(groups.windows(2).all(|pair| pair[0].1.end == pair[1].1.start && pair[0].1.start < pair[0].1.end));
        assert_eq!(groups.last().map(|g| g.1.end), Some(index().len()));
        assert!(search("thumbsup").contains(&"👍"));
        assert!(search("daumen").contains(&"👍"));
        assert!(search("zzzzqq").is_empty());
    }
}
