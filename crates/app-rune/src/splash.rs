//! The startup splash, shown until the first chat list arrives.
//!
//! Entrance is finite `Sequence`s; the only thing that keeps moving is opacity `Cycle`s, which
//! wake the window at `FPS` instead of every frame (an unbounded loop on Windows redraws as fast
//! as present returns).

use std::time::Duration;

use app_backend::schemes::active_palette;
use rune::*;

use crate::avatar::rgb;
use crate::icons::sym;
use crate::types::Actions;
use crate::ui::{UiState, text, ts};

const FPS: f32 = 30.0;
/// "Open app" shows after the first, the first-start hint after the second.
const STAGES: [Duration; 2] = [Duration::from_secs(4), Duration::from_secs(10)];

const LOGO_IN: &[Beat] = &[
    Beat::new(0.0).opacity(0.0).scale(0.55),
    Beat::new(0.55).scale(1.0).easing(Easing::OVERSHOOT),
];
const RISE: &[Beat] = &[
    Beat::new(0.0).opacity(0.0).scale(0.96),
    Beat::new(0.45).easing(Easing::ENTER),
];
const FADE: &[Beat] = &[
    Beat::new(0.0).opacity(0.0),
    Beat::new(0.3).easing(Easing::EASE_OUT),
];
/// The main window's reveal once the splash goes.
pub const REVEAL: &[Beat] = &[
    Beat::new(0.0).opacity(0.0).scale(0.985),
    Beat::new(0.32).easing(Easing::ENTER),
];

const DOT: &[(f32, f32)] = &[(0.0, 0.25), (0.2, 1.0), (0.45, 0.25), (1.0, 0.25)];

/// Moves the splash to its next stage; true when the view must redraw.
pub fn tick<A: Actions>(s: &mut UiState<A>, tx: &mut Tx) -> bool {
    if !s.starting {
        return false;
    }
    let elapsed = s.started_at.elapsed();
    if let Some(next) = STAGES.iter().find(|&&at| at > elapsed) {
        tx.wake_after(*next - elapsed);
    }
    let stage = STAGES.iter().filter(|&&at| elapsed >= at).count() as u8;
    std::mem::replace(&mut s.splash_stage, stage) != stage
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    let palette = active_palette();
    col().key("splash-body").grow(1.0).w_full().center()
        .child(mark(1.0))
        .child(title("splash-title", "Rusty Teams").mt(8.0))
        .child(fading_line("splash-status", &s.status).mt(10.0))
        .child(dots().mt(22.0))
        .children((s.splash_stage >= 2).then(|| text("The first start downloads your chats, it can take a minute.")
            .key("splash-hint").font(ts(12.0, 400.0)).color(rgb(palette.text_faint)).mt(18.0)
            .sequence(Sequence::new(FADE))))
        .children((s.splash_stage >= 1).then(|| crate::controls::button::<A>("startup-open", "Open app", crate::controls::Variant::Primary, crate::controls::Size::Md, |s: &mut UiState<A>, _| s.starting = false)
            .mt(18.0).sequence(Sequence::new(FADE))))
}

/// The brand mark: the accent tile popping in. `k` scales its 180 px box, which stays the
/// layout footprint. Also drawn by `toast::reconnect` and `account_picker::view`.
pub(crate) fn mark(k: f32) -> El {
    let palette = active_palette();
    let accent = rgb(palette.accent);
    el().key("brand-mark").size(180.0 * k, 180.0 * k)
        .child(col().key("tile").size(84.0 * k, 84.0 * k).absolute(48.0 * k, 48.0 * k).radius(24.0 * k).center().fill(accent)
            .motion_intent(MotionIntent::OVERSHOOT)
            .sequence(Sequence::new(LOGO_IN))
            .child(sym("chat_bubble", 42.0 * k).color(rgb(palette.on_accent))))
}

/// Three accent dots pulsing in turn.
pub(crate) fn dots() -> El {
    let accent = rgb(active_palette().accent);
    row().key("progress-dots").gap(8.0).h(8.0).sequence(Sequence::new(FADE).delay(0.5))
        .children((0..3).map(|i| el().key(format!("dot{i}")).size(8.0, 8.0).radius(4.0).fill(accent)
            .cycle(Cycle { period: 1.2, phase: -(i as f32) * 0.14, stops: DOT, still: 0.6, fps: FPS })))
}

/// A line that fades in each time its text changes, keyed by it.
pub(crate) fn fading_line(key: &str, line: &str) -> El {
    text(line.to_owned()).key(format!("{key}:{line}")).font(ts(14.0, 400.0))
        .color(rgb(active_palette().text_muted)).sequence(Sequence::new(FADE).delay(if line.is_empty() { 0.0 } else { 0.35 }))
}

/// The title under the mark, rising in.
pub(crate) fn title(key: &'static str, line: &'static str) -> El {
    text(line).key(key).font(ts(28.0, 600.0)).color(rgb(active_palette().foreground))
        .sequence(Sequence::new(RISE).delay(0.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_follow_elapsed_time() {
        let stage = |secs: u64| STAGES.iter().filter(|&&at| Duration::from_secs(secs) >= at).count();
        assert_eq!([stage(0), stage(4), stage(9), stage(10)], [0, 1, 1, 2]);
    }
}
