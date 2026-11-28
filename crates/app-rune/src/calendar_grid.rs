//! Week layout uses the GPUI calendar's collision and multi-day event rules.
use crate::screens::{CalendarRow, color, style};
use crate::types::Actions;
use crate::ui::UiState;
use chrono::{Datelike, Duration, Local, Offset};
use rune::*;
use crate::ui::text;

pub(crate) use app_call_model::week_grid as model;

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
            ..Default::default()
        })
        .collect()
}

/// `compact` puts title and time on one truncated line, for a block too short for two.
fn event<A: Actions + 'static>(s: &UiState<A>, e: &CalendarRow, date: chrono::NaiveDate, compact: bool) -> El {
    let selected = format!("{}-{date}", e.id);
    let menu_id = e.id.clone();
    let menu_id_for_press = menu_id.clone();
    let title = if e.cancelled {
        format!("Canceled: {}", e.subject)
    } else {
        e.subject.clone()
    };
    let active = s.screens.selected_event.as_ref() == Some(&selected);
    let time = (!e.all_day).then(|| format!(
        "{}–{}",
        e.start.with_timezone(&Local).format("%H:%M"),
        e.end.with_timezone(&Local).format("%H:%M")
    ));
    let block = if compact { row().gap(6.0).center_y().py(2.0).px(4.0) } else { col().padding(4.0) };
    let block = block
        .key(format!("calendar-{}-{date}", e.id))
        .w_full()
        .min_w(0.0)
        .label(title.clone())
        .radius(4.0)
        .fill(color(if active { "#DADDF5" } else { "#E8EBFA" }))
        .hover(color("#DADDF5"))
        .clip_soft(0.0)
        .on_press(move |s: &mut UiState<A>| {
            s.screens.selected_event = Some(selected.clone());
            s.screens.event_menu = Some(menu_id_for_press.clone());
        })
        .on_secondary_press(move |s: &mut UiState<A>, at, _| s.screens.event_context = Some((menu_id.clone(), at.x, at.y)));
    let title = text(title).font(style(12.0, 600.0)).color(color("#242424"));
    let title = if compact { title.truncate().grow(1.0).min_w(0.0) } else { title.wrap() };
    block.child(title).children(time.map(|time| text(time).font(style(11.0, 400.0)).color(color("#616161")).truncate().min_w(0.0)))
}

pub fn week<A: Actions + 'static>(s: &UiState<A>) -> El {
    let source = events(&s.screens.calendar);
    let count = s.screens.calendar_view.days();
    let dates: Vec<_> = (0..count as i64)
        .map(|day| s.screens.week + Duration::days(day))
        .collect();
    let days = model::week_days(&source, &dates, Local::now().offset().fix());
    let mut headings = row().grow(1.0).min_w(0.0).h_full().equal_columns(count);
    let all_day_height = 26.0
        * days
            .iter()
            .map(|d| d.all_day.len())
            .max()
            .unwrap_or(0)
            .max(1) as f32;
    let mut all_day = row().grow(1.0).min_w(0.0).h_full().equal_columns(count);
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
                all_day_column.child(event(s, &s.screens.calendar[index], day.date, true).h(24.0));
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
            // One pixel apart, so back-to-back meetings read as two blocks.
            let height = ((block.end_minute - block.start_minute) as f32 * HOUR / 60.0 - 1.0).max(1.0);
            day_column = day_column.child(
                row()
                    .w_full()
                    .h(height)
                    .absolute(0.0, block.start_minute as f32 * HOUR / 60.0)
                    .child(el().w_frac(block.lane as f32 / block.lanes as f32).h_full())
                    .child(
                        event(s, &s.screens.calendar[block.event_index], day.date, height < 40.0)
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

pub fn month<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut grid = col().key("calendar-month").grow(1.0).min_w(0.0).h_full().min_h(0.0);
    for week in 0..6 {
        let mut line = row().w_full().grow(1.0).min_h(0.0).equal_columns(7);
        for day in 0..7 {
            let date = s.screens.week + Duration::days(week * 7 + day);
            let mut cell = col().key(format!("month-day-{date}")).min_w(0.0).h_full().min_h(0.0).padding(4.0).gap(2.0).border(0.5, color("#E0E0E0"))
                .child(crate::controls::button::<A>(&format!("month-open-{date}"), &date.format("%a %d").to_string(), crate::controls::Variant::Secondary, crate::controls::Size::Sm, move |s: &mut UiState<A>, _| {
                    s.screens.calendar_view = crate::screens::CalendarView::Day;
                    crate::screens::select_date(s, date);
                }));
            let mut entries = col().w_full().gap(2.0);
            for entry in s.screens.calendar.iter().filter(|entry| crate::screens::occurs_on(entry, date)) {
                entries = entries.child(event(s, entry, date, true).h(28.0));
            }
            cell = cell.child(col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(entries));
            line = line.child(cell);
        }
        grid = grid.child(line);
    }
    grid
}

pub fn date_picker<A: Actions + 'static>(s: &UiState<A>) -> El {
    month_grid(s, "picker")
        .key("calendar-picker").w(280.0).padding(10.0).radius(8.0).fill(color("#FFFFFF")).border(1.0, color("#E0E0E0"))
        .anchored("calendar-date-picker", Side::Bottom, Align::Start, 4.0)
        .modal(true, |s: &mut UiState<A>| s.screens.date_picker = false)
}

/// A month to choose a day from, with arrows; its keys start with `prefix`. The header's
/// date popup and the left pane both draw it.
pub fn month_grid<A: Actions + 'static>(s: &UiState<A>, prefix: &str) -> El {
    let month = s.screens.picker_month;
    let first = month.with_day(1).unwrap_or(month);
    let start = first - Duration::days(i64::from(first.weekday().num_days_from_monday()));
    let mut picker = col().w_full().gap(4.0)
        .child(row().w_full().gap(4.0).center_y()
            .child(crate::controls::icon_button::<A>(&format!("{prefix}-previous"), "keyboard_arrow_left", "Previous month", false, |s: &mut UiState<A>, _| {
                s.screens.picker_month = s.screens.picker_month.checked_sub_months(chrono::Months::new(1)).unwrap_or(s.screens.picker_month);
            }))
            .child(text(month.format("%B %Y").to_string()).font(style(13.0, 600.0)).color(color("#242424")).grow(1.0))
            .child(crate::controls::icon_button::<A>(&format!("{prefix}-next"), "keyboard_arrow_right", "Next month", false, |s: &mut UiState<A>, _| {
                s.screens.picker_month = s.screens.picker_month.checked_add_months(chrono::Months::new(1)).unwrap_or(s.screens.picker_month);
            })));
    for week in 0..6 {
        let mut line = row().w_full().equal_columns(7);
        for day in 0..7 {
            let date = start + Duration::days(week * 7 + day);
            line = line.child(crate::controls::button::<A>(
                &format!("{prefix}-date-{date}"),
                &date.day().to_string(),
                if date == s.screens.calendar_date { crate::controls::Variant::Primary } else { crate::controls::Variant::Secondary },
                crate::controls::Size::Sm,
                move |s: &mut UiState<A>, _| crate::screens::select_date(s, date),
            ));
        }
        picker = picker.child(line);
    }
    picker
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
                ical_uid: None, dial_in: None,
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
        assert_eq!(a[3], HOUR - 1.0);
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

    fn picker_view(s: &UiState<Quiet>, _: &mut Cx<UiState<Quiet>>) -> El {
        date_picker(s)
    }

    fn by_key<'a>(elements: &'a [rune::ElementInfo], key: &str) -> &'a rune::ElementInfo {
        elements
            .iter()
            .find(|e| e.key.as_deref() == Some(key) || e.path.ends_with(&format!("/{key}")))
            .unwrap_or_else(|| panic!("no element {key}"))
    }

    #[test]
    fn date_picker_month_arrows_are_labelled_icon_buttons_and_the_day_is_primary() {
        use app_backend::schemes::active_palette;
        let date = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        let mut state = UiState::new(Quiet);
        state.screens.picker_month = date;
        state.screens.calendar_date = date;
        let mut rt = Runtime::new(state, picker_view, (1000.0, 700.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        let elements = rt.elements();
        for (key, label) in [("picker-previous", "Previous month"), ("picker-next", "Next month")] {
            let e = by_key(&elements, key);
            assert_eq!(e.role, "button");
            assert_eq!(e.label.as_deref(), Some(label), "icon button has an accessible name");
            assert!(e.focusable);
            assert!(e.text.is_none(), "no chevron glyph text in an icon button");
            assert!((e.radius - 6.0).abs() < 0.5, "radius {}", e.radius);
            assert!((e.rect[2] - 32.0).abs() < 0.5 && (e.rect[3] - 32.0).abs() < 0.5, "32x32 icon button");
        }
        let selected = by_key(&elements, "picker-date-2026-11-15");
        let accent = rune::color::hex(&format!("#{:06X}", active_palette().accent)).expect("accent colour");
        assert_eq!(selected.fill, accent, "the selected day uses the primary fill");
        let other = by_key(&elements, "picker-date-2026-11-16");
        assert_ne!(other.fill, accent, "other days stay secondary");
        assert!(
            elements.iter().all(|e| e.text.as_deref().is_none_or(|text| !text.contains('‹') && !text.contains('›'))),
            "text chevrons must not be used as state"
        );
    }
}
