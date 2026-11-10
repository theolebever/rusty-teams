use chatsvc::{Attendee, Repeat};
use chrono::{DateTime, Duration, FixedOffset, NaiveDateTime, Utc};

const AGENDA_DAYS: i64 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitee {
    pub user_id: String,
    pub name: String,
    pub mail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingDraft {
    pub subject: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub local_start: NaiveDateTime,
    pub local_end: NaiveDateTime,
    pub time_zone_name: Option<String>,
    pub repeat: Repeat,
    pub details: String,
    pub people: Vec<Invitee>,
}

pub fn attendee_for(invitee: &Invitee, looked_up: Option<String>) -> Result<Attendee, String> {
    let address = invitee
        .mail
        .clone()
        .or(looked_up)
        .ok_or_else(|| format!("No email address found for {}.", invitee.name))?;
    Ok(Attendee {
        name: invitee.name.clone(),
        address,
    })
}

pub fn fetch_window(now: DateTime<Utc>, offset: FixedOffset) -> (DateTime<Utc>, DateTime<Utc>) {
    let midnight = now
        .with_timezone(&offset)
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|local| local.and_local_timezone(offset).single())
        .map_or(now, |local| local.with_timezone(&Utc));
    (midnight, midnight + Duration::days(AGENDA_DAYS))
}

