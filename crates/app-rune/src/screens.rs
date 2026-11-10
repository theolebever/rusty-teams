//! Calendar and activity screens backed by the same account data as the main application.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};
use rune::tok::Family;
use rune::*;
use teams_core::{ActivityItem, ActivitySnapshot};

use crate::types::Actions;
use crate::ui::{self, UiState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Chat,
    Activity,
    Calendar,
    Calls,
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarRow {
    pub id: String,
    pub subject: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub cancelled: bool,
    pub all_day: bool,
    pub organizer: Option<String>,
    pub ical_uid: Option<String>,
    pub join_url: Option<String>,
    pub thread_id: Option<String>,
    pub response: chatsvc::ResponseType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivityFilter {
    #[default]
    All,
    Mentions,
    Unread,
}

pub struct ScreenState {
    pub screen: Screen,
    pub activity: ActivitySnapshot,
    pub calendar: Vec<CalendarRow>,
    pub week: NaiveDate,
    pub filter: ActivityFilter,
    pub selected_event: Option<String>,
    pub send_response: bool,
}

impl Default for ScreenState {
    fn default() -> Self {
        Self {
            screen: Screen::Chat,
            activity: ActivitySnapshot::default(),
            calendar: Vec::new(),
            week: monday(Local::now().date_naive()),
            filter: ActivityFilter::All,
            selected_event: None,
            send_response: true,
        }
    }
}

fn monday(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

fn occurs_on(event: &CalendarRow, date: NaiveDate) -> bool {
    let start = event.start.with_timezone(&Local).date_naive();
    let end = (event.end - Duration::nanoseconds(1))
        .with_timezone(&Local)
        .date_naive();
    event.end > event.start && start <= date && date <= end
}

pub fn navigate<A: Actions>(s: &mut UiState<A>, screen: Screen) {
    s.screens.screen = screen;
    if screen == Screen::Calendar {
        s.actions.load_calendar(s.screens.week);
    }
}

fn shift_week<A: Actions>(s: &mut UiState<A>, days: i64) {
    s.screens.week += Duration::days(days);
    s.screens.calendar.clear();
    s.screens.selected_event = None;
    s.actions.load_calendar(s.screens.week);
}

fn color(hex: &str) -> Color {
    crate::settings::color(hex)
}

fn style(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Segoe UI"))
}

fn button<A: Actions + 'static>(
    label: &str,
    key: &str,
    action: impl Fn(&mut UiState<A>, &mut Tx) + 'static,
) -> El {
    row()
        .key(key.to_owned())
        .h(32.0)
        .px(12.0)
        .center()
        .radius(6.0)
        .fill(color("#F5F5F5"))
        .hover(color("#EBEBEB"))
        .on_press_tx(action)
        .child(
            text(label.to_owned())
                .font(style(13.0, 600.0))
                .color(color("#242424")),
        )
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    match s.screens.screen {
        Screen::Activity => activity(s),
        Screen::Calendar => calendar(s),
        Screen::Calls => crate::calls::view(s),
        Screen::Settings => crate::settings::view(s),
        Screen::Chat => col(),
    }
}

fn calendar<A: Actions + 'static>(s: &UiState<A>) -> El {
    let week = s.screens.week;
    let header = row()
        .w_full()
        .h(56.0)
        .px(20.0)
        .gap(8.0)
        .center_y()
        .child(
            text("Calendar")
                .font(style(18.0, 700.0))
                .color(color("#242424")),
        )
        .child(button::<A>("Today", "calendar-today", |s, _| {
            s.screens.week = monday(Local::now().date_naive());
            s.screens.calendar.clear();
            s.actions.load_calendar(s.screens.week);
        }))
        .child(button::<A>("‹", "calendar-previous", |s, _| {
            shift_week(s, -7)
        }))
        .child(button::<A>("›", "calendar-next", |s, _| shift_week(s, 7)))
        .child(
            text(format!(
                "{} – {}",
                week.format("%d %b"),
                (week + Duration::days(6)).format("%d %b %Y")
            ))
            .font(style(14.0, 600.0))
            .color(color("#242424")),
        );
    let mut rows = col().w_full().px(20.0).gap(8.0);
    for day in 0..7 {
        let date = week + Duration::days(day);
        rows = rows.child(
            text(date.format("%A, %d %B").to_string())
                .font(style(14.0, 700.0))
                .color(color("#242424")),
        );
        for event in s
            .screens
            .calendar
            .iter()
            .filter(|event| occurs_on(event, date))
        {
            let time = if event.all_day {
                "All day".to_owned()
            } else {
                format!(
                    "{} – {}",
                    event.start.with_timezone(&Local).format("%H:%M"),
                    event.end.with_timezone(&Local).format("%H:%M")
                )
            };
            let title = if event.cancelled {
                format!("Canceled: {}", event.subject)
            } else {
                event.subject.clone()
            };
            let detail = event
                .organizer
                .as_deref()
                .map(|name| format!("{time} · {name}"))
                .unwrap_or(time);
            let event_key = format!("{}-{date}", event.id);
            let selected = s.screens.selected_event.as_deref() == Some(event_key.as_str());
            rows = rows.child(
                col()
                    .key(format!("calendar-{}-{date}", event.id))
                    .w_full()
                    .padding(10.0)
                    .radius(6.0)
                    .fill(color("#E8EBFA"))
                    .hover(color("#DADDF5"))
                    .on_press_tx(move |s: &mut UiState<A>, _| {
                        s.screens.selected_event = Some(event_key.clone())
                    })
                    .child(
                        text(title)
                            .font(style(14.0, 600.0))
                            .color(color("#242424"))
                            .wrap(),
                    )
                    .child(
                        text(detail)
                            .font(style(12.0, 400.0))
                            .color(color("#616161")),
                    ),
            );
            if selected {
                let mut details = col()
                    .w_full()
                    .padding(12.0)
                    .gap(8.0)
                    .fill(color("#F5F5F5"))
                    .child(
                        text(format!(
                            "{} – {}",
                            event.start.with_timezone(&Local).format("%d %b %Y %H:%M"),
                            event.end.with_timezone(&Local).format("%d %b %Y %H:%M")
                        ))
                        .font(style(13.0, 400.0))
                        .color(color("#242424")),
                    );
                if let Some(url) = &event.join_url
                    && !s.read_only
                    && !event.cancelled
                {
                    let url = url.clone();
                    details = details.child(button::<A>("Join", "calendar-join", move |s, _| {
                        s.calls.link = TextEdit::new(&url);
                        s.screens.screen = Screen::Calls;
                    }));
                }
                if let Some(thread) = &event.thread_id {
                    let thread = thread.clone();
                    details = details.child(button::<A>("Chat", "calendar-chat", move |s, tx| {
                        s.screens.screen = Screen::Chat;
                        ui::open_chat(s, &thread, tx);
                    }));
                }
                if !s.read_only
                    && !event.cancelled
                    && event.response != chatsvc::ResponseType::Organizer
                {
                    let send_response = s.screens.send_response;
                    details = details.child(button::<A>(
                        if send_response {
                            "✓ Send response"
                        } else {
                            "Send response"
                        },
                        "calendar-send-response",
                        |s, _| s.screens.send_response = !s.screens.send_response,
                    ));
                    let mut choices = row().gap(8.0);
                    for (label, reply) in [
                        ("Accept", chatsvc::Reply::Accept),
                        ("Tentative", chatsvc::Reply::Tentative),
                        ("Decline", chatsvc::Reply::Decline),
                    ] {
                        let id = event.id.clone();
                        let uid = event.ical_uid.clone();
                        choices = choices.child(button::<A>(
                            label,
                            &format!("calendar-rsvp-{label}"),
                            move |s, _| {
                                s.actions.respond_event(
                                    &id,
                                    uid.as_deref(),
                                    reply,
                                    s.screens.send_response,
                                );
                            },
                        ));
                    }
                    details = details.child(choices);
                }
                details = details.child(button::<A>("Close", "calendar-close", |s, _| {
                    s.screens.selected_event = None
                }));
                rows = rows.child(details);
            }
        }
    }
    col()
        .key("calendar")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .fill(color("#FFFFFF"))
        .child(header)
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(rows),
        )
}

fn matches_filter(
    snapshot: &ActivitySnapshot,
    item: &ActivityItem,
    filter: ActivityFilter,
) -> bool {
    match filter {
        ActivityFilter::All => true,
        ActivityFilter::Unread => !snapshot.is_read(item),
        ActivityFilter::Mentions => matches!(
            item.activity_type.as_str(),
            "mention"
                | "mentionInChat"
                | "mentionInCommunity"
                | "mentionInSubCommunity"
                | "communityMention"
        ),
    }
}

fn activity<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut header = row().w_full().h(56.0).px(20.0).gap(8.0).center_y().child(
        text("Activity")
            .font(style(18.0, 700.0))
            .color(color("#242424")),
    );
    for (label, filter) in [
        ("All", ActivityFilter::All),
        ("@Mentions", ActivityFilter::Mentions),
        ("Unread", ActivityFilter::Unread),
    ] {
        header = header.child(button::<A>(
            label,
            &format!("activity-filter-{label}"),
            move |s, _| s.screens.filter = filter,
        ));
    }
    if !s.read_only {
        header = header.child(button::<A>(
            "Mark all as read",
            "activity-read-all",
            |s, _| s.actions.mark_all_activity_read(),
        ));
    }
    let mut rows = col().w_full().px(20.0).gap(4.0);
    for item in s
        .screens
        .activity
        .items
        .iter()
        .filter(|item| matches_filter(&s.screens.activity, item, s.screens.filter))
    {
        let conversation = if item.source_thread_id.is_empty() {
            item.meeting_thread_id.clone().unwrap_or_default()
        } else {
            item.source_thread_id.clone()
        };
        let id = item.message_id.clone();
        let unread = !s.screens.activity.is_read(item);
        let mut row = col()
            .key(format!("activity-{id}"))
            .w_full()
            .padding(12.0)
            .gap(4.0)
            .radius(6.0)
            .fill(color(if unread { "#E8EBFA" } else { "#F5F5F5" }))
            .child(
                text(item.source_user_name.clone())
                    .font(style(14.0, 700.0))
                    .color(color("#242424")),
            )
            .child(
                text(item.preview.clone())
                    .font(style(13.0, 400.0))
                    .color(color("#242424"))
                    .wrap(),
            );
        if !conversation.is_empty() {
            row =
                row.hover(color("#EBEBEB"))
                    .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| {
                        if !s.read_only && unread {
                            s.actions.mark_activity_read(&id);
                        }
                        s.screens.screen = Screen::Chat;
                        ui::open_chat(s, &conversation, tx);
                    });
        }
        rows = rows.child(row);
    }
    col()
        .key("activity")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .fill(color("#FFFFFF"))
        .child(header)
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(rows),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake { responses: RefCell<Vec<(String, chatsvc::Reply, bool)>> }
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn respond_event(&self, id: &str, _: Option<&str>, reply: chatsvc::Reply, send_response: bool) {
            self.responses.borrow_mut().push((id.to_owned(), reply, send_response));
        }
    }

    #[test]
    fn week_starts_on_monday_and_activity_filters_respect_read_horizon() {
        let sunday = NaiveDate::from_ymd_opt(2026, 11, 8).unwrap();
        assert_eq!(
            monday(sunday),
            NaiveDate::from_ymd_opt(2026, 11, 2).unwrap()
        );
        let item = ActivityItem {
            activity_type: "mentionInChat".into(),
            version_ms: 10,
            ..Default::default()
        };
        let mut snapshot = ActivitySnapshot::default();
        assert!(matches_filter(&snapshot, &item, ActivityFilter::Mentions));
        assert!(matches_filter(&snapshot, &item, ActivityFilter::Unread));
        snapshot.horizons.bookmark_ms = 10;
        assert!(!matches_filter(&snapshot, &item, ActivityFilter::Unread));
    }

    #[test]
    fn calendar_midnight_end_does_not_repeat_on_the_next_day() {
        use chrono::TimeZone;
        let start = Local
            .with_ymd_and_hms(2026, 11, 2, 0, 0, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let end = Local
            .with_ymd_and_hms(2026, 11, 3, 0, 0, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let event = CalendarRow {
            id: "1".into(),
            subject: "Event".into(),
            start,
            end,
            cancelled: false,
            all_day: true,
            organizer: None,
            ical_uid: None,
            join_url: None,
            thread_id: None,
            response: chatsvc::ResponseType::NotResponded,
        };
        assert!(occurs_on(
            &event,
            NaiveDate::from_ymd_opt(2026, 11, 2).unwrap()
        ));
        assert!(!occurs_on(
            &event,
            NaiveDate::from_ymd_opt(2026, 11, 3).unwrap()
        ));
    }

    #[test]
    fn calendar_rsvp_uses_selected_event_and_response_choice() {
        use chrono::TimeZone;
        let date = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let start = Local.from_local_datetime(&date.and_hms_opt(9, 0, 0).unwrap()).single().unwrap().with_timezone(&Utc);
        let mut state = UiState::new(Fake::default());
        state.screens.screen = Screen::Calendar;
        state.screens.week = date;
        state.screens.calendar = vec![CalendarRow {
            id: "meeting".into(), subject: "Meeting".into(), start, end: start + Duration::hours(1),
            cancelled: false, all_day: false, organizer: None, ical_uid: None, join_url: None,
            thread_id: None, response: chatsvc::ResponseType::NotResponded,
        }];
        let mut runtime = Runtime::new(state, crate::ui::view::<Fake>, (1000.0, 700.0), Env { system_dark: false, scale: 1.0 });
        runtime.frame(0.0);
        for key in ["calendar-meeting-2026-11-02", "calendar-send-response", "calendar-rsvp-Accept"] {
            let (x, y) = runtime.find(key).unwrap_or_else(|| panic!("missing {key}"));
            runtime.pointer_down(x, y);
            runtime.pointer_up(x, y);
            runtime.frame(0.0);
        }
        assert_eq!(*runtime.state.actions.responses.borrow(), [("meeting".into(), chatsvc::Reply::Accept, false)]);
    }
}
