//! Week layout uses the GPUI calendar's collision and multi-day event rules.
use crate::screens::{CalendarRow, color, style};
use crate::types::Actions;
use crate::ui::UiState;
use chrono::{Local, Offset};
use rune::*;
use crate::ui::text;

#[allow(dead_code)]
mod model {
    mod app_base {
        pub mod format {
            pub fn calendar_range_label(
                start: chrono::NaiveDate,
                end: chrono::NaiveDate,
            ) -> String {
                use chrono::Datelike;
                if start == end {
                    end.format("%B %-d, %Y").to_string()
                } else if start.year() != end.year() {
                    format!(
                        "{} - {}",
                        start.format("%B %-d, %Y"),
                        end.format("%B %-d, %Y")
                    )
                } else if start.month() != end.month() {
                    format!("{} - {}", start.format("%B %-d"), end.format("%B %-d, %Y"))
                } else {
                    format!("{}-{}, {}", start.format("%B %-d"), end.day(), end.year())
                }
            }
        }
    }
    include!("../../app-call-model/src/week_grid.rs");
}

const HOUR: f32 = 48.0;

fn events(rows: &[CalendarRow]) -> Vec<chatsvc::CalendarEvent> {
    rows.iter()
        .map(|e| chatsvc::CalendarEvent {
            object_id: e.id.clone(),
            subject: e.subject.clone(),
            start: e.start,
            end: e.end,
            all_day: e.all_day,
            cancelled: e.cancelled,
            online: e.join_url.is_some(),
            join_url: e.join_url.clone(),
            thread_id: e.thread_id.clone(),
            organizer_name: e.organizer.clone(),
            organizer_address: None,
            ical_uid: e.ical_uid.clone(),
            show_as: Default::default(),
            dial_in: None,
            response: e.response,
        })
        .collect()
}

fn event<A: Actions + 'static>(s: &UiState<A>, e: &CalendarRow, date: chrono::NaiveDate) -> El {
    let selected = format!("{}-{date}", e.id);
    let title = if e.cancelled {
        format!("Canceled: {}", e.subject)
    } else {
        e.subject.clone()
    };
    let active = s.screens.selected_event.as_ref() == Some(&selected);
    col()
        .key(format!("calendar-{}-{date}", e.id))
        .w_full()
        .min_w(0.0)
        .padding(4.0)
        .radius(4.0)
        .fill(color(if active { "#DADDF5" } else { "#E8EBFA" }))
        .hover(color("#DADDF5"))
        .clip_soft(0.0)
        .on_press(move |s: &mut UiState<A>| s.screens.selected_event = Some(selected.clone()))
        .child(
            text(title)
                .font(style(12.0, 600.0))
                .color(color("#242424"))
                .wrap(),
        )
        .children((!e.all_day).then(|| {
            text(format!(
                "{}–{}",
                e.start.with_timezone(&Local).format("%H:%M"),
                e.end.with_timezone(&Local).format("%H:%M")
            ))
            .font(style(11.0, 400.0))
            .color(color("#616161"))
        }))
}

pub fn week<A: Actions + 'static>(s: &UiState<A>) -> El {
    let source = events(&s.screens.calendar);
    let days = model::week_days(&source, s.screens.week, 7, Local::now().offset().fix());
    let mut headings = row().grow(1.0).min_w(0.0).h_full().equal_columns(7);
    let all_day_height = 26.0
        * days
            .iter()
            .map(|d| d.all_day.len())
            .max()
            .unwrap_or(0)
            .max(1) as f32;
    let mut all_day = row().grow(1.0).min_w(0.0).h_full().equal_columns(7);
    let mut hours = col().w(52.0).h(24.0 * HOUR);
    for hour in 0..24 {
        hours = hours.child(
            text(format!("{hour:02}:00"))
                .key(format!("calendar-hour-{hour}"))
                .h(HOUR)
                .font(style(10.0, 400.0))
                .color(color("#616161")),
        );
    }
    let mut columns = row().w_full().h(24.0 * HOUR).child(hours);
    for day in &days {
        let today = day.date == Local::now().date_naive();
        headings = headings.child(
            col()
                .key(format!("calendar-heading-{}", day.date))
                .min_w(0.0)
                .h_full()
                .padding(6.0)
                .child(
                    text(day.date.format("%a %d").to_string())
                        .font(style(13.0, 600.0))
                        .color(color(if today { "#5B5FC7" } else { "#242424" })),
                ),
        );
        let mut all_day_column = col().grow(1.0).min_w(0.0).h_full().gap(2.0).px(2.0);
        for &index in &day.all_day {
            all_day_column =
                all_day_column.child(event(s, &s.screens.calendar[index], day.date).h(24.0));
        }
        all_day = all_day.child(all_day_column);
        let mut background = col().w_full().h(24.0 * HOUR);
        for _ in 0..24 {
            background = background.child(el().w_full().h(HOUR).border(0.5, color("#E0E0E0")));
        }
        let mut day_column = el()
            .key(format!("calendar-day-{}", day.date))
            .grow(1.0)
            .min_w(0.0)
            .h_full()
            .child(background);
        for block in &day.blocks {
            let height = (block.end_minute - block.start_minute) as f32 * HOUR / 60.0;
            day_column = day_column.child(
                row()
                    .w_full()
                    .h(height)
                    .absolute(0.0, block.start_minute as f32 * HOUR / 60.0)
                    .child(el().w_frac(block.lane as f32 / block.lanes as f32).h_full())
                    .child(
                        event(s, &s.screens.calendar[block.event_index], day.date)
                            .w_frac(1.0 / block.lanes as f32)
                            .h_full(),
                    ),
            );
        }
        columns = columns.child(day_column);
    }
    col()
        .key("calendar-week")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .child(row().w_full().h(48.0).child(el().w(52.0)).child(headings))
        .child(
            row()
                .w_full()
                .h(all_day_height)
                .child(
                    text("All day")
                        .w(52.0)
                        .font(style(10.0, 400.0))
                        .color(color("#616161")),
                )
                .child(all_day),
        )
        .child(
            col()
                .key("calendar-hours")
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(columns),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};

    struct Quiet;
    impl Actions for Quiet {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
    }

    #[test]
    fn simultaneous_meetings_have_separate_lanes_in_the_same_day() {
        let date = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let start = Local
            .from_local_datetime(&date.and_hms_opt(9, 0, 0).unwrap())
            .single()
            .unwrap()
            .with_timezone(&chrono::Utc);
        let mut state = UiState::new(Quiet);
        state.screens.screen = crate::screens::Screen::Calendar;
        state.screens.week = date;
        state.screens.calendar = ["a", "b"]
            .into_iter()
            .map(|id| CalendarRow {
                id: id.into(),
                subject: id.into(),
                start,
                end: start + chrono::Duration::hours(1),
                cancelled: false,
                all_day: false,
                organizer: None,
                ical_uid: None,
                join_url: None,
                thread_id: None,
                response: chatsvc::ResponseType::Accepted,
            })
            .collect();
        let mut rt = Runtime::new(
            state,
            crate::ui::view::<Quiet>,
            (1280.0, 800.0),
            Env {
                system_dark: false,
                scale: 1.0,
            },
        );
        rt.frame(1.0);
        let elements = rt.elements();
        let rect = |key: &str| {
            elements
                .iter()
                .find(|e| e.path.ends_with(&format!("/{key}")))
                .unwrap()
                .rect
        };
        let a = rect("calendar-a-2026-11-02");
        let b = rect("calendar-b-2026-11-02");
        assert_eq!(a[1], b[1]);
        assert_eq!(a[3], HOUR);
        assert!(
            a[0] + a[2] <= b[0],
            "overlapping meeting boxes: {a:?}, {b:?}"
        );
        let heading = rect("calendar-heading-2026-11-08");
        let day = rect("calendar-day-2026-11-08");
        assert!(
            (heading[0] - day[0]).abs() < 1.0,
            "day headings must align with the grid: {heading:?}, {day:?}"
        );
    }
}
