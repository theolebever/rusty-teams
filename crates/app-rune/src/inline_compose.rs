//! Clipboard and dropped images become hosted inline images at their draft positions.

use rune::*;
use crate::types::Actions;
use crate::ui::{UiState, text, ts};
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_DRAFT_BYTES: usize = 64 * 1024 * 1024;

pub fn image(bytes: Vec<u8>, name: String) -> Result<state::outgoing::InlineImage, String> {
    let format = app_base::inline_image::sniff_inline_format(&bytes).ok_or_else(|| "Unsupported inline image format".to_owned())?;
    if bytes.len() > MAX_BYTES { return Err("Inline images are limited to 32 MiB".into()); }
    let (width, height) = app_base::inline_image::image_dimensions(format, &bytes).ok_or_else(|| "Invalid image".to_owned())?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 64 * 1024 * 1024 { return Err("Image dimensions exceed 64 megapixels".into()); }
    Ok(state::outgoing::InlineImage { name, image: app_base::inline_image::ImageData::from_bytes(format.image_format(), bytes), dimensions: Some((width, height)) })
}

pub fn marker() -> String { format!("![image](rune-attachment:{})", state::outbox::new_outbox_id()) }

pub fn insert<A: Actions>(s: &mut UiState<A>, marker: String, bytes: Vec<u8>, name: String, reserve: bool) -> Result<(), String> {
    let image = image(bytes, name)?;
    s.compose.inline.retain(|(marker, _)| s.composer.text.contains(marker));
    let total: usize = s.compose.inline.iter().map(|(_, image)| image.image.bytes.len()).sum();
    if total.saturating_add(image.image.bytes.len()) > MAX_DRAFT_BYTES { return Err("A draft can contain up to 64 MiB of inline images".into()); }
    if reserve { s.composer.insert(&marker); }
    s.media.loaded(marker.clone(), image.image.bytes.to_vec());
    s.compose.inline.push((marker, image));
    Ok(())
}

pub fn paste<A: Actions>(s: &mut UiState<A>, content: &ClipboardContent) -> bool {
    if s.read_only { return false; }
    if let Some(image) = content.image() {
        if let Err(error) = insert(s, marker(), image.png().to_vec(), "Pasted image.png".into(), true) { s.toast.show(error); }
        return true;
    }
    if !content.files().is_empty() { drop_files(s, content.files()); return true; }
    false
}

pub fn drop_files<A: Actions>(s: &mut UiState<A>, paths: &[std::path::PathBuf]) {
    if s.read_only { return; }
    let Some(target) = crate::ui::active_target(s) else { return };
    for path in paths {
        let marker = marker();
        s.composer.insert(&marker);
        s.compose.inline_loading.insert(marker.clone());
        s.actions.load_inline_file(target.clone(), marker, path.clone());
    }
}

pub fn previews<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut previews = row().key("inline-previews").w_full().wrap().gap(6.0);
    for (marker, image) in s.compose.inline.iter().filter(|(marker, _)| s.composer.text.contains(marker)) {
        let remove = marker.clone();
        let mut preview = col().key(format!("inline-{marker}")).w(100.0).gap(4.0);
        if let Some(loaded) = s.media.image(marker) {
            preview = preview.child(el().w(96.0).h(72.0).pixels_fit(loaded.pixels(), SurfaceFit::Fit));
        }
        preview = preview.child(text(image.name.clone()).font(ts(11.0, 400.0)).truncate())
            .child(crate::controls::button::<A>(&format!("inline-remove-{marker}"), "Remove", crate::controls::Variant::Danger, crate::controls::Size::Sm, move |s: &mut UiState<A>, _| {
                s.composer = TextEdit::new(s.composer.text.replace(&remove, ""));
                s.compose.inline.retain(|(marker, _)| marker != &remove);
            }));
        previews = previews.child(preview);
    }
    previews
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake;
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
    }
    fn fixture() -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        ::image::DynamicImage::new_rgba8(2, 3).write_to(&mut bytes, ::image::ImageFormat::Png).expect("fixture");
        bytes.into_inner()
    }
    #[test]
    fn inline_images_follow_their_text_positions_and_deleted_images_are_not_sent() {
        let mut s = UiState::new(Fake);
        s.composer = TextEdit::new("before after");
        s.composer.set_selection(7, 7);
        let token = marker();
        insert(&mut s, token.clone(), fixture(), "first.png".into(), true).expect("insert image");
        let composed = s.compose.take(s.composer.text.clone());
        let outgoing = crate::composed::outgoing(&composed, &[]);
        assert!(outgoing.html().contains("before <img src=\"../hostedContents/1/$value\">after"), "{}", outgoing.html());
        assert_eq!(outgoing.extras().images.len(), 1);
        let deleted = crate::types::Composed { text: "text without the image".into(), ..composed };
        let outgoing = crate::composed::outgoing(&deleted, &[]);
        assert!(outgoing.images.is_empty());
        assert!(outgoing.extras().images.is_empty());
    }
    #[test]
    fn malformed_images_are_rejected_before_becoming_attachments() {
        assert!(image(b"not an image".to_vec(), "wrong.png".into()).is_err());
    }
}
