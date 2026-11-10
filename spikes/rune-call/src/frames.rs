//! Synthetic participant feeds. A producer thread makes RGBA frames (1280x720, 30 fps per feed),
//! converts them to the BGRA bytes `NativeSurface::push_bytes` takes, and pushes them. Each
//! surface keeps only the newest frame; an older one that was never drawn is dropped.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rune::NativeSurface;
use rune::native_surface::{ByteFrame, ColorInfo, Format, FrameTiming};

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;
pub const FPS: u64 = 30;
pub const FEEDS: usize = 4;

/// Per-feed counters: frames the producer pushed, and draws Rune presented (one per distinct
/// frame id). Pushed minus presented is what was superseded before it reached the screen.
#[derive(Default)]
pub struct Presented {
    count: AtomicU64,
    last: AtomicU64,
    pushed: AtomicU64,
}

impl Presented {
    /// Presented frames since the last call.
    pub fn take(&self) -> u64 {
        self.count.swap(0, Ordering::Relaxed)
    }

    /// Frames pushed since the last call.
    pub fn take_pushed(&self) -> u64 {
        self.pushed.swap(0, Ordering::Relaxed)
    }
}

/// One surface per feed. Rune calls the observer on the completion path for every draw; a
/// completed draw of a new frame id counts as a presented frame.
pub fn feeds(presented: &Arc<[Presented; FEEDS]>) -> [NativeSurface; FEEDS] {
    std::array::from_fn(|i| {
        let surface = NativeSurface::new();
        let presented = presented.clone();
        surface.set_observer(Arc::new(move |t: FrameTiming| {
            if t.completed_at.is_some() && presented[i].last.swap(t.frame_id, Ordering::Relaxed) != t.frame_id {
                presented[i].count.fetch_add(1, Ordering::Relaxed);
            }
        }));
        surface
    })
}

/// One RGBA frame: a horizontal gradient that scrolls each frame, a vertical ramp, a per-feed
/// tint, and a white bar along the bottom whose length counts frames (wraps every 300).
pub fn rgba_frame(feed: usize, n: u64) -> Vec<u8> {
    let (w, h) = (WIDTH as usize, HEIGHT as usize);
    let shift = (n as usize * 6) % w;
    let ramp: Vec<u8> = (0..w).map(|x| (((x + shift) % w) * 255 / (w - 1)) as u8).collect();
    let tint = (feed as u8).wrapping_mul(70).wrapping_add(40);
    let mut px = vec![255u8; w * h * 4];
    for y in 0..h {
        let g = (y * 255 / (h - 1)) as u8;
        for (x, p) in px[y * w * 4..(y + 1) * w * 4].chunks_exact_mut(4).enumerate() {
            p[0] = ramp[x];
            p[1] = g;
            p[2] = tint;
        }
    }
    let bar = (n as usize % 300) * w / 300;
    for y in h - 24..h {
        for x in 0..bar {
            px[(y * w + x) * 4..(y * w + x) * 4 + 3].fill(255);
        }
    }
    px
}

fn push(surface: &NativeSurface, feed: usize, n: u64) {
    let mut bgra = rgba_frame(feed, n);
    for p in bgra.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    surface
        .push_bytes(
            ByteFrame {
                width: WIDTH,
                height: HEIGHT,
                format: Format::Bgra,
                color: ColorInfo::default(),
                planes: [bgra, Vec::new()],
            },
            n,
        )
        .expect("synthetic frame matches its dimensions");
}

/// Frame 0 on every feed, so the first paint (and a headless snapshot) has pictures.
pub fn init_frames(feeds: &[NativeSurface; FEEDS]) {
    for (i, f) in feeds.iter().enumerate() {
        push(f, i, 0);
    }
}

/// Starts the 30 fps producer. Each tick pushes one frame per feed; a producer that falls more
/// than three periods behind resets its clock instead of bursting.
pub fn spawn_producer(feeds: [NativeSurface; FEEDS], presented: Arc<[Presented; FEEDS]>) {
    std::thread::spawn(move || {
        let period = Duration::from_micros(1_000_000 / FPS);
        let mut next = Instant::now();
        for n in 1u64.. {
            next += period;
            let now = Instant::now();
            if now < next {
                std::thread::sleep(next - now);
            } else if now - next > period * 3 {
                next = now;
            }
            for (i, f) in feeds.iter().enumerate() {
                push(f, i, n);
                presented[i].pushed.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}
