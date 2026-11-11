//! The emoji picker under the composer: a search box, category tabs and a scrolling grid. The
//! data is the GPUI app's own `emoji.tsv` (glyph, English codes, German aliases), in Unicode group order.
//! ponytail: no skin tones or recents; `app-state` has both but pulls in the GPUI kit.

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
                        s.composer.insert(glyph);
                        s.emoji_open = false;
                        tx.focus("composer", false);
                    })
                    .child(text(glyph).font(ts(20.0, 400.0))),
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
