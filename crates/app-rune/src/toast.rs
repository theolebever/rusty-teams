//! The connection banner under the title bar and the transient notice toast, as
//! `connection_banner.rs` and `notice_toast.rs` draw them in the GPUI app. Bridge status lines
//! split in two: connection lines drive the banner, everything else is a notice.

use std::time::{Duration, Instant};

use app_backend::schemes::active_palette;
use rune::*;

use crate::avatar::rgb;
use crate::icons::sym;
use crate::types::Actions;
use crate::ui::{UiState, text, ts};

/// A bad connection shows only once it lasted this long, as in the GPUI banner.
const SHOW_AFTER: Duration = Duration::from_secs(4);
const NOTICE_FOR: Duration = Duration::from_secs(6);

#[derive(Default)]
pub struct State {
    notice: Option<(String, Instant)>,
    seen: String,
    bad_since: Option<Instant>,
    banner: bool,
}

/// The banner text for a connection status line; `None` when it is a notice or online.
fn banner_text(status: &str) -> Option<&'static str> {
    if status.starts_with("Connecting") {
        Some("Connecting...")
    } else if status.starts_with("Sign in") {
        Some("Sign-in required. Sign in to Teams in the window that opened.")
    } else if status.starts_with("No browser") || status.starts_with("Open Teams") || status.starts_with("Failed") {
        Some("Something went wrong while connecting to Teams. Reconnect to keep the conversation going.")
    } else {
        None
    }
}

fn is_connection_line(status: &str) -> bool {
    status.is_empty() || status.starts_with("Online") || banner_text(status).is_some()
}

impl State {
    pub fn show(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    /// Takes in the latest status line; each distinct one counts once. True when a notice appeared.
    pub fn observe(&mut self, status: &str) -> bool {
        if status == self.seen {
            return false;
        }
        self.seen = status.to_owned();
        if banner_text(status).is_some() {
            self.bad_since.get_or_insert_with(Instant::now);
        } else if status.starts_with("Online") {
            self.bad_since = None;
            self.banner = false;
        }
        let notice = !is_connection_line(status);
        if notice {
            self.show(status.split(" · Read-only").next().unwrap_or(status));
        }
        notice
    }

    /// Expires the notice and shows the banner once due. True when something changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if self.notice.as_ref().is_some_and(|(_, shown)| now.duration_since(*shown) >= NOTICE_FOR) {
            self.notice = None;
            changed = true;
        }
        if !self.banner && self.bad_since.is_some_and(|since| now.duration_since(since) >= SHOW_AFTER) {
            self.banner = true;
            changed = true;
        }
        changed
    }
}

/// The amber strip for a bad connection, in the flow under the title bar.
pub fn banner<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let text_line = banner_text(&s.status).filter(|_| s.toast.banner)?;
    let palette = active_palette();
    Some(
        row()
            .key("connection-banner")
            .h(32.0)
            .px(12.0)
            .mx(8.0)
            .mb(8.0)
            .gap(8.0)
            .center_y()
            .radius(8.0)
            .fill(rgb(palette.amber).alpha(0.14))
            .child(sym("status_away", 14.0).color(rgb(palette.amber)))
            .child(text(text_line).font(ts(12.0, 400.0)).color(rgb(palette.foreground)).truncate().min_w(0.0)),
    )
}

/// The notice, bottom right at the root.
pub fn layer<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let (message, _) = s.toast.notice.as_ref()?;
    let palette = active_palette();
    Some(
        row()
            .key("toast")
            .pin(None, None, Some(16.0), Some(16.0))
            .max_w(420.0)
            .px(14.0)
            .py(10.0)
            .gap(12.0)
            .center_y()
            .radius(8.0)
            .fill(rgb(palette.popup))
            .border(1.0, rgb(palette.border))
            .child(text(message.clone()).font(ts(13.0, 400.0)).color(rgb(palette.foreground)).min_w(0.0))
            .child(
                row()
                    .key("toast-close")
                    .size(20.0, 20.0)
                    .radius(4.0)
                    .center()
                    .hover(rgb(palette.popup_hover))
                    .label("Dismiss")
                    .on_press(|s: &mut UiState<A>| s.toast.notice = None)
                    .child(sym("close", 12.0).color(rgb(palette.text_muted))),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_expire_and_a_lasting_bad_connection_raises_the_banner() {
        let mut state = State::default();
        state.observe("Online");
        assert!(state.notice.is_none() && !state.banner);
        state.observe("Could not rename chat: offline");
        assert_eq!(state.notice.as_ref().map(|n| n.0.as_str()), Some("Could not rename chat: offline"));
        let start = Instant::now();
        assert!(!state.tick(start));
        assert!(state.tick(start + NOTICE_FOR));
        assert!(state.notice.is_none());
        state.observe("Failed: no network");
        assert!(!state.tick(Instant::now()) && !state.banner);
        assert!(state.tick(Instant::now() + SHOW_AFTER) && state.banner);
        state.observe("Online");
        assert!(!state.banner);
    }
}
