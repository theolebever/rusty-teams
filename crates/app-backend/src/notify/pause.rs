//! "Pause notifications": the end time, kept in the store's meta table.

use chrono::{DateTime, Utc};
use store::Store;

pub const META_KEY: &str = "notify_paused_until";

pub fn load(store: &Store) -> Option<DateTime<Utc>> {
    store
        .meta(META_KEY)
        .ok()
        .flatten()
        .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
        .map(|until| until.with_timezone(&Utc))
}

pub fn is_paused(until: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    until.is_some_and(|until| now < until)
}
