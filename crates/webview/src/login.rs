use std::time::{Duration, Instant};

const PROMPT_INTERVAL: Duration = Duration::from_secs(600);

#[derive(Default)]
pub(crate) struct LoginPrompt {
    last_shown: Option<Instant>,
}

impl LoginPrompt {
    pub(crate) fn claim(&mut self, now: Instant, any_visible: bool) -> bool {
        if any_visible
            || self
                .last_shown
                .is_some_and(|shown| now.duration_since(shown) < PROMPT_INTERVAL)
        {
            return false;
        }
        self.last_shown = Some(now);
        true
    }

    pub(crate) fn completed(&mut self) {
        self.last_shown = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simultaneous_app_logins_share_one_prompt() {
        let now = Instant::now();
        let mut prompt = LoginPrompt::default();
        assert!(prompt.claim(now, false));
        assert!(!prompt.claim(now, false));
        assert!(!prompt.claim(now + PROMPT_INTERVAL, true));
    }

    #[test]
    fn closing_a_prompt_does_not_immediately_reopen_it() {
        let now = Instant::now();
        let mut prompt = LoginPrompt::default();
        assert!(prompt.claim(now, false));
        assert!(!prompt.claim(now + Duration::from_secs(4), false));
        assert!(prompt.claim(now + PROMPT_INTERVAL, false));
    }

    #[test]
    fn completed_login_allows_a_remaining_consent_prompt() {
        let now = Instant::now();
        let mut prompt = LoginPrompt::default();
        assert!(prompt.claim(now, false));
        prompt.completed();
        assert!(prompt.claim(now, false));
    }
}
