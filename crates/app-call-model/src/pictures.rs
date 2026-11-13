use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use calling::{VideoHub, VideoKey, VideoPicture};

const STALE_AFTER: Duration = Duration::from_millis(2500);

struct Shown {
    picture: Arc<VideoPicture>,
    updated: Instant,
}

/// The latest BGRA picture per video key. Views turn them into textures.
#[derive(Default)]
pub struct CallPictures {
    shown: HashMap<VideoKey, Shown>,
}

impl CallPictures {
    pub fn apply(&mut self, hub: &VideoHub) {
        let updated = Instant::now();
        for (key, picture) in hub.drain() {
            self.shown.insert(key, Shown { picture, updated });
        }
    }

    pub fn live(&self, key: &VideoKey, now: Instant) -> Option<&Arc<VideoPicture>> {
        self.shown
            .get(key)
            .filter(|shown| {
                *key == VideoKey::Screen || now.saturating_duration_since(shown.updated) < STALE_AFTER
            })
            .map(|shown| &shown.picture)
    }

    pub fn forget(&mut self, key: &VideoKey) {
        self.shown.remove(key);
    }

    pub fn clear(&mut self) {
        self.shown.clear();
    }
}
