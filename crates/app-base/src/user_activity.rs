use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gpui_kit::{
    DispatchPhase, IntoElement, KeyDownEvent, MouseDownEvent, MouseMoveEvent, ScrollWheelEvent,
    canvas,
};
use tokio::sync::watch;

const SAMPLE_INTERVAL: Duration = Duration::from_secs(15);

pub struct UserInput {
    origin: Instant,
    last_input_millis: AtomicU64,
}

const NO_INPUT: u64 = u64::MAX;

impl UserInput {
    pub fn new() -> Arc<Self> {
        Arc::new(UserInput {
            origin: Instant::now(),
            last_input_millis: AtomicU64::new(NO_INPUT),
        })
    }

    pub fn touch(&self) {
        let elapsed = self.origin.elapsed().as_millis() as u64;
        self.last_input_millis.store(elapsed, Ordering::Relaxed);
    }

    fn last_input(&self) -> Option<Instant> {
        match self.last_input_millis.load(Ordering::Relaxed) {
            NO_INPUT => None,
            millis => Some(self.origin + Duration::from_millis(millis)),
        }
    }
}

pub fn input_probe(input: Arc<UserInput>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let moved = input.clone();
            window.on_mouse_event(move |_: &MouseMoveEvent, phase, _, _| {
                if phase == DispatchPhase::Capture {
                    moved.touch();
                }
            });
            let pressed = input.clone();
            window.on_mouse_event(move |_: &MouseDownEvent, phase, _, _| {
                if phase == DispatchPhase::Capture {
                    pressed.touch();
                }
            });
            let scrolled = input.clone();
            window.on_mouse_event(move |_: &ScrollWheelEvent, phase, _, _| {
                if phase == DispatchPhase::Capture {
                    scrolled.touch();
                }
            });
            let typed = input.clone();
            window.on_key_event(move |_: &KeyDownEvent, phase, _, _| {
                if phase == DispatchPhase::Capture {
                    typed.touch();
                }
            });
        },
    )
}

pub use app_backend::user_activity::{IDLE_AFTER, activity_change, is_active, latest_input, system_idle};

pub fn track(input: Arc<UserInput>) -> watch::Receiver<bool> {
    let (sender, receiver) = watch::channel(false);
    crate::runtime::handle().spawn(async move {
        let mut interval = tokio::time::interval(SAMPLE_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let now = Instant::now();
            let last_input = latest_input(input.last_input(), system_idle(), now);
            let previous = *sender.borrow();
            if let Some(current) = activity_change(previous, last_input, now, IDLE_AFTER) {
                sender.send_replace(current);
            }
        }
    });
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seconds(count: u64) -> Duration {
        Duration::from_secs(count)
    }

    #[test]
    fn active_until_the_idle_threshold() {
        let last_input = Instant::now();
        assert!(is_active(
            Some(last_input),
            last_input + seconds(299),
            IDLE_AFTER
        ));
        assert!(!is_active(
            Some(last_input),
            last_input + seconds(300),
            IDLE_AFTER
        ));
        assert!(is_active(
            Some(last_input + seconds(10)),
            last_input,
            IDLE_AFTER
        ));
    }

    #[test]
    fn without_any_input_the_user_is_inactive() {
        assert!(!is_active(None, Instant::now(), IDLE_AFTER));
        assert_eq!(
            activity_change(false, None, Instant::now(), IDLE_AFTER),
            None
        );
    }

    #[test]
    fn only_a_change_of_state_is_reported() {
        let last_input = Some(Instant::now());
        let at = |offset: u64| last_input.unwrap() + seconds(offset);
        assert_eq!(activity_change(true, last_input, at(10), IDLE_AFTER), None);
        assert_eq!(
            activity_change(true, last_input, at(301), IDLE_AFTER),
            Some(false)
        );
        assert_eq!(
            activity_change(false, last_input, at(301), IDLE_AFTER),
            None
        );
        assert_eq!(
            activity_change(false, Some(at(400)), at(401), IDLE_AFTER),
            Some(true)
        );
        assert_eq!(
            activity_change(false, last_input, at(10), IDLE_AFTER),
            Some(true)
        );
    }

    #[test]
    fn system_input_counts_when_newer_than_app_input() {
        let now = Instant::now() + seconds(1000);
        let app_input = Some(now - seconds(900));
        assert_eq!(
            latest_input(app_input, Some(seconds(20)), now),
            Some(now - seconds(20))
        );
        assert_eq!(latest_input(app_input, Some(seconds(2000)), now), app_input);
        assert_eq!(latest_input(app_input, None, now), app_input);
        assert_eq!(
            latest_input(None, Some(seconds(20)), now),
            Some(now - seconds(20))
        );
        assert_eq!(latest_input(None, None, now), None);
    }

    #[test]
    fn touch_sets_and_moves_the_last_input_forward() {
        let input = UserInput::new();
        assert_eq!(input.last_input(), None);
        input.touch();
        let first = input.last_input().unwrap();
        std::thread::sleep(Duration::from_millis(5));
        input.touch();
        assert!(input.last_input().unwrap() > first);
    }
}
