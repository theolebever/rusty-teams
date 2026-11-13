//! GPUI textures for the BGRA call pictures that state keeps. One texture per slot;
//! a replaced or unused texture is freed on the next [`release`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use calling::{ShareKind, VideoKey, VideoPicture};
use gpui_kit::{App, RenderImage};
use image::{Frame, RgbaImage};

use app_call_model::pictures::CallPictures;

/// A slot not drawn for this long gives its texture back.
const UNUSED_AFTER: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Slot {
    Call(VideoKey),
    Preview,
    Share(ShareKind, u64),
}

struct Shown {
    picture: Arc<VideoPicture>,
    image: Arc<RenderImage>,
    used: Instant,
}

#[derive(Default)]
struct Textures {
    shown: HashMap<Slot, Shown>,
    retired: Vec<Arc<RenderImage>>,
}

thread_local! {
    static TEXTURES: RefCell<Textures> = RefCell::default();
}

/// The texture for `picture` in `slot`, converted once per new picture.
pub fn texture(slot: Slot, picture: &Arc<VideoPicture>) -> Option<Arc<RenderImage>> {
    TEXTURES.with_borrow_mut(|textures| {
        let now = Instant::now();
        if let Some(shown) = textures.shown.get_mut(&slot)
            && Arc::ptr_eq(&shown.picture, picture)
        {
            shown.used = now;
            return Some(shown.image.clone());
        }
        // ponytail: copies the BGRA bytes once per frame, since state keeps its picture;
        // hand the picture over instead if large screen shares show up in profiles.
        let bytes = RgbaImage::from_raw(picture.width, picture.height, picture.bgra.clone())?;
        let image = Arc::new(RenderImage::new(vec![Frame::new(bytes)]));
        let shown = Shown {
            picture: picture.clone(),
            image: image.clone(),
            used: now,
        };
        if let Some(previous) = textures.shown.insert(slot, shown) {
            textures.retired.push(previous.image);
        }
        Some(image)
    })
}

/// The texture of the call picture for `key`, if it is live.
pub fn call_texture(
    pictures: &CallPictures,
    key: &VideoKey,
    now: Instant,
) -> Option<Arc<RenderImage>> {
    texture(Slot::Call(key.clone()), pictures.live(key, now)?)
}

/// Frees replaced textures and those of slots no longer drawn.
pub fn release(cx: &mut App) {
    let retired = TEXTURES.with_borrow_mut(|textures| {
        let now = Instant::now();
        let mut retired = std::mem::take(&mut textures.retired);
        textures.shown.retain(|_, shown| {
            let keep = now.saturating_duration_since(shown.used) < UNUSED_AFTER;
            if !keep {
                retired.push(shown.image.clone());
            }
            keep
        });
        retired
    });
    for image in retired {
        cx.drop_image(image, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture() -> Arc<VideoPicture> {
        Arc::new(VideoPicture {
            width: 1,
            height: 1,
            bgra: vec![0; 4],
        })
    }

    #[test]
    fn a_slot_converts_once_per_picture_and_retires_the_one_it_replaces() {
        let (first, second) = (picture(), picture());
        let image = texture(Slot::Preview, &first).unwrap();
        assert!(Arc::ptr_eq(&image, &texture(Slot::Preview, &first).unwrap()));
        let replaced = texture(Slot::Preview, &second).unwrap();
        assert!(!Arc::ptr_eq(&image, &replaced));
        TEXTURES.with_borrow(|textures| {
            assert_eq!(textures.retired.len(), 1);
            assert!(Arc::ptr_eq(&textures.retired[0], &image));
        });
    }
}
