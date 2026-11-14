//! Full-window view of one message image. Escape, a click outside or the close button dismisses it.

use rune::*;
use crate::ui::text;

use crate::avatar::rgb;
use crate::types::Actions;
use crate::ui::{UiState, ts};
use app_base::image_view_math as math;

pub struct State { pub zoom: f32, pub rotation: u8, pub pan: math::Pair, drag_pan: math::Pair, pub stage: math::Pair }
impl Default for State {
    fn default() -> Self { Self { zoom: 1.0, rotation: 0, pan: (0.0, 0.0), drag_pan: (0.0, 0.0), stage: (800.0, 600.0) } }
}

fn dimensions<A: Actions>(s: &UiState<A>) -> math::Pair {
    s.media.viewer.as_ref().and_then(|(key, _)| s.media.viewed_image(key)).and_then(|image| image.size()).map(|[w, h]| (w as f32, h as f32)).unwrap_or((800.0, 600.0))
}

fn zoom<A: Actions>(s: &mut UiState<A>, direction: i32) {
    let size = math::rotated_size(dimensions(s), s.media.viewer_state.rotation);
    let state = &mut s.media.viewer_state;
    let fit = math::fit_scale(size, state.stage);
    let scale = math::clamp_scale(math::step_scale(fit * state.zoom, direction), fit);
    state.zoom = scale / fit;
    state.pan = math::clamp_pan(state.pan, math::display_size(size, scale), state.stage);
}

fn navigate<A: Actions>(s: &mut UiState<A>, delta: isize) {
    let images: Vec<_> = s.messages.iter().filter(|message| !message.deleted).flat_map(|message| &message.media.images).cloned().collect();
    let Some(index) = s.media.viewer.as_ref().and_then(|(key, _)| images.iter().position(|image| image.key() == key)) else { return };
    let next = math::step_index(index, images.len(), delta);
    if next == index { return; }
    let image = &images[next];
    if s.media.image(image.key()).is_none() { s.actions.load_image(image.clone()); }
    s.media.open_viewer(image.key().into(), image.alt.clone().unwrap_or_else(|| "Message image".into()));
}

pub fn png(bytes: &[u8], turns: u8) -> Result<Vec<u8>, String> {
    use ::image::ImageDecoder;
    let decoder = ::image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|error| error.to_string())?.into_decoder().map_err(|error| error.to_string())?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > 64 * 1024 * 1024 { return Err("Images above 64 megapixels cannot be exported".into()); }
    let image = ::image::DynamicImage::from_decoder(decoder).map_err(|error| error.to_string())?;
    let image = match turns % 4 { 1 => image.rotate90(), 2 => image.rotate180(), 3 => image.rotate270(), _ => image };
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, ::image::ImageFormat::Png).map_err(|error| error.to_string())?;
    Ok(bytes.into_inner())
}

fn copy<A: Actions>(s: &mut UiState<A>) {
    if let Some((key, _)) = &s.media.viewer && let Some(bytes) = s.media.image_bytes(key) {
        s.actions.export_image(bytes.to_vec(), s.media.viewer_state.rotation, None);
    }
}

fn save<A: Actions + 'static>(s: &mut UiState<A>, tx: &mut Tx) {
    let Some((key, _)) = &s.media.viewer else { return };
    let Some(bytes) = s.media.image_bytes(key).map(<[u8]>::to_vec) else { return };
    let rotation = s.media.viewer_state.rotation;
    tx.save_file(FileDialogOptions { file_name: Some("image.png".into()), ..Default::default() }, move |s: &mut UiState<A>, result, _| {
        match result { Ok(Some(path)) => s.actions.export_image(bytes, rotation, Some(path)), Ok(None) => {}, Err(error) => s.toast.show(error.to_string()) }
    });
}

/// A scrim with the picture centred, or nothing while no image is open.
pub fn overlay<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let (key, alt) = s.media.viewer.as_ref()?;
    let loaded = s.media.viewed_image(key).filter(|image| image.error().is_none());
    let state = &s.media.viewer_state;
    let original = dimensions(s);
    let rotated = math::rotated_size(original, state.rotation);
    let fit = math::fit_scale(rotated, state.stage);
    let scale = math::clamp_scale(fit * state.zoom, fit);
    let pan = math::clamp_pan(state.pan, math::display_size(rotated, scale), state.stage);
    let mut stage = col().key("image-viewer-stage").grow(1.0).w_full().min_h(0.0).clip_rounded(0.0)
        .on_layout(|s: &mut UiState<A>, [_, _, w, h]| s.media.viewer_state.stage = (w.max(1.0), h.max(1.0)))
        .on_double_press(|s: &mut UiState<A>| { if s.media.viewer_state.zoom > 1.0 { s.media.viewer_state.zoom = 1.0; s.media.viewer_state.pan = (0.0, 0.0); } else { zoom(s, 3); } })
        .on_wheel(|s: &mut UiState<A>, event, _| { zoom(s, if event.dy > 0.0 { 1 } else { -1 }); true })
        .on_drag(|s: &mut UiState<A>, drag, _| {
            let size = math::rotated_size(dimensions(s), s.media.viewer_state.rotation);
            let state = &mut s.media.viewer_state;
            match drag.phase {
                Phase::Down => state.drag_pan = state.pan,
                Phase::Move => {
                    let scale = math::fit_scale(size, state.stage) * state.zoom;
                    state.pan = math::clamp_pan((state.drag_pan.0 + drag.dx, state.drag_pan.1 + drag.dy), math::display_size(size, scale), state.stage);
                }
                Phase::Up if drag.cancelled() => state.pan = state.drag_pan,
                Phase::Up => {}
            }
        });
    stage = stage.child(if let Some(loaded) = loaded {
        col().key("image-viewer-picture").absolute((state.stage.0 - original.0 * scale) / 2.0 + pan.0, (state.stage.1 - original.1 * scale) / 2.0 + pan.1)
            .w(original.0 * scale).h(original.1 * scale).pixels_fit(loaded.pixels(), SurfaceFit::Fit).rotate(f32::from(state.rotation) * std::f32::consts::FRAC_PI_2)
    } else { text(s.media.errors.get(key).cloned().unwrap_or_else(|| "Loading image…".into())).font(ts(14.0, 400.0)).color(rgb(0xFFFFFF)) });
    let images: Vec<_> = s.messages.iter().filter(|message| !message.deleted).flat_map(|message| &message.media.images).collect();
    let index = images.iter().position(|image| image.key() == key).unwrap_or_default();
    let controls = row().gap(8.0).wrap().center_y()
        .child(crate::screens::button("Previous", "image-viewer-previous", |s: &mut UiState<A>, _| navigate(s, -1)))
        .child(text(math::counter_label(index, images.len().max(1))).color(rgb(0xFFFFFF)))
        .child(crate::screens::button("Next", "image-viewer-next", |s: &mut UiState<A>, _| navigate(s, 1)))
        .child(crate::screens::button("−", "image-viewer-zoom-out", |s: &mut UiState<A>, _| zoom(s, -1)))
        .child(text(math::percent_label(scale)).color(rgb(0xFFFFFF)))
        .child(crate::screens::button("+", "image-viewer-zoom-in", |s: &mut UiState<A>, _| zoom(s, 1)))
        .child(crate::screens::button("Fit", "image-viewer-fit", |s: &mut UiState<A>, _| { s.media.viewer_state.zoom = 1.0; s.media.viewer_state.pan = (0.0, 0.0); }))
        .child(crate::screens::button("Rotate", "image-viewer-rotate", |s: &mut UiState<A>, _| {
            s.media.viewer_state.rotation = math::next_rotation(s.media.viewer_state.rotation); s.media.viewer_state.pan = (0.0, 0.0);
        }))
        .child(crate::screens::button("Copy image", "image-viewer-copy", |s: &mut UiState<A>, _| copy(s)))
        .child(crate::screens::button("Save as", "image-viewer-save", |s: &mut UiState<A>, tx| save(s, tx)));
    Some(col().key("image-viewer").absolute(0.0, 0.0).w_full().h_full().center().padding(32.0).gap(12.0)
        .fill(rgb(0).alpha(0.85)).label(alt.clone())
        .instant_changes()
        .modal(true, |s: &mut UiState<A>| s.media.viewer = None)
        .on_key(|s: &mut UiState<A>, key, tx| {
            match &key.key {
                Key::Left => navigate(s, -1), Key::Right => navigate(s, 1),
                Key::Command('+') | Key::Command('=') => zoom(s, 1), Key::Command('-') => zoom(s, -1),
                Key::Command('c') => copy(s), Key::Command('s') => save(s, tx),
                Key::Text(text) if text.eq_ignore_ascii_case("r") => { s.media.viewer_state.rotation = math::next_rotation(s.media.viewer_state.rotation); }
                _ => return false,
            }
            true
        })
        .child(stage)
        .child(controls)
        .child(row().key("image-viewer-close").px(16.0).h(32.0).center().radius(4.0).hover(rgb(0xFFFFFF).alpha(0.2))
            .on_press(|s: &mut UiState<A>| s.media.viewer = None)
            .child(text("Close").font(ts(13.0, 600.0)).color(rgb(0xFFFFFF)))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exporting_rotation_preserves_pixels_and_rejects_invalid_images() {
        let source = ::image::DynamicImage::new_rgba8(2, 3);
        let mut bytes = std::io::Cursor::new(Vec::new());
        source.write_to(&mut bytes, ::image::ImageFormat::Png).expect("fixture PNG");
        let rotated = png(bytes.get_ref(), 1).expect("rotated PNG");
        let decoded = ::image::load_from_memory(&rotated).expect("read exported PNG");
        assert_eq!((decoded.width(), decoded.height()), (3, 2));
        assert!(png(b"invalid", 0).is_err());
    }

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
