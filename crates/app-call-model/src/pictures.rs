use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use calling::{VideoHub, VideoKey, VideoPicture};
use gpui_kit::*;
use image::{Frame, RgbaImage};

const STALE_AFTER: Duration = Duration::from_millis(2500);

struct Shown {
    image: Arc<RenderImage>,
    updated: Instant,
}

#[derive(Default)]
pub struct CallPictures {
    shown: HashMap<VideoKey, Shown>,
}

/// Images no longer shown, waiting for the GPUI side to free their textures.
// ponytail: one global queue, drained by `release_retired`; goes away when the
// render cache moves to the GPUI view (state-decoupling step 7).
static RETIRED: Mutex<Vec<Arc<RenderImage>>> = Mutex::new(Vec::new());

fn retire(image: Arc<RenderImage>) {
    RETIRED.lock().expect("retired pictures lock poisoned").push(image);
}

/// Frees the textures of pictures that were replaced or forgotten.
pub fn release_retired(cx: &mut App) {
    let retired = std::mem::take(&mut *RETIRED.lock().expect("retired pictures lock poisoned"));
    for image in retired {
        cx.drop_image(image, None);
    }
}

pub(crate) fn render_image(picture: VideoPicture) -> Option<Arc<RenderImage>> {
    let bytes = RgbaImage::from_raw(picture.width, picture.height, picture.bgra)?;
    Some(Arc::new(RenderImage::new(vec![Frame::new(bytes)])))
}

impl CallPictures {
    pub fn apply(&mut self, hub: &VideoHub) {
        let now = Instant::now();
        for (key, picture) in hub.drain() {
            let picture = Arc::try_unwrap(picture).unwrap_or_else(|shared| (*shared).clone());
            let Some(image) = render_image(picture) else {
                continue;
            };
            if let Some(previous) = self.shown.insert(
                key,
                Shown {
                    image,
                    updated: now,
                },
            ) {
                retire(previous.image);
            }
        }
    }

    pub fn live(&self, key: &VideoKey, now: Instant) -> Option<Arc<RenderImage>> {
        self.shown
            .get(key)
            .filter(|shown| *key == VideoKey::Screen || now.saturating_duration_since(shown.updated) < STALE_AFTER)
            .map(|shown| shown.image.clone())
    }

    pub fn forget(&mut self, key: &VideoKey) {
        if let Some(previous) = self.shown.remove(key) {
            retire(previous.image);
        }
    }

    pub fn clear(&mut self) {
        for (_, previous) in self.shown.drain() {
            retire(previous.image);
        }
    }
}

impl Drop for CallPictures {
    fn drop(&mut self) {
        self.clear();
    }
}
