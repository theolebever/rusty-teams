//! Calendar and activity screens backed by the same account data as the main application.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};
use rune::*;
use crate::ui::text;
use teams_core::{ActivityItem, ActivitySnapshot};

use crate::types::Actions;
use crate::controls::{Size, Variant};
use crate::ui::{self, UiState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Chat,
    Teams,
    Agents,
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
    pub dial_in: Option<chatsvc::DialIn>,
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
    Reactions,
    MissedCalls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalendarView {
    Day,
    WorkWeek,
    #[default]
    Week,
    Month,
}

impl CalendarView {
    pub fn days(self) -> usize { match self { Self::Day => 1, Self::WorkWeek => 5, Self::Week => 7, Self::Month => 42 } }
    const ALL: [Self; 4] = [Self::Day, Self::WorkWeek, Self::Week, Self::Month];
    /// The shared model's span, which names the view and its previous/next tooltips.
    fn span(self) -> crate::calendar_grid::model::CalendarSpan {
        use crate::calendar_grid::model::CalendarSpan;
        match self { Self::Day => CalendarSpan::Day, Self::WorkWeek => CalendarSpan::WorkWeek, Self::Week => CalendarSpan::Week, Self::Month => CalendarSpan::Month }
    }
    /// What the previous and next buttons move by, named as GPUI's tooltips do.
    fn step_tips(self) -> (&'static str, &'static str) {
        match self {
            Self::Day => ("calendar.view.previous_day", "calendar.view.next_day"),
            Self::Month => ("calendar.view.previous_month", "calendar.view.next_month"),
            Self::WorkWeek | Self::Week => ("calendar.view.previous_week", "calendar.view.next_week"),
        }
    }
    fn start(self, date: NaiveDate) -> NaiveDate {
        match self { Self::Day => date, Self::Month => monday(date.with_day(1).unwrap_or(date)), _ => monday(date) }
    }
}

pub struct ScreenState {
    pub last_chat: Option<String>,
    pub last_channel: Option<String>,
    pub calendar_view: CalendarView,
    pub calendar_date: NaiveDate,
    pub date_picker: bool,
    pub picker_month: NaiveDate,
    pub join_id_open: bool,
    pub meeting_id: TextEdit,
    pub meeting_passcode: TextEdit,
    pub selected_activity: Option<String>,
    pub activity_search: TextEdit,
    pub screen: Screen,
    /// Screens left by `navigate`, newest last; the title bar's back button pops it.
    back: Vec<Screen>,
    /// Screens left by going back, newest last; any other navigation clears it.
    forward: Vec<Screen>,
    pub activity: ActivitySnapshot,
    pub calendar: Vec<CalendarRow>,
    pub agenda: Vec<chatsvc::CalendarEvent>,
    pub week: NaiveDate,
    pub filter: ActivityFilter,
    pub selected_event: Option<String>,
    pub event_menu: Option<String>,
    pub send_response: bool,
    /// The calendar's view menu is open.
    pub span_menu: bool,
    /// The right-click menu of an event: its id and the pointer position.
    pub event_context: Option<(String, f32, f32)>,
}

impl Default for ScreenState {
    fn default() -> Self {
        Self {
            last_chat: None,
            last_channel: None,
            calendar_view: CalendarView::Week,
            calendar_date: Local::now().date_naive(),
            date_picker: false,
            picker_month: Local::now().date_naive(),
            join_id_open: false,
            meeting_id: TextEdit::default(),
            meeting_passcode: TextEdit::default(),
            selected_activity: None,
            activity_search: TextEdit::default(),
            screen: Screen::Chat,
            back: Vec::new(),
            forward: Vec::new(),
            activity: ActivitySnapshot::default(),
            calendar: Vec::new(),
            agenda: Vec::new(),
            week: monday(Local::now().date_naive()),
            filter: ActivityFilter::All,
            selected_event: None,
            event_menu: None,
            send_response: true,
            span_menu: false,
            event_context: None,
        }
    }
}

impl ScreenState {
    /// Whether `go_back` has a screen to return to.
    pub fn can_go_back(&self) -> bool { !self.back.is_empty() }

    /// Whether `go_forward` has a screen to return to.
    pub fn can_go_forward(&self) -> bool { !self.forward.is_empty() }
}

fn monday(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

pub(crate) fn occurs_on(event: &CalendarRow, date: NaiveDate) -> bool {
    let start = event.start.with_timezone(&Local).date_naive();
    let end = (event.end - Duration::nanoseconds(1))
        .with_timezone(&Local)
        .date_naive();
    event.end > event.start && start <= date && date <= end
}

/// A navigation history deeper than this is not useful and would grow for a whole session.
const MAX_SCREEN_HISTORY: usize = 32;

fn remember<A: Actions>(s: &mut UiState<A>, screen: Screen) {
    s.screens.back.push(screen);
    if s.screens.back.len() > MAX_SCREEN_HISTORY {
        s.screens.back.remove(0);
    }
}

/// Opens `screen` and records the one left, so the title bar's back button can return to it.
pub fn navigate<A: Actions>(s: &mut UiState<A>, screen: Screen) {
    if s.screens.screen != screen {
        remember(s, s.screens.screen);
        s.screens.forward.clear();
    }
    enter(s, screen);
}

/// Steps one screen back through `ScreenState::back`; does nothing at the start of the history.
pub fn go_back<A: Actions>(s: &mut UiState<A>) {
    let Some(screen) = s.screens.back.pop() else { return };
    s.screens.forward.push(s.screens.screen);
    enter(s, screen);
}

/// Undoes `go_back`; does nothing when no screen was stepped back from.
pub fn go_forward<A: Actions>(s: &mut UiState<A>) {
    let Some(screen) = s.screens.forward.pop() else { return };
    remember(s, s.screens.screen);
    enter(s, screen);
}

fn enter<A: Actions>(s: &mut UiState<A>, screen: Screen) {
    if s.screens.screen != screen { crate::view_composer::stop_typing(s); }
    s.chatlist.quick_view = None;
    if screen == Screen::Teams && s.selected.as_ref().is_some_and(|id| !s.teams.iter().flat_map(|t| &t.channels).any(|c| &c.id == id)) {
        ui::save_draft(s);
        s.selected = None;
        s.messages.clear();
        s.pinned.clear();
    s.pin_index = 0;
    }
    s.screens.screen = screen;
    if screen == Screen::Calendar {
        reload_calendar(s);
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

pub fn navigate_tab<A: Actions>(s: &mut UiState<A>, screen: Screen, tx: &mut Tx) {
    if let Some(id) = &s.selected {
        if s.channel(id).is_some() { s.screens.last_channel = Some(id.clone()); }
        else { s.screens.last_chat = Some(id.clone()); }
    }
    navigate(s, screen);
    let remembered = match screen {
        Screen::Chat => s.screens.last_chat.clone(),
        Screen::Teams => s.screens.last_channel.clone(),
        _ => return,
    };
    if let Some(id) = remembered.filter(|id| s.selected.as_ref() != Some(id)) { ui::open_chat(s, &id, tx); }
}

/// One step back (`-1`) or forward (`1`): a day, a week, or a month, by the current view.
fn shift_view<A: Actions>(s: &mut UiState<A>, direction: i64) {
    let date = s.screens.calendar_date;
    let next = match s.screens.calendar_view {
        CalendarView::Month => {
            let months = chrono::Months::new(1);
            if direction < 0 { date.checked_sub_months(months) } else { date.checked_add_months(months) }.unwrap_or(date)
        }
        CalendarView::Day => date + Duration::days(direction),
        CalendarView::WorkWeek | CalendarView::Week => date + Duration::days(7 * direction),
    };
    select_date(s, next);
}

pub(crate) fn reload_calendar<A: Actions>(s: &UiState<A>) {
    s.actions.load_calendar_range(s.screens.week, s.screens.week + Duration::days(s.screens.calendar_view.days() as i64));
}

pub(crate) fn select_date<A: Actions>(s: &mut UiState<A>, date: NaiveDate) {
    s.screens.calendar_date = date;
    s.screens.week = s.screens.calendar_view.start(date);
    s.screens.calendar.clear();
    s.screens.selected_event = None;
    s.screens.date_picker = false;
    s.screens.event_context = None;
    s.screens.picker_month = date.with_day(1).unwrap_or(date);
    reload_calendar(s);
}

pub(crate) use crate::settings::color;

pub(crate) use crate::ui::ts as style;

pub(crate) fn button<A: Actions + 'static>(
    label: &str,
    key: &str,
    action: impl Fn(&mut UiState<A>, &mut Tx) + 'static,
) -> El {
    // Compatibility wrapper for the hundred-plus call sites that still use this helper: the
    // shared primitive supplies the palette fill, accessible name, Tab stop and focus ring that
    // the local row used to omit. Callers that need a different weight use `controls::button`
    // directly.
    crate::controls::button(key, label, crate::controls::Variant::Secondary, crate::controls::Size::Md, action)
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    match s.screens.screen {
        Screen::Activity => activity(s),
        Screen::Calendar => calendar(s),
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

/// A header button: an optional leading icon (`"#"` is drawn as text, the icon set has no hash) and a label.
fn header_action<A: Actions + 'static>(
    key: &str,
    glyph: Option<&str>,
    label: &str,
    primary: bool,
    action: impl Fn(&mut UiState<A>, &mut Tx) + 'static,
) -> El {
    let palette = app_backend::schemes::active_palette();
    let rgb = crate::avatar::rgb;
    let (fill, hover, ink) = if primary {
        (palette.accent, palette.accent_strong, rgb(palette.on_accent))
    } else {
        (palette.surface_raised, palette.row_hover, rgb(palette.foreground))
    };
    let icon = glyph.map(|glyph| if glyph == "#" {
        text("#").font(style(13.0, 600.0)).color(ink)
    } else {
        crate::icons::sym(glyph, 16.0).color(ink)
    });
    row().key(key.to_owned()).h(32.0).px(12.0).gap(6.0).center().radius(6.0)
        .fill(rgb(fill)).hover(rgb(hover))
        .role("button").label(label.to_owned()).focusable(true)
        .focus_ring(2.0, 1.0, rgb(palette.accent))
        .on_press_tx(action)
        .children(icon)
        .child(text(label.to_owned()).font(style(13.0, 600.0)).color(ink))
}

/// The left pane: the title and a month to jump by. GPUI also lists the connected calendars
/// there; Rune has no calendar list, so it has neither that nor "Add calendar".
fn calendar_pane<A: Actions + 'static>(s: &UiState<A>) -> El {
    col().key("calendar-pane").w(240.0).h_full().min_h(0.0).padding(12.0).gap(8.0)
        .child(text("Calendar").font(style(18.0, 600.0)).color(color("#242424")))
        .child(crate::calendar_grid::month_grid(s, "mini"))
}

fn calendar<A: Actions + 'static>(s: &UiState<A>) -> El {
    let view = s.screens.calendar_view;
    let (previous_tip, next_tip) = view.step_tips();
    let (previous_tip, next_tip) = (app_backend::ui_locale::tr(previous_tip), app_backend::ui_locale::tr(next_tip));
    let mut header = row()
        .w_full()
        .min_h(52.0)
        .wrap()
        .py(8.0)
        .px(16.0)
        .gap(8.0)
        .center_y()
        .child(header_action::<A>("calendar-today", Some("calendar_month"), "Today", false, |s, _| {
            select_date(s, Local::now().date_naive());
        }))
        .child(crate::controls::icon_button::<A>("calendar-previous", "keyboard_arrow_left", &previous_tip, false, |s, _| shift_view(s, -1)))
        .child(crate::controls::icon_button::<A>("calendar-next", "keyboard_arrow_right", &next_tip, false, |s, _| shift_view(s, 1)))
        .child(crate::controls::button::<A>("calendar-date-picker", &format!("{} ▾", s.screens.calendar_date.format("%B %Y")), Variant::Menu, Size::Md, |s, _| {
            s.screens.picker_month = s.screens.calendar_date.with_day(1).unwrap_or(s.screens.calendar_date);
            s.screens.date_picker = !s.screens.date_picker;
        }))
        // Pushes the view menu and the actions to the right edge.
        .child(el().grow(1.0))
        // One menu for the views, as GPUI's `calendar-span` dropdown.
        .child(crate::controls::button::<A>("calendar-span", &format!("{} ▾", view.span().label()), Variant::Menu, Size::Md, |s, _| {
            s.screens.span_menu = !s.screens.span_menu;
        }));
    if !s.read_only {
        header = header
            .child(header_action::<A>("calendar-join-id", Some("#"), "Join with an ID", false, |s, _| s.screens.join_id_open = true))
            .child(header_action::<A>("calendar-meet-now", Some("videocam"), "Meet now", false, |s, _| s.actions.meet_now("")))
            .child(header_action::<A>("calendar-new-meeting", Some("add"), "New meeting", true, |s, tx| { crate::meeting_form::open(s); tx.focus("meeting-title", false); }));
    }
    let body = row()
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .child(calendar_pane(s))
        .child(if s.screens.calendar_view == CalendarView::Month { crate::calendar_grid::month(s) } else { crate::calendar_grid::week(s) });
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

fn join_event<A: Actions>(s: &mut UiState<A>, url: &str) {
    s.screens.event_menu = None;
    s.screens.event_context = None;
    s.calls.link = TextEdit::new(url);
    s.calls.prejoin_open = true;
    s.screens.screen = Screen::Calls;
}

fn open_event_chat<A: Actions>(s: &mut UiState<A>, thread: &str, tx: &mut Tx) {
    s.screens.event_menu = None;
    s.screens.event_context = None;
    s.screens.screen = Screen::Chat;
    ui::open_chat(s, thread, tx);
}

/// GPUI offers a response only on a live event somebody else organises.
fn can_respond<A: Actions>(s: &UiState<A>, event: &CalendarRow) -> bool {
    !s.read_only && !event.cancelled && event.response != chatsvc::ResponseType::Organizer && event.organizer.is_some()
}

const RESPONSES: [(&str, chatsvc::Reply, chatsvc::ResponseType); 3] = [
    ("Accept", chatsvc::Reply::Accept, chatsvc::ResponseType::Accepted),
    ("Tentative", chatsvc::Reply::Tentative, chatsvc::ResponseType::Tentative),
    ("Decline", chatsvc::Reply::Decline, chatsvc::ResponseType::Declined),
];

/// The right-click menu of an event, as GPUI's `event_menu`: Join, Copy join info, Chat with
/// participants, then the responses and the "Send a response" switch.
fn event_context_menu<A: Actions + 'static>(s: &UiState<A>, event: &CalendarRow) -> El {
    let palette = app_backend::schemes::active_palette();
    let rgb = crate::avatar::rgb;
    use crate::controls::menu_item as item;
    let mut menu = col().key("calendar-event-context").w(240.0).padding(6.0).gap(2.0).radius(8.0)
        .fill(rgb(palette.popup)).border(1.0, rgb(palette.border));
    if let Some(url) = &event.join_url {
        if !s.read_only && !event.cancelled {
            let url = url.clone();
            menu = menu.child(item("calendar-context-join", "Join", Some("videocam"), false, move |s: &mut UiState<A>| join_event(s, &url)));
        }
        let info = chatsvc::join_info_text(url, event.dial_in.as_ref());
        menu = menu.child(item("calendar-context-copy-info", "Copy join info", Some("link"), false, |_: &mut UiState<A>| {})
            .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| {
                s.screens.event_context = None;
                tx.copy(info.clone());
                s.toast.show("Join info copied");
            }));
    }
    if let Some(thread) = &event.thread_id {
        let thread = thread.clone();
        menu = menu.child(item("calendar-context-chat", "Chat with participants", Some("chat_bubble"), false, |_: &mut UiState<A>| {})
            .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| open_event_chat(s, &thread, tx)));
    }
    if can_respond(s, event) {
        if event.join_url.is_some() || event.thread_id.is_some() {
            menu = menu.child(el().w_full().h(1.0).fill(rgb(palette.border)));
        }
        for (label, reply, response) in RESPONSES {
            let (id, uid) = (event.id.clone(), event.ical_uid.clone());
            menu = menu.child(item(&format!("calendar-context-{label}"), label, (event.response == response).then_some("check"), event.response == response, move |s: &mut UiState<A>| {
                s.screens.event_context = None;
                s.actions.respond_event(&id, uid.as_deref(), reply, s.screens.send_response);
            }));
        }
        menu = menu.child(el().w_full().h(1.0).fill(rgb(palette.border)))
            .child(item("calendar-context-send-response", "Send a response", s.screens.send_response.then_some("check"), s.screens.send_response, |s: &mut UiState<A>| {
                s.screens.send_response = !s.screens.send_response;
            }));
    }
    menu
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
        details = details.child(button::<A>("Join", "calendar-join", move |s, _| join_event(s, &url)));
    }
    if let Some(url) = &event.join_url {
        let info = chatsvc::join_info_text(url, event.dial_in.as_ref());
        details = details.child(button::<A>("Copy join info", "calendar-copy-info", move |s, tx| {
            tx.copy(info.clone());
            s.toast.show("Join info copied");
        }));
    }
    if let Some(thread) = &event.thread_id {
        let thread = thread.clone();
        details = details.child(button::<A>("Chat with participants", "calendar-chat", move |s, tx| open_event_chat(s, &thread, tx)));
    }
    if can_respond(s, event) {
        let send_response = s.screens.send_response;
        details = details.child(crate::controls::button::<A>(
            "calendar-send-response",
            "Send a response",
            if send_response { crate::controls::Variant::Primary } else { crate::controls::Variant::Secondary },
            crate::controls::Size::Md,
            |s, _| s.screens.send_response = !s.screens.send_response,
        ));
        let mut choices = row().gap(8.0);
        for (label, reply, response) in RESPONSES {
            let id = event.id.clone();
            let uid = event.ical_uid.clone();
            let key = format!("calendar-rsvp-{label}");
            choices = choices.child(crate::controls::radio_item::<A>(
                &key,
                label,
                event.response == response,
                move |s| {
                    s.actions
                        .respond_event(&id, uid.as_deref(), reply, s.screens.send_response);
                },
            ));
        }
        details = details.child(choices);
    }
    details = details.child(button::<A>("Close", "calendar-close", |s, _| {
        s.screens.selected_event = None;
        s.screens.event_menu = None;
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
        ActivityFilter::Reactions => matches!(item.activity_type.as_str(), "reaction" | "reactionInChat" | "like" | "likeInChat" | "reactionInCommunity" | "reactionInSubCommunity"),
        ActivityFilter::MissedCalls => item.activity_type == "call" && item.activity_subtype == "missedCall",
        ActivityFilter::Mentions => crate::quick_views::is_mention(&item.activity_type),
    }
}

fn activity<A: Actions + 'static>(s: &UiState<A>) -> El {
    use crate::avatar::rgb;
    use crate::icons::sym;
    let palette = app_backend::schemes::active_palette();
    let mut header = row().w_full().h(56.0).px(20.0).gap(8.0).center_y().child(
        text("Activity")
            .font(style(18.0, 600.0))
            .color(rgb(palette.foreground)).grow(1.0),
    );
    if !s.read_only {
        header = header.child(row().key("activity-read-all").size(32.0, 32.0).center().radius(4.0)
            .label("Mark all as read").hover(rgb(palette.row_hover))
            .on_press(|s: &mut UiState<A>| s.actions.mark_all_activity_read())
            .child(sym("done_all", 20.0).color(rgb(palette.text_muted))));
    }
    let mut filters = row().w_full().px(16.0).gap(6.0).pb(12.0).wrap();
    for (label, filter) in [
        ("All", ActivityFilter::All),
        ("Unread", ActivityFilter::Unread),
        ("@Mentions", ActivityFilter::Mentions),
        ("Replies", ActivityFilter::Replies),
        ("Reactions", ActivityFilter::Reactions),
        ("Missed calls", ActivityFilter::MissedCalls),
    ] {
        let selected = s.screens.filter == filter;
        filters = filters.child(row().key(format!("activity-filter-{label}")).h(28.0).min_w(40.0).px(12.0).center().radius(14.0)
            .fill(rgb(if selected { palette.accent } else { palette.surface_raised }))
            .hover(rgb(if selected { palette.accent_strong } else { palette.row_hover })).label(label).checked(selected)
            .on_press(move |s: &mut UiState<A>| s.screens.filter = filter)
            .child(text(label).font(style(12.0, if selected { 600.0 } else { 400.0 }))
                .color(rgb(if selected { palette.on_accent } else { palette.foreground }))));
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
        if unread && !s.read_only {
            let read_id = id.clone();
            notification = notification.child(row().key(format!("activity-read-{id}")).size(28.0, 28.0).center().radius(4.0)
                .label("Mark as read").help("Mark as read").hover(rgb(palette.row_hover))
                .on_press(move |s: &mut UiState<A>| s.actions.mark_activity_read(&read_id))
                .child(sym("done_all", 16.0).color(rgb(palette.text_muted))));
        }
        // Without a conversation (a meeting invite), open the event's day, else the item's Teams link.
        let event_day = item.event_start_ms.and_then(DateTime::from_timestamp_millis).map(|at| at.with_timezone(&Local).date_naive());
        let tap_url = item.tap_url.clone().filter(|url| crate::rich_text::is_openable(url));
        if !conversation.is_empty() || event_day.is_some() || tap_url.is_some() {
            let root = item.source_reply_chain_id.clone();
            notification =
                notification.hover(rgb(palette.row_hover))
                    .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| {
                        if !s.read_only && unread {
                            s.actions.mark_activity_read(&id);
                        }
                        s.screens.selected_activity = Some(id.clone());
                        if !conversation.is_empty() {
                            ui::open_chat(s, &conversation, tx);
                            if ui::active_target(s).is_some_and(|target| target.channel) && root.is_some() { ui::select_thread(s, root.clone()); }
                        } else if let Some(day) = event_day {
                            s.screens.calendar_date = day;
                            navigate(s, Screen::Calendar);
                        } else if let Some(url) = &tap_url {
                            crate::rich_text::open(s, tx, url);
                        }
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
        kind if crate::quick_views::is_mention(kind) => (format!("{actor} mentioned you"), "alternate_email"),
        "reply" | "replyToReply" | "replyInChat" | "communityReply" | "communityReplyToReply" => (format!("{actor} replied"), "reply"),
        "reaction" | "reactionInChat" | "like" | "likeInChat" | "reactionInCommunity" | "reactionInSubCommunity" => (format!("{actor} reacted to your message"), "thumb_up"),
        "call" if item.activity_subtype == "missedCall" => (format!("Missed call from {actor}"), "call"),
        "call" | "voicemail" | "callOrMeetup" => (actor.to_owned(), "call"),
        "teamMembershipChange" => (format!("{actor} updated your team membership"), "group"),
        _ => (actor.to_owned(), "notifications"),
    }
}

pub fn overlays<A: Actions + 'static>(s: &UiState<A>) -> Vec<El> {
    let mut layers = Vec::new();
    if s.screens.screen == Screen::Calendar
        && let Some(event) = s.screens.calendar.iter().find(|event| Some(&event.id) == s.screens.event_menu.as_ref()) {
            let card = col()
                .w(420.0)
                .max_h(560.0)
                .radius(10.0)
                .fill(color("#FFFFFF"))
                .border(1.0, color("#E0E0E0"))
                .child(el().w_full().h(4.0).fill(color("#7B61FF")))
                .child(calendar_details(s, event));
            layers.push(
                el().key("calendar-event-menu-host")
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h_full()
                    .items_center()
                    .pt(72.0)
                    .modal(true, |s: &mut UiState<A>| s.screens.event_menu = None)
                    .child(card),
            );
        }
    if s.screens.screen == Screen::Calendar
        && let Some((id, x, y)) = &s.screens.event_context
        && let Some(event) = s.screens.calendar.iter().find(|event| &event.id == id) {
            let close = |s: &mut UiState<A>| s.screens.event_context = None;
            layers.push(el().key("calendar-event-context-layer").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .child(el().key("calendar-event-context-backdrop").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).on_press(close))
                .child(el().key("calendar-event-context-at").size(1.0, 1.0).pin(Some(*x), Some(*y), None, None))
                .child(event_context_menu(s, event)
                    .anchored("calendar-event-context-at", Side::Bottom, Align::Start, 4.0)));
        }
    if s.screens.span_menu && s.screens.screen == Screen::Calendar {
        let palette = app_backend::schemes::active_palette();
        let mut menu = col().key("calendar-span-menu").w(180.0).padding(6.0).gap(2.0).radius(8.0)
            .fill(crate::avatar::rgb(palette.popup)).border(1.0, crate::avatar::rgb(palette.border))
            .anchored("calendar-span", Side::Bottom, Align::End, 4.0)
            .modal(true, |s: &mut UiState<A>| s.screens.span_menu = false);
        for choice in CalendarView::ALL {
            let selected = s.screens.calendar_view == choice;
            menu = menu.child(crate::controls::menu_item::<A>(
                &format!("calendar-view-{choice:?}"),
                &choice.span().label(),
                selected.then_some("check"),
                selected,
                move |s| {
                    s.screens.span_menu = false;
                    s.screens.calendar_view = choice;
                    select_date(s, s.screens.calendar_date);
                },
            ));
        }
        layers.push(menu);
    }
    if s.screens.date_picker && s.screens.screen == Screen::Calendar { layers.push(crate::calendar_grid::date_picker(s)); }
    if s.screens.join_id_open {
        let input = |edit: &TextEdit, key: &str, label: &str| text_input(edit, style(14.0, 400.0)).key(key.to_owned()).w_full().label(label.to_owned())
            .placeholder(label.to_owned(), color("#616161")).color(color("#242424"));
        let dialog = col().key("join-id-dialog").w(380.0).padding(20.0).gap(14.0).radius(8.0).fill(color("#FFFFFF"))
            .role("dialog").label("Join with an ID").modal(true, |s: &mut UiState<A>| s.screens.join_id_open = false)
            .child(text("Join with an ID").font(style(18.0, 600.0)).color(color("#242424")))
            .child(input(&s.screens.meeting_id, "meeting-id", "Meeting ID").on_edit(|s: &mut UiState<A>| &mut s.screens.meeting_id))
            .child(input(&s.screens.meeting_passcode, "meeting-passcode", "Passcode").on_edit(|s: &mut UiState<A>| &mut s.screens.meeting_passcode))
            .child(row().gap(8.0).justify_end()
                .child(button("Cancel", "join-id-cancel", |s: &mut UiState<A>, _| s.screens.join_id_open = false))
                .child(button("Join meeting", "join-id-confirm", |s: &mut UiState<A>, _| {
                    let Some(code) = teams_core::meeting_code_from_id(&s.screens.meeting_id.text, &s.screens.meeting_passcode.text) else {
                        s.toast.show("Enter a valid meeting ID"); return;
                    };
                    s.calls.link = TextEdit::new(code.url);
                    s.calls.prejoin_open = true;
                    s.screens.screen = Screen::Calls;
                    s.screens.join_id_open = false;
                    s.screens.meeting_passcode.clear();
                })));
        layers.push(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().fill(Color::BLACK.alpha(0.2)).child(dialog));
    }
    layers.extend(crate::reminders::view(s));
    layers
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn back_and_forward_walk_the_screens_visited() {
        let mut state = UiState::new(Fake::default());
        navigate(&mut state, Screen::Teams);
        navigate(&mut state, Screen::Settings);
        go_back(&mut state);
        assert_eq!(state.screens.screen, Screen::Teams);
        go_back(&mut state);
        assert_eq!(state.screens.screen, Screen::Chat);
        go_back(&mut state);
        assert_eq!(state.screens.screen, Screen::Chat, "nothing before the first screen");
        go_forward(&mut state);
        assert_eq!(state.screens.screen, Screen::Teams);
        navigate(&mut state, Screen::Calls);
        assert!(!state.screens.can_go_forward(), "a new visit drops the forward history");
    }

    /// The shared button used at over a hundred call sites must carry an accessible name,
    /// a button role and a keyboard Tab stop, and stay on the Fluent control radius.
    #[test]
    fn calendar_header_uses_named_icon_buttons_and_accent_selected_view() {
        let mut state = UiState::new(Fake::default());
        state.screens.screen = Screen::Calendar;
        let mut rt = Runtime::new(state, crate::ui::view::<Fake>, (1100.0, 800.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        let elements = rt.elements();
        for (key, label) in [("calendar-previous", "Go to previous week"), ("calendar-next", "Go to next week")] {
            let found = elements.iter().find(|element| element.key.as_deref() == Some(key))
                .unwrap_or_else(|| panic!("missing {key}"));
            assert_eq!(found.label.as_deref(), Some(label));
            assert!(!elements.iter().any(|element| element.text.as_deref() == Some("‹") || element.text.as_deref() == Some("›")));
        }
    }

    fn calendar_runtime() -> Runtime<UiState<Fake>> {
        let mut state = UiState::new(Fake::default());
        state.screens.screen = Screen::Calendar;
        let mut rt = Runtime::new(state, crate::ui::view::<Fake>, (1100.0, 800.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        rt
    }

    fn press(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn calendar_header_has_gpui_order_and_one_view_menu() {
        let mut rt = calendar_runtime();
        let elements = rt.elements();
        let x = |key: &str| elements.iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("missing {key}")).rect[0];
        let order = ["calendar-today", "calendar-previous", "calendar-next", "calendar-date-picker", "calendar-span", "calendar-join-id", "calendar-meet-now", "calendar-new-meeting"];
        for pair in order.windows(2) {
            assert!(x(pair[0]) < x(pair[1]), "{} must come before {}", pair[0], pair[1]);
        }
        for gone in ["calendar-agenda", "calendar-views", "calendar-view-Day", "calendar-view-Month"] {
            assert!(rt.find(gone).is_none(), "{gone} is not in GPUI's header");
        }
        press(&mut rt, "calendar-span");
        let labels: Vec<_> = ["Day", "Work week", "Week", "Month"].into_iter()
            .zip(["calendar-view-Day", "calendar-view-WorkWeek", "calendar-view-Week", "calendar-view-Month"])
            .map(|(label, key)| (label, rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("missing {key}")).label))
            .collect();
        for (label, found) in labels { assert_eq!(found.as_deref(), Some(label)); }
        press(&mut rt, "calendar-view-Month");
        assert_eq!(rt.state.screens.calendar_view, CalendarView::Month);
        assert!(!rt.state.screens.span_menu);
    }

    #[test]
    fn previous_and_next_move_by_the_view_and_say_so() {
        let mut rt = calendar_runtime();
        let date = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
        for (view, tips, next) in [
            (CalendarView::Day, ("Go to previous day", "Go to next day"), NaiveDate::from_ymd_opt(2026, 11, 19).unwrap()),
            (CalendarView::WorkWeek, ("Go to previous week", "Go to next week"), NaiveDate::from_ymd_opt(2026, 11, 25).unwrap()),
            (CalendarView::Week, ("Go to previous week", "Go to next week"), NaiveDate::from_ymd_opt(2026, 11, 25).unwrap()),
            (CalendarView::Month, ("Go to previous month", "Go to next month"), NaiveDate::from_ymd_opt(2026, 12, 18).unwrap()),
        ] {
            rt.update(move |s, _| { s.screens.calendar_view = view; select_date(s, date); });
            rt.frame(1.0 / 60.0);
            let labels: Vec<_> = ["calendar-previous", "calendar-next"].into_iter()
                .map(|key| rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).unwrap().label.unwrap()).collect();
            assert_eq!(labels, [tips.0, tips.1], "{view:?}");
            press(&mut rt, "calendar-next");
            assert_eq!(rt.state.screens.calendar_date, next, "{view:?}");
            press(&mut rt, "calendar-previous");
            assert_eq!(rt.state.screens.calendar_date, date, "{view:?}");
        }
    }

    #[test]
    fn the_left_pane_has_a_mini_month_that_jumps_to_a_day() {
        let mut rt = calendar_runtime();
        let date = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
        rt.update(move |s, _| select_date(s, date));
        rt.frame(1.0 / 60.0);
        assert!(rt.find("calendar-pane").is_some());
        press(&mut rt, "mini-date-2026-11-20");
        assert_eq!(rt.state.screens.calendar_date, NaiveDate::from_ymd_opt(2026, 11, 20).unwrap());
    }

    #[test]
    fn right_click_opens_the_event_menu_with_gpui_labels() {
        use chrono::TimeZone;
        let date = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let start = Local.from_local_datetime(&date.and_hms_opt(9, 0, 0).unwrap()).single().unwrap().with_timezone(&Utc);
        let mut rt = calendar_runtime();
        rt.update(move |s, _| {
            s.screens.week = date;
            s.screens.calendar = vec![CalendarRow {
                id: "meeting".into(), subject: "Meeting".into(), start, end: start + Duration::hours(1),
                cancelled: false, all_day: false, organizer: Some("Ada".into()), ical_uid: None, dial_in: None,
                join_url: Some("https://teams.microsoft.com/l/meetup-join/x".into()), thread_id: Some("chat".into()),
                response: chatsvc::ResponseType::NotResponded,
            }];
        });
        rt.frame(1.0);
        let (x, y) = rt.find("calendar-meeting-2026-11-02").expect("event");
        rt.secondary_down(x, y);
        rt.secondary_up(x, y);
        rt.frame(1.0 / 60.0);
        assert!(rt.find("calendar-event-menu-host").is_none(), "right-click is a menu, not the details dialog");
        for key in ["calendar-context-join", "calendar-context-copy-info", "calendar-context-chat", "calendar-context-Accept", "calendar-context-Tentative", "calendar-context-Decline", "calendar-context-send-response"] {
            assert!(rt.find(key).is_some(), "missing {key}");
        }
        let chat = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("calendar-context-chat")).unwrap();
        assert_eq!(chat.label.as_deref(), Some("Chat with participants"));
        press(&mut rt, "calendar-context-Decline");
        assert_eq!(*rt.state.actions.responses.borrow(), [("meeting".into(), chatsvc::Reply::Decline, true)]);
        assert!(rt.state.screens.event_context.is_none());
    }

    #[test]
    fn rsvp_choices_use_checked_state_instead_of_a_text_tick() {
        fn details_view(s: &UiState<Fake>, _: &mut Cx<UiState<Fake>>) -> El {
            let event = s.screens.calendar.first().expect("event").clone();
            calendar_details(s, &event)
        }
        let mut state = UiState::new(Fake::default());
        state.screens.screen = Screen::Calendar;
        state.screens.send_response = true;
        state.apply(crate::types::Update::Calendar {
            week: state.screens.week,
            rows: vec![CalendarRow {
                id: "event".into(),
                subject: "Review".into(),
                start: Utc::now(),
                end: Utc::now(),
                cancelled: false,
                all_day: false,
                organizer: Some("Ada".into()),
                ical_uid: None,
                dial_in: None,
                join_url: None,
                thread_id: Some("chat".into()),
                response: chatsvc::ResponseType::Accepted,
            }],
        });
        let mut rt = Runtime::new(state, details_view, (800.0, 600.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        let elements = rt.elements();
        assert!(
            elements.iter().all(|element| element.text.as_deref().is_none_or(|text| !text.contains('✓'))),
            "a text tick must not encode a selected state"
        );
        let accept = elements.iter().find(|element| element.key.as_deref() == Some("calendar-rsvp-Accept")).expect("accept choice");
        assert_eq!(accept.role, "radio");
    }

    #[test]
    fn shared_button_exposes_a11y_and_control_radius() {
        fn view(_: &UiState<Fake>, _: &mut Cx<UiState<Fake>>) -> El {
            button::<Fake>("Save", "shared-save", |_, _| {})
        }
        let mut rt = Runtime::new(UiState::new(Fake::default()), view, (400.0, 200.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        let found = rt.elements().into_iter()
            .find(|element| element.key.as_deref() == Some("shared-save"))
            .expect("shared button");
        assert_eq!(found.role, "button");
        assert_eq!(found.label.as_deref(), Some("Save"));
        assert!(found.focusable, "shared button must be reachable with Tab");
        assert!((found.radius - 6.0).abs() < 0.01, "control radius must be 6");
    }

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
            ical_uid: None, dial_in: None,
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
            organizer: Some("Ada".into()), ical_uid: None, dial_in: None,
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
            organizer: Some("Ada".into()),
            ical_uid: None, dial_in: None,
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
