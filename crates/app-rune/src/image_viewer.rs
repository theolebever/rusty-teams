//! Full-window view of one message image. Escape, a click outside or the close button dismisses it.

use rune::*;
use crate::ui::text;

use crate::avatar::rgb;
use crate::types::Actions;
use crate::ui::{UiState, ts};

/// A scrim with the picture centred, or nothing while no image is open.
pub fn overlay<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let (key, alt) = s.media.viewer.as_ref()?;
    let loaded = s.media.images.get(key).filter(|image| image.error().is_none())?;
    Some(col().key("image-viewer").absolute(0.0, 0.0).w_full().h_full().center().padding(32.0).gap(12.0)
        .fill(rgb(0).alpha(0.85)).label(alt.clone())
        .modal(true, |s: &mut UiState<A>| s.media.viewer = None)
        .child(col().key("image-viewer-picture").grow(1.0).w_full().min_h(0.0).pixels_fit(loaded.pixels(), SurfaceFit::Fit))
        .child(row().key("image-viewer-close").px(16.0).h(32.0).center().radius(4.0).hover(rgb(0xFFFFFF).alpha(0.2))
            .on_press(|s: &mut UiState<A>| s.media.viewer = None)
            .child(text("Close").font(ts(13.0, 600.0)).color(rgb(0xFFFFFF)))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_drawn_without_an_open_image() {
        struct Quiet;
        impl Actions for Quiet {
            fn open(&self, _: &str) {}
            fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        }
        assert!(overlay(&UiState::new(Quiet)).is_none());
    }
}
