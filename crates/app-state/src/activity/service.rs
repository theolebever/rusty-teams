use chrono::Utc;
use teams_core::{ActivitySnapshot, stored_activity_feed};

use super::feed::{Entry, Feed};
use super::server::feed_of;
use crate::Host;
use crate::app_state::AppState;
use crate::demo;

const SEEN_KEY: &str = "activity_seen_ms";

/// The activity feed and what the user has read and seen of it.
/// The UI wraps it, feeds it state events and redraws after each call.
pub struct ActivityModel {
    snapshot: ActivitySnapshot,
    seen_locally_ms: i64,
    feed: Feed,
}

impl ActivityModel {
    pub fn new(state: &mut AppState, cx: &mut dyn Host) -> Self {
        let snapshot = if state.mode.demo {
            demo::activity_snapshot()
        } else {
            stored_activity_feed(&state.store).unwrap_or_default()
        };
        let seen_locally_ms = state
            .store
            .meta(SEEN_KEY)
            .ok()
            .flatten()
            .and_then(|text| text.parse().ok())
            .unwrap_or(0);
        let feed = feed_of(&snapshot, seen_locally_ms);
        let center = ActivityModel {
            snapshot,
            seen_locally_ms,
            feed,
        };
        center.request_avatars(state, cx);
        center
    }

    pub fn feed(&self) -> &Feed {
        &self.feed
    }

    pub fn mark_read(&mut self, id: &str, state: &mut AppState, cx: &mut dyn Host) {
        self.mark_read_where(|entry| entry.id == id, state, cx);
    }

    pub fn mark_conversation_read(
        &mut self,
        conversation_id: &str,
        state: &mut AppState,
        cx: &mut dyn Host,
    ) {
        self.mark_read_where(|entry| entry.conversation_id == conversation_id, state, cx);
    }

    pub fn mark_all_read(&mut self, state: &mut AppState, cx: &mut dyn Host) {
        if self.feed.unread_count() == 0 {
            return;
        }
        self.snapshot.mark_all_read(Utc::now().timestamp_millis());
        self.rebuild();
        state.mark_all_activity_read(cx);
    }

    /// Teams resets the bell when the feed is opened.
    pub fn mark_seen(&mut self, state: &AppState) {
        if self.feed.bell_count() == 0 {
            return;
        }
        self.seen_locally_ms = Utc::now().timestamp_millis();
        let _ = state.store.set_meta(SEEN_KEY, &self.seen_locally_ms.to_string());
        self.rebuild();
    }

    /// Reloads the stored feed after `AppEvent::Activity`.
    pub fn reload(&mut self, state: &mut AppState, cx: &mut dyn Host) {
        if state.mode.demo {
            return;
        }
        let Some(snapshot) = stored_activity_feed(&state.store) else {
            return;
        };
        self.snapshot = snapshot;
        self.rebuild();
        self.request_avatars(state, cx);
    }

    fn mark_read_where(
        &mut self,
        matches: impl Fn(&Entry) -> bool,
        state: &mut AppState,
        cx: &mut dyn Host,
    ) {
        let message_ids: Vec<String> = self
            .feed
            .unread_entries()
            .filter(|entry| matches(entry))
            .map(|entry| entry.id.clone())
            .collect();
        if message_ids.is_empty() {
            return;
        }
        for message_id in &message_ids {
            self.snapshot.mark_read(message_id);
        }
        self.rebuild();
        state.mark_activity_read(message_ids, cx);
    }

    fn rebuild(&mut self) {
        self.feed = feed_of(&self.snapshot, self.seen_locally_ms);
    }

    fn request_avatars(&self, state: &mut AppState, cx: &mut dyn Host) {
        let mut user_ids: Vec<String> = self
            .feed
            .entries()
            .iter()
            .filter_map(|entry| entry.latest_actor()?.user_id.clone())
            .collect();
        user_ids.sort();
        user_ids.dedup();
        if !user_ids.is_empty() {
            state.request_avatars(user_ids, cx);
        }
    }
}
