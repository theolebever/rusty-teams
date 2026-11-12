//! Calendar and activity screens backed by the same account data as the main application.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};
use rune::tok::Family;
use rune::*;
use crate::ui::text;
use teams_core::{ActivityItem, ActivitySnapshot};

use crate::types::Actions;
use crate::ui::{self, UiState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Chat,
    Teams,
    Agents,
    Activity,
    Calendar,
    Agenda,
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
    Replies,
}

pub struct ScreenState {
    pub selected_activity: Option<String>,
    pub activity_search: TextEdit,
    pub screen: Screen,
    pub activity: ActivitySnapshot,
    pub calendar: Vec<CalendarRow>,
    pub agenda: Vec<chatsvc::CalendarEvent>,
    pub week: NaiveDate,
    pub filter: ActivityFilter,
    pub selected_event: Option<String>,
    pub send_response: bool,
}

impl Default for ScreenState {
    fn default() -> Self {
        Self {
            selected_activity: None,
            activity_search: TextEdit::default(),
            screen: Screen::Chat,
            activity: ActivitySnapshot::default(),
            calendar: Vec::new(),
            agenda: Vec::new(),
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

#[cfg(test)]
fn occurs_on(event: &CalendarRow, date: NaiveDate) -> bool {
    let start = event.start.with_timezone(&Local).date_naive();
    let end = (event.end - Duration::nanoseconds(1))
        .with_timezone(&Local)
        .date_naive();
    event.end > event.start && start <= date && date <= end
}

pub fn navigate<A: Actions>(s: &mut UiState<A>, screen: Screen) {
    if s.screens.screen != screen { crate::view_composer::stop_typing(s); }
    if screen == Screen::Teams && s.selected.as_ref().is_some_and(|id| !s.teams.iter().flat_map(|t| &t.channels).any(|c| &c.id == id)) {
        ui::save_draft(s);
        s.selected = None;
        s.messages.clear();
        s.pinned = None;
    }
    s.screens.screen = screen;
    if screen == Screen::Calendar {
        s.actions.load_calendar(s.screens.week);
    }
    if screen == Screen::Calls {
        if !s.calls.state.as_ref().is_some_and(calling::CallState::is_active) && s.calls.ring.is_none() {
            s.calls.prejoin_open = false;
            s.calls.link.clear();
        }
        s.calls.history_loading = true;
        s.actions.load_call_history();
    }
}

fn shift_week<A: Actions>(s: &mut UiState<A>, days: i64) {
    s.screens.week += Duration::days(days);
    s.screens.calendar.clear();
    s.screens.selected_event = None;
    s.actions.load_calendar(s.screens.week);
}

pub(crate) fn color(hex: &str) -> Color {
    crate::settings::color(hex)
}

pub(crate) fn style(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Segoe UI"))
}

pub(crate) fn button<A: Actions + 'static>(
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
        Screen::Agenda => crate::agenda::view(s),
        Screen::Calls => crate::calls::view(s),
        Screen::Settings => crate::settings::view(s),
        Screen::Chat => col(),
        Screen::Teams => col()
            .key("teams")
            .w(320.0)
            .h_full()
            .min_h(0.0)
            .child(
                text("Teams")
                    .font(style(18.0, 600.0))
                    .color(color("#242424"))
                    .padding(20.0),
            )
            .child(
                col()
                    .grow(1.0)
                    .min_h(0.0)
                    .w_full()
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(crate::view_chatlist::teams(s)),
            ),
        Screen::Agents => crate::agent_panel::view(s),
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
        .child(button::<A>("Agenda", "calendar-agenda", |s, _| {
            s.screens.screen = Screen::Agenda;
            s.actions.load_agenda();
        }))
        .child(
            text(format!(
                "{} – {}",
                week.format("%d %b"),
                (week + Duration::days(6)).format("%d %b %Y")
            ))
            .font(style(14.0, 600.0))
            .color(color("#242424")),
        );
    let mut body = row()
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .child(crate::calendar_grid::week(s));
    if let Some(event) = s.screens.calendar.iter().find(|event| {
        (0..7).any(|day| {
            s.screens.selected_event.as_deref()
                == Some(format!("{}-{}", event.id, week + Duration::days(day)).as_str())
        })
    }) {
        body = body.child(
            col()
                .key("calendar-details")
                .w(280.0)
                .h_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(calendar_details(s, event)),
        );
    }
    col()
        .key("calendar")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .fill(color("#FFFFFF"))
        .instant_changes()
        .child(header)
        .child(body)
}

fn calendar_details<A: Actions + 'static>(s: &UiState<A>, event: &CalendarRow) -> El {
    let mut details = col()
        .w_full()
        .padding(12.0)
        .gap(8.0)
        .fill(color("#F5F5F5"))
        .child(text(event.subject.clone()).key("calendar-detail-subject")
            .font(style(16.0, 600.0)).color(color("#242424")).wrap())
        .child(
            text(format!(
                "{} – {}",
                event.start.with_timezone(&Local).format("%d %b %Y %H:%M"),
                event.end.with_timezone(&Local).format("%d %b %Y %H:%M")
            ))
            .font(style(13.0, 400.0))
            .color(color("#242424")),
        );
    if let Some(organizer) = &event.organizer {
        details = details.child(text(format!("Organizer: {organizer}"))
            .key("calendar-detail-organizer").font(style(13.0, 400.0))
            .color(color("#616161")).wrap());
    }
    if event.all_day {
        details = details.child(text("All day").key("calendar-detail-all-day")
            .font(style(13.0, 400.0)).color(color("#616161")));
    }
    if event.cancelled {
        details = details.child(text("Cancelled").key("calendar-detail-cancelled")
            .font(style(13.0, 600.0)).color(color("#C4314B")));
    }
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
    if !s.read_only && !event.cancelled && event.response != chatsvc::ResponseType::Organizer {
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
                    s.actions
                        .respond_event(&id, uid.as_deref(), reply, s.screens.send_response);
                },
            ));
        }
        details = details.child(choices);
    }
    details = details.child(button::<A>("Close", "calendar-close", |s, _| {
        s.screens.selected_event = None
    }));
    details
}

fn matches_filter(
    snapshot: &ActivitySnapshot,
    item: &ActivityItem,
    filter: ActivityFilter,
) -> bool {
    match filter {
        ActivityFilter::All => true,
        ActivityFilter::Unread => !snapshot.is_read(item),
        ActivityFilter::Replies => matches!(item.activity_type.as_str(), "reply" | "replyToReply" | "replyInChat" | "communityReply" | "communityReplyToReply"),
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
    use crate::avatar::rgb;
    use crate::icons::sym;
    let palette = app_backend::schemes::active_palette();
    let mut header = row().w_full().h(56.0).px(20.0).gap(8.0).center_y().child(
        text("Activity")
            .font(style(18.0, 700.0))
            .color(rgb(palette.foreground)).grow(1.0),
    );
    if !s.read_only {
        header = header.child(row().key("activity-read-all").size(32.0, 32.0).center().radius(4.0)
            .label("Mark all as read").hover(rgb(palette.row_hover))
            .on_press(|s: &mut UiState<A>| s.actions.mark_all_activity_read())
            .child(sym("done_all", 20.0).color(rgb(palette.text_muted))));
    }
    let mut filters = row().w_full().px(16.0).gap(6.0).pb(12.0);
    for (label, filter) in [
        ("All", ActivityFilter::All),
        ("Unread", ActivityFilter::Unread),
        ("@Mentions", ActivityFilter::Mentions),
        ("Replies", ActivityFilter::Replies),
    ] {
        let selected = s.screens.filter == filter;
        filters = filters.child(row().key(format!("activity-filter-{label}")).h(28.0).px(9.0).center().radius(14.0)
            .fill(rgb(if selected { palette.selection } else { palette.surface_raised }))
            .hover(rgb(palette.row_hover)).label(label)
            .on_press(move |s: &mut UiState<A>| s.screens.filter = filter)
            .child(text(label).font(style(12.0, if selected { 600.0 } else { 400.0 }))
                .color(rgb(if selected { palette.accent } else { palette.foreground }))));
    }
    let query = s.screens.activity_search.text.trim().to_lowercase();
    let search = row().w_full().px(16.0).pb(12.0).child(
        row().w_full().h(32.0).px(8.0).gap(8.0).center_y().radius(4.0).fill(rgb(palette.surface_raised))
            .child(sym("search", 16.0).color(rgb(palette.text_muted)))
            .child(text_input(&s.screens.activity_search, style(13.0, 400.0)).key("activity-search").grow(1.0).min_w(0.0)
                .placeholder("Find in activity", rgb(palette.text_muted)).color(rgb(palette.foreground))
                .on_edit(|s: &mut UiState<A>| &mut s.screens.activity_search)));
    let mut rows = col().w_full().px(8.0).gap(2.0);
    let mut count = 0;
    let mut previous_day = None;
    for item in s
        .screens
        .activity
        .items
        .iter()
        .filter(|item| matches_filter(&s.screens.activity, item, s.screens.filter))
        .filter(|item| query.is_empty() || format!("{} {} {}", item.source_user_name, item.preview, item.thread_topic.as_deref().unwrap_or_default()).to_lowercase().contains(&query))
    {
        count += 1;
        let at = DateTime::from_timestamp_millis(item.timestamp_ms).unwrap_or_default().with_timezone(&Local);
        let day = at.date_naive();
        if previous_day != Some(day) {
            let today = Local::now().date_naive();
            let label = if day == today { "Today".into() } else if day == today - Duration::days(1) { "Yesterday".into() } else { at.format("%d %b").to_string() };
            rows = rows.child(text(label).font(style(12.0, 600.0)).color(rgb(palette.text_muted)).px(12.0).py(10.0));
            previous_day = Some(day);
        }
        let conversation = if item.source_thread_id.is_empty() {
            item.meeting_thread_id.clone().unwrap_or_default()
        } else {
            item.source_thread_id.clone()
        };
        let id = item.message_id.clone();
        let unread = !s.screens.activity.is_read(item);
        let selected = s.screens.selected_activity.as_deref() == Some(&id);
        let (headline, icon) = activity_headline(item);
        let context = item.thread_topic.clone().or_else(|| s.chats.iter().find(|chat| chat.id == conversation).map(|chat| chat.title.clone())).unwrap_or_default();
        let mut notification = row()
            .key(format!("activity-{id}"))
            .w_full()
            .padding(10.0).gap(10.0).radius(4.0)
            .fill(rgb(if selected { palette.selection } else { palette.surface }))
            .child(col().w(6.0).pt(14.0).child(el().size(6.0, 6.0).radius(3.0)
                .fill(rgb(if unread { palette.accent } else if selected { palette.selection } else { palette.surface }))))
            .child(el().size(36.0, 40.0)
                .child(crate::avatar::person(s, &item.source_user_name, item.source_user_id.as_deref(), 32.0, None))
                .child(el().size(18.0, 18.0).radius(9.0).center().fill(rgb(palette.surface))
                    .absolute(20.0, 22.0).child(sym(icon, 12.0).color(rgb(palette.accent)))))
            .child(col().grow(1.0).min_w(0.0).gap(4.0)
                .child(row().w_full().gap(8.0).center_y()
                    .child(text(headline).font(style(13.0, if unread { 600.0 } else { 400.0 })).color(rgb(palette.foreground)).truncate().grow(1.0).min_w(0.0))
                    .child(text(at.format("%H:%M").to_string()).font(style(11.0, 400.0)).color(rgb(palette.text_muted))))
                .children((!context.is_empty()).then(|| text(context).font(style(12.0, 400.0)).color(rgb(palette.text_muted)).truncate()))
                .child(text(teams_core::preview_text(&item.preview).unwrap_or_default()).font(style(12.0, 400.0)).color(rgb(palette.text_soft)).truncate()));
        if !conversation.is_empty() {
            let root = item.source_reply_chain_id.clone();
            notification =
                notification.hover(rgb(palette.row_hover))
                    .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| {
                        if !s.read_only && unread {
                            s.actions.mark_activity_read(&id);
                        }
                        s.screens.selected_activity = Some(id.clone());
                        ui::open_chat(s, &conversation, tx);
                        if ui::active_target(s).is_some_and(|target| target.channel) && root.is_some() { ui::select_thread(s, root.clone()); }
                    });
        }
        rows = rows.child(notification);
    }
    if count == 0 {
        rows = rows.child(col().w_full().padding(24.0).gap(12.0).items_center()
            .child(sym("notifications", 32.0).color(rgb(palette.text_muted)))
            .child(text("You're all caught up").font(style(16.0, 600.0)).color(rgb(palette.foreground)))
            .child(text("No activity matches these filters.").font(style(13.0, 400.0)).color(rgb(palette.text_muted)).wrap()));
    }
    col()
        .key("activity")
        .w(360.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .fill(rgb(palette.surface))
        .child(header)
        .child(search)
        .child(filters)
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

fn activity_headline(item: &ActivityItem) -> (String, &'static str) {
    let actor = if item.source_user_name.trim().is_empty() { item.source_app_name.as_deref().unwrap_or("Teams") } else { &item.source_user_name };
    match item.activity_type.as_str() {
        "mention" | "mentionInChat" | "mentionInCommunity" | "mentionInSubCommunity" | "communityMention" => (format!("{actor} mentioned you"), "alternate_email"),
        "reply" | "replyToReply" | "replyInChat" | "communityReply" | "communityReplyToReply" => (format!("{actor} replied"), "reply"),
        "reaction" | "reactionInChat" | "like" | "likeInChat" | "reactionInCommunity" | "reactionInSubCommunity" => (format!("{actor} reacted to your message"), "thumb_up"),
        "call" if item.activity_subtype == "missedCall" => (format!("Missed call from {actor}"), "call"),
        "call" | "voicemail" | "callOrMeetup" => (actor.to_owned(), "call"),
        "teamMembershipChange" => (format!("{actor} updated your team membership"), "group"),
        _ => (actor.to_owned(), "notifications"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        responses: RefCell<Vec<(String, chatsvc::Reply, bool)>>,
    }
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn respond_event(
            &self,
            id: &str,
            _: Option<&str>,
            reply: chatsvc::Reply,
            send_response: bool,
        ) {
            self.responses
                .borrow_mut()
                .push((id.to_owned(), reply, send_response));
        }
    }

    #[test]
    fn activity_keeps_the_feed_alongside_the_selected_conversation() {
        let mut state = UiState::new(Fake::default());
        state.screens.screen = Screen::Activity;
        state.screens.activity.items = vec![ActivityItem {
            message_id: "notification".into(), source_thread_id: "chat".into(),
            source_user_name: "Ada".into(), activity_type: "mentionInChat".into(),
            timestamp_ms: Utc::now().timestamp_millis(), version_ms: 1,
            preview: "Please review".into(), ..Default::default()
        }];
        let mut rt = Runtime::new(state, crate::ui::view::<Fake>, (1100.0, 800.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        assert!(rt.find("activity-preview").is_some());
        let (x, y) = rt.find("activity-notification").unwrap();
        rt.pointer_down(x, y); rt.pointer_up(x, y); rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.screens.screen, Screen::Activity);
        assert_eq!(rt.state.selected.as_deref(), Some("chat"));
        assert!(rt.find("activity-notification").is_some());
        assert!(rt.find("conversation").is_some());
        assert!(rt.find("activity-preview").is_none());
        assert_eq!(activity_headline(&rt.state.screens.activity.items[0]).0, "Ada mentioned you");
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
    fn cancelled_all_day_calendar_details_show_context_without_actions() {
        let now = Utc::now();
        let mut state = UiState::new(Fake::default());
        state.screens.calendar = vec![CalendarRow {
            id: "cancelled".into(), subject: "Planning".into(),
            start: now, end: now + Duration::days(1), cancelled: true, all_day: true,
            organizer: Some("Ada".into()), ical_uid: None,
            join_url: Some("https://teams.microsoft.com/meeting".into()), thread_id: None,
            response: chatsvc::ResponseType::NotResponded,
        }];
        let mut runtime = Runtime::new(state,
            |s: &UiState<Fake>, _: &mut Cx<UiState<Fake>>| calendar_details(s, &s.screens.calendar[0]),
            (400.0, 600.0), Env { system_dark: false, scale: 1.0 });
        runtime.frame(0.0);
        for key in ["calendar-detail-subject", "calendar-detail-organizer", "calendar-detail-all-day", "calendar-detail-cancelled"] {
            assert!(runtime.find(key).is_some(), "missing {key}");
        }
        assert!(runtime.find("calendar-join").is_none());
        assert!(runtime.find("calendar-rsvp-Accept").is_none());
    }

    #[test]
    fn calendar_rsvp_uses_selected_event_and_response_choice() {
        use chrono::TimeZone;
        let date = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let start = Local
            .from_local_datetime(&date.and_hms_opt(9, 0, 0).unwrap())
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let mut state = UiState::new(Fake::default());
        state.screens.screen = Screen::Calendar;
        state.screens.week = date;
        state.screens.calendar = vec![CalendarRow {
            id: "meeting".into(),
            subject: "Meeting".into(),
            start,
            end: start + Duration::hours(1),
            cancelled: false,
            all_day: false,
            organizer: None,
            ical_uid: None,
            join_url: None,
            thread_id: None,
            response: chatsvc::ResponseType::NotResponded,
        }];
        let mut runtime = Runtime::new(
            state,
            crate::ui::view::<Fake>,
            (1000.0, 700.0),
            Env {
                system_dark: false,
                scale: 1.0,
            },
        );
        runtime.frame(1.0);
        for key in [
            "calendar-meeting-2026-11-02",
            "calendar-send-response",
            "calendar-rsvp-Accept",
        ] {
            let (x, y) = runtime.find(key).unwrap_or_else(|| panic!("missing {key}"));
            runtime.pointer_down(x, y);
            runtime.pointer_up(x, y);
            runtime.frame(1.0);
        }
        assert_eq!(
            *runtime.state.actions.responses.borrow(),
            [("meeting".into(), chatsvc::Reply::Accept, false)]
        );
    }
}
