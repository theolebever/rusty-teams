//! The card under a message that links to a page: site, title, description and thumbnail.

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;
use teams_core::LinkPreview;

use crate::avatar::rgb;
use crate::rich_text::remote;
use crate::types::Actions;
use crate::ui::{UiState, ts};

const THUMB: f32 = 72.0;

pub fn view<A: Actions + 'static>(s: &UiState<A>, preview: &LinkPreview, message_id: &str) -> El {
    let palette = active_palette();
    let url = preview.url.clone();
    let mut info = col().grow(1.0).min_w(0.0).gap(2.0)
        .child(text(preview.domain()).font(ts(11.0, 400.0)).color(rgb(palette.text_muted)).truncate());
    if let Some(title) = preview.title.as_deref().filter(|title| !title.trim().is_empty()) {
        info = info.child(text(title.to_owned()).font(ts(13.0, 600.0)).color(rgb(palette.accent)).wrap().max_lines(2));
    }
    if let Some(description) = preview.description.as_deref().filter(|text| !text.trim().is_empty()) {
        info = info.child(text(description.to_owned()).font(ts(12.0, 400.0)).color(rgb(palette.text_soft)).wrap().max_lines(3));
    }
    let mut card = row().key(format!("link-preview-{message_id}")).w_full().max_w(420.0).gap(10.0).padding(8.0).radius(6.0)
        .border(1.0, rgb(palette.border)).fill(rgb(palette.surface_raised)).hover(rgb(palette.row_hover))
        .label(preview.title.clone().unwrap_or_else(|| preview.domain()))
        .on_press_tx(move |s: &mut UiState<A>, tx| crate::rich_text::open(s, tx, &url))
        .child(info);
    if let Some(image) = preview.image() {
        if let Some(loaded) = s.media.image(image.key()).filter(|loaded| loaded.error().is_none()) {
            card = card.child(col().w(THUMB).h(THUMB).radius(4.0).pixels_fit(loaded.pixels(), SurfaceFit::Fill));
        }
    }
    card
}

/// An image that may still be loading; nothing is drawn until it is.
pub fn thumbnail<A: Actions + 'static>(s: &UiState<A>, url: &str, w: f32, h: f32) -> Option<El> {
    let image = s.media.image(remote(url).key()).filter(|image| image.error().is_none())?;
    Some(col().w(w).h(h).pixels_fit(image.pixels(), SurfaceFit::Fit))
}
