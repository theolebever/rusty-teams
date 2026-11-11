//! The GIF picker under the composer (GPUI `fun_picker.rs`): a search box over the Teams GIF
//! service, trending when empty. A pick waits in the composer as a chip and is sent with the message.
//! ponytail: results are titles, not thumbnails (the image decoder here has no GIF support).

use app_backend::schemes::active_palette;
use chatsvc::Gif;
use rune::*;
use crate::ui::text;

use crate::avatar::rgb;
use crate::types::Actions;
use crate::ui::{INK, LINE, MUTED, UiState, c, ts};

#[derive(Default)]
pub enum Results {
    #[default]
    Idle,
    Loading,
    Ready(Vec<Gif>),
    Failed(String),
}

#[derive(Default)]
pub struct State {
    pub open: bool,
    pub query: TextEdit,
    pub results: Results,
    /// The query the pending request was made for; older answers are dropped.
    asked: String,
}

impl State {
    pub fn loaded(&mut self, query: String, result: Result<Vec<Gif>, String>) {
        if query == self.asked {
            self.results = match result {
                Ok(found) => Results::Ready(found),
                Err(error) => Results::Failed(error),
            };
        }
    }
}

pub fn search<A: Actions>(s: &mut UiState<A>) {
    let query = s.compose.gif.query.text.trim().to_owned();
    s.compose.gif.asked = query.clone();
    s.compose.gif.results = Results::Loading;
    s.actions.search_gifs(&query);
}

pub fn toggle<A: Actions>(s: &mut UiState<A>) {
    s.compose.gif.open = !s.compose.gif.open;
    if s.compose.gif.open {
        s.emoji_open = false;
        search(s);
    }
}

pub fn picker<A: Actions + 'static>(s: &UiState<A>) -> El {
    let palette = active_palette();
    let hover = rgb(palette.popup_hover);
    let note = |message: String| text(message).font(ts(13.0, 400.0)).color(c(MUTED));
    let mut list = col().key("gif-list").w_full().h(216.0).scroll(ScrollSpec { stick_bottom: false });
    match &s.compose.gif.results {
        Results::Idle | Results::Loading => list = list.child(note("Loading GIFs…".into())),
        Results::Failed(error) => list = list.child(note(format!("GIFs unavailable: {error}"))),
        Results::Ready(found) if found.is_empty() => list = list.child(note("No GIFs found".into())),
        Results::Ready(found) => {
            for (index, gif) in found.iter().enumerate() {
                let gif = gif.clone();
                let title = if gif.title.is_empty() { "GIF".to_owned() } else { gif.title.clone() };
                list = list.child(
                    row()
                        .key(format!("gif-{index}"))
                        .w_full()
                        .h(30.0)
                        .px(8.0)
                        .center_y()
                        .radius(4.0)
                        .hover(hover)
                        .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| {
                            s.compose.gifs.push(gif.clone());
                            s.compose.gif.open = false;
                            tx.focus("composer", false);
                        })
                        .child(text(title).font(ts(13.0, 400.0)).color(c(INK))),
                );
            }
        }
    }
    let search_box = row().w_full().h(32.0).px(8.0).center_y().radius(6.0).border(1.0, c(LINE)).child(
        text_input(&s.compose.gif.query, ts(13.0, 400.0))
            .key("gif-search")
            .label("Search GIFs")
            .placeholder("Search GIFs", c(MUTED))
            .color(c(INK))
            .single_line()
            .w_full()
            .on_edit(|s: &mut UiState<A>| &mut s.compose.gif.query)
            .on_submit(|s: &mut UiState<A>, _tx: &mut Tx| search(s)),
    );
    col()
        .key("gif-picker")
        .w_full()
        .padding(8.0)
        .gap(6.0)
        .radius(6.0)
        .border(1.0, rgb(palette.border_strong))
        .fill(rgb(palette.surface))
        .child(search_box)
        .child(list)
}

/// Chosen GIFs waiting to be sent, each removable.
pub fn chips<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    if s.compose.gifs.is_empty() {
        return None;
    }
    let mut chips = row().key("gif-chips").gap(6.0);
    for (index, gif) in s.compose.gifs.iter().enumerate() {
        chips = chips.child(
            row()
                .key(format!("gif-chip-{index}"))
                .h(28.0)
                .px(8.0)
                .center_y()
                .radius(14.0)
                .fill(rgb(active_palette().popup_hover))
                .on_press(move |s: &mut UiState<A>| {
                    if index < s.compose.gifs.len() {
                        s.compose.gifs.remove(index);
                    }
                })
                .child(text(format!("GIF: {}  ×", gif.title)).font(ts(12.0, 600.0)).color(c(INK))),
        );
    }
    Some(chips)
}
