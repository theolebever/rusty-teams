//! The in-call "Meeting info" panel: the join link and dial-in numbers Teams shows
//! next to "Meeting options...", sourced from the calendar event behind the meeting.

use chatsvc::DialIn;

/// State for the "Meeting info" panel opened from the call's "..." menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingInfoDialog {
    pub join_url: Option<String>,
    pub dial_in: Option<DialIn>,
    /// A join-link fetch is in flight because no matching calendar event carried one.
    pub loading: bool,
}

impl MeetingInfoDialog {
    /// Built from a calendar lookup; keeps fetching the join link when the lookup had none
    /// (a Meet Now with no invite has no matching event at all).
    pub fn from_calendar(join_url: Option<String>, dial_in: Option<DialIn>) -> Self {
        let loading = join_url.is_none();
        MeetingInfoDialog {
            join_url,
            dial_in,
            loading,
        }
    }

    /// A join-link fetch finished; a meeting with no online meeting stays without one.
    pub fn join_url_loaded(&mut self, join_url: Option<String>) {
        self.loading = false;
        if self.join_url.is_none() {
            self.join_url = join_url;
        }
    }

    /// The exact text Teams puts on the clipboard for "Copy join info"; `None` while the
    /// join link isn't known yet.
    pub fn copy_text(&self) -> Option<String> {
        Some(chatsvc::join_info_text(
            self.join_url.as_deref()?,
            self.dial_in.as_ref(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dial_in() -> DialIn {
        DialIn {
            toll_number: "+49 30 123456".into(),
            conference_id: "987654321".into(),
        }
    }

    #[test]
    fn a_matching_calendar_event_carries_dial_in_and_needs_no_fetch() {
        let dialog = MeetingInfoDialog::from_calendar(
            Some("https://teams.microsoft.com/l/meetup-join/x".into()),
            Some(dial_in()),
        );
        assert!(!dialog.loading);
        assert_eq!(
            dialog.copy_text().as_deref(),
            Some(
                "Microsoft Teams meeting\nJoin: https://teams.microsoft.com/l/meetup-join/x\n\nDial in by phone\n+49 30 123456,,987654321#\nPhone conference ID: 987654321#"
            )
        );
    }

    #[test]
    fn a_meet_now_with_no_invite_degrades_to_just_the_join_link() {
        let mut dialog = MeetingInfoDialog::from_calendar(None, None);
        assert!(dialog.loading);
        assert_eq!(dialog.copy_text(), None);
        dialog.join_url_loaded(Some("https://teams.microsoft.com/l/meetup-join/y".into()));
        assert!(!dialog.loading);
        assert_eq!(
            dialog.copy_text().as_deref(),
            Some("Microsoft Teams meeting\nJoin: https://teams.microsoft.com/l/meetup-join/y")
        );
    }

    #[test]
    fn a_failed_fetch_leaves_the_dialog_without_a_join_link() {
        let mut dialog = MeetingInfoDialog::from_calendar(None, None);
        dialog.join_url_loaded(None);
        assert!(!dialog.loading);
        assert_eq!(dialog.copy_text(), None);
    }
}
