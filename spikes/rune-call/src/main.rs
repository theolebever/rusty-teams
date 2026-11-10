//! Spike: a Microsoft Teams call screen in Rune with live video tiles fed by RGBA frames.
//! Synthetic feeds only; no WebRTC, no network. See FINDINGS.md for what the spike shows.
//!
//! `cargo run --release` opens the window. `--snap meeting.png --scene meeting --scale 1` renders
//! a scene headless (the producer thread does not start for a snapshot).

mod frames;
mod icons;

use std::sync::Arc;

use rune::color::hex;
use rune::{AppSpec, Color, Cx, El, NativeSurface, SurfaceFit, TextStyle, Tx, Waker, col, el, row, text};

use frames::{FEEDS, Presented};
use icons::{Icon, icon};

const STAGE: &str = "#1F1F1F";
const CHROME: &str = "#292929";
const TILE: &str = "#292929";
const PURPLE: &str = "#5B5FC7";
const RED: &str = "#C4314B";
const GREEN: &str = "#92C353";
const WHITE: &str = "#FFFFFF";
const SECONDARY: &str = "#B3B3B3";
const BUTTON: &str = "#3B3B3B";

const TILE_W: f32 = 640.0;
const TILE_H: f32 = 360.0;
const SELF_W: f32 = 240.0;
const SELF_H: f32 = 135.0;
const SIDE_W: f32 = 264.0;
const SPEAKER_PERIOD: u64 = 3;

const TITLE: TextStyle = TextStyle::new(13.0, 600.0);
const BODY: TextStyle = TextStyle::new(13.0, 400.0);
const LABEL: TextStyle = TextStyle::new(12.0, 500.0);
const SMALL: TextStyle = TextStyle::new(11.0, 500.0);

fn hx(s: &str) -> Color {
    hex(s).expect("valid colour")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Idle,
    /// The idle screen with the incoming-call toast on top.
    Ringing,
    InCall,
}

struct Person {
    name: &'static str,
    initials: &'static str,
    color: &'static str,
    /// Index into `Meeting::feeds`; `None` shows the avatar (camera off).
    feed: Option<usize>,
    muted: bool,
}

/// Gallery order. Feed 0..=2 are the three live gallery tiles; feed 3 is the self-view.
const GALLERY: [Person; 4] = [
    Person { name: "Marie Beaudouin", initials: "MB", color: "#6264A7", feed: Some(0), muted: false },
    Person { name: "Lucas Moreau", initials: "LM", color: "#C239B3", feed: Some(1), muted: true },
    Person { name: "Chloé Durand", initials: "CD", color: "#8378DE", feed: None, muted: false },
    Person { name: "Samir Haddad", initials: "SH", color: "#038387", feed: Some(2), muted: false },
];
const SELF_FEED: usize = 3;
const FEED_NAMES: [&str; FEEDS] = ["Marie", "Lucas", "Samir", "You"];

struct Meeting {
    screen: Screen,
    /// Last poll time, seconds since start (virtual when headless).
    now: f64,
    /// When the call began, on the same clock. Negative values only come from scenes.
    started_at: Option<f64>,
    /// Set once the FPS counter has a full sample window.
    fps_ready: bool,
    /// Presented draws per second, per feed (the last full sample window).
    fps: [u64; FEEDS],
    /// Frames the producer pushed per second, per feed.
    pushed: [u64; FEEDS],
    last_sample: f64,
    mic_on: bool,
    cam_on: bool,
    feeds: [NativeSurface; FEEDS],
    presented: Arc<[Presented; FEEDS]>,
}

impl Meeting {
    fn elapsed(&self) -> u64 {
        self.started_at.map_or(0, |t| (self.now - t).max(0.0) as u64)
    }

    fn speaker(&self) -> usize {
        ((self.elapsed() / SPEAKER_PERIOD) % GALLERY.len() as u64) as usize
    }
}

fn init() -> Meeting {
    let presented: Arc<[Presented; FEEDS]> = Arc::new(std::array::from_fn(|_| Presented::default()));
    let feeds = frames::feeds(&presented);
    frames::init_frames(&feeds);
    Meeting {
        screen: Screen::Ringing,
        now: 0.0,
        started_at: None,
        fps_ready: false,
        fps: [0; FEEDS],
        pushed: [0; FEEDS],
        last_sample: 0.0,
        mic_on: true,
        cam_on: true,
        feeds,
        presented,
    }
}

fn setup(s: &mut Meeting, wake: Waker) {
    for f in &s.feeds {
        f.set_waker(wake.clone());
    }
    // A headless snapshot shows frame 0 only, so no producer runs for it.
    let headless = std::env::args().any(|a| a == "--snap" || a == "--tree" || a == "--first-pixels");
    if !headless {
        frames::spawn_producer(s.feeds.clone(), s.presented.clone());
    }
}

/// Once per frame. Returns true only when something visible changed: the timer second, the
/// active speaker, or the FPS sample (once a second).
fn poll(s: &mut Meeting, now: f64, _: &mut Tx) -> bool {
    let before = (s.elapsed(), s.speaker());
    s.now = now;
    let mut changed = before != (s.elapsed(), s.speaker());
    if now - s.last_sample >= 1.0 {
        let window = now - s.last_sample;
        s.last_sample = now;
        for (i, p) in s.presented.iter().enumerate() {
            s.fps[i] = (p.take() as f64 / window).round() as u64;
            s.pushed[i] = (p.take_pushed() as f64 / window).round() as u64;
        }
        s.fps_ready = true;
        eprintln!("t={now:.0}s presented {:?} pushed {:?}", s.fps, s.pushed);
        changed = true;
    }
    changed
}

fn view(s: &Meeting, _: &mut Cx<Meeting>) -> El {
    let root = el().key("root").w_full().h_full().fill(hx(STAGE));
    let root = match s.screen {
        Screen::InCall => root.child(in_call(s)),
        Screen::Idle | Screen::Ringing => root.child(idle()),
    };
    if s.screen == Screen::Ringing { root.child(toast()) } else { root }
}

fn idle() -> El {
    col().key("idle").w_full().h_full().center().gap(16.0)
        .child(text("No active call").font(TITLE).color(hx(WHITE)))
        .child(
            pill("call", "Call", hx(PURPLE), hx(WHITE))
                .on_press(|s: &mut Meeting| start_call(s, true)),
        )
}

/// Teams's incoming-call card, pinned top-right over the idle screen.
fn toast() -> El {
    let caller = row().gap(12.0).center_y()
        .child(avatar("MB", hx("#6264A7"), 48.0))
        .child(text("Marie Beaudouin is calling you").font(TITLE).color(hx(WHITE)));
    let actions = row().w_full().gap(8.0)
        .child(pill("decline", "Decline", hx(RED), hx(WHITE)).grow(1.0).on_press(|s: &mut Meeting| s.screen = Screen::Idle))
        .child(pill("accept-audio", "Accept audio", hx(BUTTON), hx(WHITE)).grow(1.0).on_press(|s: &mut Meeting| start_call(s, false)))
        .child(pill("accept-video", "Accept video", hx(GREEN), hx("#1F1F1F")).grow(1.0).on_press(|s: &mut Meeting| start_call(s, true)));
    col().key("toast").w(380.0).px(16.0).py(16.0).gap(14.0).radius(12.0)
        .fill(hx("#2E2E2E")).border(1.0, hx("#3D3D3D"))
        .shadow(8.0, 24.0, hx("#00000080"))
        .pin(None, Some(24.0), Some(24.0), None)
        .child(caller)
        .child(actions)
}

fn start_call(s: &mut Meeting, video: bool) {
    s.screen = Screen::InCall;
    s.started_at = Some(s.now);
    s.mic_on = true;
    s.cam_on = video;
}

fn pill(key: &'static str, label: &'static str, fill: Color, ink: Color) -> El {
    row().key(key).role("button").label(label).h(36.0).px(16.0).center().radius(18.0).fill(fill).focusable(true)
        .child(text(label).font(TITLE).color(ink))
}

fn avatar(initials: &str, fill: Color, size: f32) -> El {
    el().size(size, size).radius(size / 2.0).fill(fill).center()
        .child(text(initials).font(TextStyle::new(size * 0.36, 600.0)).color(hx(WHITE)))
}

fn in_call(s: &Meeting) -> El {
    col().key("call").w_full().h_full()
        .child(top_bar(s))
        .child(stage(s))
        .child(control_bar(s))
}

fn top_bar(s: &Meeting) -> El {
    let e = s.elapsed();
    let fps = if s.fps_ready {
        FEED_NAMES.iter().zip(s.fps).map(|(n, f)| format!("{n} {f}")).collect::<Vec<_>>().join("  ·  ")
    } else {
        "–".to_owned()
    };
    row().key("top-bar").w_full().h(48.0).px(16.0).gap(12.0).center_y().fill(hx(CHROME))
        .child(text(format!("{:02}:{:02}", e / 60, e % 60)).key("timer").font(TITLE).color(hx(WHITE)))
        .child(text("Design sync").font(BODY).color(hx(SECONDARY)))
        .child(el().grow(1.0))
        .child(text(format!("presented fps  {fps}")).key("fps").font(SMALL).color(hx(SECONDARY)))
}

fn stage(s: &Meeting) -> El {
    let gallery = col().key("gallery").gap(16.0)
        .child(row().gap(16.0).child(tile(s, 0)).child(tile(s, 1)))
        .child(row().gap(16.0).child(tile(s, 2)).child(tile(s, 3)));
    // The self-view sits in its own column, so it never covers a gallery tile.
    let side = col().key("side").w(SIDE_W).h_full().px((SIDE_W - SELF_W) / 2.0).py(16.0)
        .child(el().grow(1.0))
        .child(self_view(s));
    row().key("stage").w_full().grow(1.0).min_h(0.0)
        .child(el().grow(1.0).h_full().center().child(gallery))
        .child(side)
}

fn tile(s: &Meeting, i: usize) -> El {
    let p = &GALLERY[i];
    let t = el().key(format!("tile-{i}")).w(TILE_W).h(TILE_H).radius(8.0).fill(hx(TILE));
    let t = match p.feed {
        Some(f) => t.native_surface(&s.feeds[f], SurfaceFit::Fill),
        None => t.center().child(avatar(p.initials, hx(p.color), 160.0)),
    };
    t.child(name_label(p.name, p.muted))
        .child(speaker_ring(s.speaker() == i))
}

fn self_view(s: &Meeting) -> El {
    let v = el().key("self").w(SELF_W).h(SELF_H).radius(8.0).fill(hx(TILE));
    let v = if s.cam_on {
        v.native_surface(&s.feeds[SELF_FEED], SurfaceFit::Fill)
    } else {
        v.center().child(avatar("YO", hx(BUTTON), 64.0))
    };
    v.child(name_label("You", !s.mic_on))
}

/// The name in a dark pill at the bottom-left, with a mic-off glyph when muted.
fn name_label(name: &str, muted: bool) -> El {
    let mut l = row().key("name").pin(Some(12.0), None, None, Some(12.0)).h(26.0).px(8.0).gap(6.0).center_y()
        .radius(6.0).fill(hx("#000000B3"));
    if muted {
        l = l.child(icon(Icon::Mic, 14.0, hx(WHITE), true));
    }
    l.child(text(name).font(LABEL).color(hx(WHITE)))
}

/// The purple ring marks the active speaker. It sits above the video as a border-only layer.
fn speaker_ring(active: bool) -> El {
    el().key("ring").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).radius(8.0)
        .border(3.0, if active { hx(PURPLE) } else { Color::TRANSPARENT })
}

fn control_bar(s: &Meeting) -> El {
    row().key("controls").w_full().h(88.0).center().gap(6.0).fill(hx(CHROME))
        .child(bar_button("chat", "Chat", Icon::Chat, false))
        .child(bar_button("people", "People", Icon::People, false))
        .child(bar_button("raise", "Raise", Icon::Hand, false))
        .child(bar_button("react", "React", Icon::React, false))
        .child(bar_button("view", "View", Icon::View, false))
        .child(bar_button("more", "More", Icon::More, false))
        .child(el().w(1.0).h(40.0).fill(hx("#4A4A4A")).key("separator"))
        .child(bar_button("camera", "Camera", Icon::Camera, !s.cam_on).on_press(|s: &mut Meeting| s.cam_on = !s.cam_on))
        .child(bar_button("mic", "Mic", Icon::Mic, !s.mic_on).on_press(|s: &mut Meeting| s.mic_on = !s.mic_on))
        .child(bar_button("share", "Share", Icon::Share, false))
        .child(leave_button())
}

fn bar_button(key: &'static str, label: &'static str, kind: Icon, slashed: bool) -> El {
    col().key(key).role("button").label(label).w(64.0).h(64.0).gap(4.0).center().radius(8.0).focusable(true)
        .child(icon(kind, 24.0, hx(WHITE), slashed))
        .child(text(label).font(SMALL).color(hx(WHITE)))
}

fn leave_button() -> El {
    row().key("leave").role("button").label("Leave").h(40.0).px(18.0).gap(8.0).center().radius(20.0)
        .fill(hx(RED)).focusable(true)
        .on_press(|s: &mut Meeting| {
            s.screen = Screen::Idle;
            s.started_at = None;
        })
        .child(text("Leave").font(TITLE).color(hx(WHITE)))
        .child(icon(Icon::Chevron, 14.0, hx(WHITE), false))
}

fn main() {
    rune::run(AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        scenes: &[
            ("meeting", |s| {
                s.screen = Screen::InCall;
                s.started_at = Some(-12.0);
            }),
            ("ringing", |s| s.screen = Screen::Ringing),
            ("idle", |s| s.screen = Screen::Idle),
        ],
        ..AppSpec::new("Teams call", (1600.0, 900.0), init, view)
    });
}
