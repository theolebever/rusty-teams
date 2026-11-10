//! Agenda: meetings in the next seven days, grouped by day, with Join into the call screen.
//! The grouping and row rules come from app-call-model's agenda model. That crate depends on
//! GPUI, so its source is compiled here with `include!`. Its one GPUI-side call,
//! `app_base::format::clock_format`, is stubbed with the 24-hour clock the other screens use.

#[allow(dead_code)]
mod model {
    mod app_base {
        pub mod format {
            pub fn clock_format() -> &'static str {
                "%H:%M"
            }
        }
    }

    include!("../../app-call-model/src/agenda.rs");
}

use chrono::{DateTime, Local, Offset, Utc};
use rune::*;

use crate::screens::{Screen, button, color, style};
use crate::types::Actions;
use crate::ui::UiState;

/// The agenda's fetch window: seven days from local midnight, as the GPUI app fetches it.
pub fn window(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    model::fetch_window(now, Local::now().offset().fix())
}

fn grouped(events: &[chatsvc::CalendarEvent], now: DateTime<Utc>) -> Vec<model::AgendaDay> {
    model::group_by_day(events, now, Local::now().offset().fix())
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    let now = Utc::now();
    let mut rows = col().w_full().px(20.0).gap(8.0);
    let days = grouped(&s.screens.agenda, now);
    if days.is_empty() {
        rows = rows.child(
            text("No meetings in the next seven days.")
                .font(style(13.0, 400.0))
                .color(color("#616161")),
        );
    }
    for day in days {
        rows = rows.child(
            text(day.label)
                .font(style(14.0, 700.0))
                .color(color("#242424")),
        );
        for meeting in &day.rows {
            rows = rows.child(meeting_card(s, meeting, now));
        }
    }
    let header = row()
        .w_full()
        .h(56.0)
        .px(20.0)
        .gap(8.0)
        .center_y()
        .child(
            text("Agenda")
                .font(style(18.0, 700.0))
                .color(color("#242424")),
        )
        .child(button::<A>("Refresh", "agenda-refresh", |s, _| s.actions.load_agenda()))
        .child(button::<A>("Week", "agenda-week", |s, _| {
            crate::screens::navigate(s, Screen::Calendar)
        }));
    col()
        .key("agenda")
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

fn meeting_card<A: Actions + 'static>(
    s: &UiState<A>,
    meeting: &model::AgendaRow,
    now: DateTime<Utc>,
) -> El {
    let when = if meeting.detail.is_empty() {
        meeting.time_label.clone()
    } else {
        format!("{} · {}", meeting.time_label, meeting.detail)
    };
    let mut card = col()
        .key(format!("agenda-{}", meeting.key))
        .w_full()
        .padding(10.0)
        .gap(4.0)
        .radius(6.0)
        .fill(color("#E8EBFA"))
        .child(
            text(when)
                .font(style(12.0, 400.0))
                .color(color("#616161")),
        )
        .child(
            text(meeting.subject.clone())
                .font(style(14.0, 600.0))
                .color(color("#242424"))
                .wrap(),
        );
    // Same gate as the GPUI calendar: only from five minutes before the start until the end.
    let join = meeting
        .join_url
        .clone()
        .filter(|_| !s.read_only && model::join_available(meeting.start, meeting.end, now));
    if let Some(url) = join {
        // The existing call flow: the Calls screen with the link filled in, joined from its prejoin.
        card = card.child(button::<A>(
            "Join",
            &format!("agenda-join-{}", meeting.key),
            move |s, _| {
                s.calls.link = TextEdit::new(&url);
                s.screens.screen = Screen::Calls;
            },
        ));
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SendTarget;
    use chrono::Duration;
    use std::cell::Cell;

    #[derive(Default)]
    struct Fake {
        loads: Cell<u32>,
        read_only: bool,
    }

    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn load_agenda(&self) {
            self.loads.set(self.loads.get() + 1);
        }
        fn read_only(&self) -> bool {
            self.read_only
        }
    }

    fn meeting(id: &str, starts_in: Duration) -> chatsvc::CalendarEvent {
        let start = Utc::now() + starts_in;
        chatsvc::CalendarEvent {
            object_id: id.into(),
            subject: format!("Meeting {id}"),
            start,
            end: start + Duration::minutes(30),
            all_day: false,
            cancelled: false,
            online: true,
            join_url: Some(format!("https://teams.microsoft.com/l/meetup-join/{id}")),
            thread_id: None,
            organizer_name: Some("Ada".into()),
            organizer_address: None,
            ical_uid: None,
            show_as: Default::default(),
            dial_in: None,
            response: chatsvc::ResponseType::Accepted,
        }
    }

    fn runtime(fake: Fake, events: Vec<chatsvc::CalendarEvent>) -> Runtime<UiState<Fake>> {
        let env = Env {
            system_dark: false,
            scale: 1.0,
        };
        let mut rt = Runtime::new(
            UiState::new(fake),
            crate::ui::view::<Fake>,
            (1000.0, 700.0),
            env,
        );
        rt.update(move |s, _| {
            s.screens.screen = Screen::Agenda;
            s.screens.agenda = events;
        });
        rt.frame(0.0);
        rt
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn join_is_offered_only_while_the_meeting_can_be_joined() {
        let events = vec![
            meeting("now", Duration::minutes(-2)),
            meeting("later", Duration::hours(3)),
        ];
        let mut rt = runtime(Fake::default(), events);
        assert!(rt.find("agenda-now").is_some() && rt.find("agenda-later").is_some());
        assert!(rt.find("agenda-join-now").is_some());
        assert!(rt.find("agenda-join-later").is_none());
        click(&mut rt, "agenda-join-now");
        assert_eq!(rt.state.screens.screen, Screen::Calls);
        assert_eq!(
            rt.state.calls.link.text,
            "https://teams.microsoft.com/l/meetup-join/now"
        );
    }

    #[test]
    fn read_only_accounts_see_meetings_without_join() {
        let events = vec![meeting("now", Duration::minutes(-2))];
        let rt = runtime(
            Fake {
                read_only: true,
                ..Default::default()
            },
            events,
        );
        assert!(rt.find("agenda-now").is_some());
        assert!(rt.find("agenda-join-now").is_none());
    }

    #[test]
    fn the_calendar_opens_the_agenda_which_loads_and_returns_to_the_week() {
        let mut rt = runtime(Fake::default(), vec![]);
        rt.update(|s, _| s.screens.screen = Screen::Calendar);
        rt.frame(0.0);
        click(&mut rt, "calendar-agenda");
        assert_eq!(rt.state.screens.screen, Screen::Agenda);
        assert_eq!(rt.state.actions.loads.get(), 1);
        click(&mut rt, "agenda-week");
        assert_eq!(rt.state.screens.screen, Screen::Calendar);
    }
}
