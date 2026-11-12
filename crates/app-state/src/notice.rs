use std::rc::Rc;
use std::time::Duration;

use crate::Host;
use crate::app_state::AppState;

const NOTICE_DURATION: Duration = Duration::from_secs(6);
const MAX_REASON_CHARS: usize = 140;

type NoticeCallback = Rc<dyn Fn(&mut AppState, &mut dyn Host)>;

#[derive(Clone)]
pub struct NoticeAction {
    pub label: String,
    pub run: NoticeCallback,
}

impl NoticeAction {
    pub fn new(label: &str, run: impl Fn(&mut AppState, &mut dyn Host) + 'static) -> Self {
        NoticeAction {
            label: label.to_owned(),
            run: Rc::new(run),
        }
    }
}

#[derive(Clone)]
pub struct Notice {
    pub id: u64,
    pub text: String,
    pub action: Option<NoticeAction>,
}

pub fn short_error(error: &dyn std::fmt::Display) -> String {
    error.to_string().chars().take(MAX_REASON_CHARS).collect()
}

pub fn truncated(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{}...", kept.trim_end())
}

impl AppState {
    pub fn raise_notice(&mut self, text: String, action: Option<NoticeAction>, cx: &mut dyn Host) {
        self.notice_count += 1;
        let id = self.notice_count;
        self.notice = Some(Notice { id, text, action });
        let this = cx.handle();
        cx.spawn(Box::pin(async move {
            this.sleep(NOTICE_DURATION).await;
            this.update(move |state, cx| {
                if state.notice.as_ref().is_some_and(|notice| notice.id == id) {
                    state.dismiss_notice(cx);
                }
            })
            .await;
        }));
        cx.emit(crate::app_state::AppEvent::Status);
        cx.notify();
    }

    pub fn dismiss_notice(&mut self, cx: &mut dyn Host) {
        if self.notice.take().is_some() {
            cx.emit(crate::app_state::AppEvent::Status);
            cx.notify();
        }
    }

    pub fn run_notice_action(&mut self, cx: &mut dyn Host) {
        let Some(notice) = self.notice.take() else {
            return;
        };
        cx.emit(crate::app_state::AppEvent::Status);
        cx.notify();
        if let Some(action) = notice.action {
            (action.run)(self, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{short_error, truncated};

    #[test]
    fn short_titles_pass_through() {
        assert_eq!(truncated("Planning", 40), "Planning");
    }

    #[test]
    fn long_titles_are_cut_at_the_limit() {
        let title = "x".repeat(60);
        assert_eq!(truncated(&title, 40), format!("{}...", "x".repeat(40)));
    }

    #[test]
    fn errors_are_capped() {
        assert_eq!(short_error(&"y".repeat(500)).chars().count(), 140);
    }
}
