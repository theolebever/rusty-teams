//! Material Symbols the Rune views draw, embedded from the GPUI app's assets. `sym` returns a
//! glyph sized in points; tint it with `.color()`.

#![allow(dead_code)] // the views that draw these land after this module

use std::collections::HashMap;
use std::sync::OnceLock;

use rune::*;

const SOURCES: &[(&str, &str)] = &[
    ("add", include_str!("../../app/assets/symbols/add.svg")),
    ("alternate_email", include_str!("../../app/assets/symbols/alternate_email.svg")),
    ("attach_file", include_str!("../../app/assets/symbols/attach_file.svg")),
    ("auto_fix_high", include_str!("../../app/assets/symbols/auto_fix_high.svg")),
    ("bookmark", include_str!("../../app/assets/symbols/bookmark.svg")),
    ("calendar_month", include_str!("../../app/assets/symbols/calendar_month.svg")),
    ("call", include_str!("../../app/assets/symbols/call.svg")),
    ("chat_bubble", include_str!("../../app/assets/symbols/chat_bubble.svg")),
    ("chat_bubble_fill", include_str!("../../app/assets/symbols/chat_bubble_fill.svg")),
    ("check", include_str!("../../app/assets/symbols/check.svg")),
    ("close", include_str!("../../app/assets/symbols/close.svg")),
    ("done_all", include_str!("../../app/assets/symbols/done_all.svg")),
    ("drafts", include_str!("../../app/assets/symbols/drafts.svg")),
    ("edit", include_str!("../../app/assets/symbols/edit.svg")),
    ("favorite", include_str!("../../app/assets/symbols/favorite.svg")),
    ("filter_alt", include_str!("../../app/assets/symbols/filter_alt.svg")),
    ("group", include_str!("../../app/assets/symbols/group.svg")),
    ("group_fill", include_str!("../../app/assets/symbols/group_fill.svg")),
    ("keep", include_str!("../../app/assets/symbols/keep.svg")),
    ("keyboard_arrow_down", include_str!("../../app/assets/symbols/keyboard_arrow_down.svg")),
    ("keyboard_arrow_right", include_str!("../../app/assets/symbols/keyboard_arrow_right.svg")),
    ("mood", include_str!("../../app/assets/symbols/mood.svg")),
    ("more_horiz", include_str!("../../app/assets/symbols/more_horiz.svg")),
    ("notifications", include_str!("../../app/assets/symbols/notifications.svg")),
    ("notifications_off", include_str!("../../app/assets/symbols/notifications_off.svg")),
    ("person_add", include_str!("../../app/assets/symbols/person_add.svg")),
    ("reply", include_str!("../../app/assets/symbols/reply.svg")),
    ("schedule", include_str!("../../app/assets/symbols/schedule.svg")),
    ("search", include_str!("../../app/assets/symbols/search.svg")),
    ("send", include_str!("../../app/assets/symbols/send.svg")),
    ("settings", include_str!("../../app/assets/symbols/settings.svg")),
    ("smart_toy", include_str!("../../app/assets/symbols/smart_toy.svg")),
    ("star", include_str!("../../app/assets/symbols/star.svg")),
    ("status_available", include_str!("../../app/assets/symbols/status_available.svg")),
    ("status_away", include_str!("../../app/assets/symbols/status_away.svg")),
    ("status_dnd", include_str!("../../app/assets/symbols/status_dnd.svg")),
    ("status_dot", include_str!("../../app/assets/symbols/status_dot.svg")),
    ("status_offline", include_str!("../../app/assets/symbols/status_offline.svg")),
    ("text_format", include_str!("../../app/assets/symbols/text_format.svg")),
    ("thumb_up", include_str!("../../app/assets/symbols/thumb_up.svg")),
    ("videocam", include_str!("../../app/assets/symbols/videocam.svg")),
];

static ICONS: OnceLock<HashMap<&'static str, Icon>> = OnceLock::new();

/// A glyph for the Material Symbol `name`, `size` points square. Panics on a name not in `SOURCES`.
pub fn sym(name: &str, size: f32) -> El {
    rune::icon(lookup(name), size)
}

fn lookup(name: &str) -> &'static Icon {
    ICONS
        .get_or_init(|| SOURCES.iter().map(|&(name, svg)| (name, build(name, svg))).collect())
        .get(name)
        .unwrap_or_else(|| panic!("no symbol named {name:?}"))
}

/// The SVG's own viewBox is moved to start at the origin, so the body draws inside `0..view`.
/// Material files use `0 -960 960 960`; the status dots use `0 0 24 24`.
fn build(name: &'static str, svg: &'static str) -> Icon {
    let view_box = svg
        .split("viewBox=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("symbol has a viewBox");
    let [min_x, min_y, view, _]: [f32; 4] = view_box
        .split_whitespace()
        .map(|number| number.parse().expect("viewBox numbers"))
        .collect::<Vec<f32>>()
        .try_into()
        .expect("viewBox has four numbers");
    // `0.0 - x` rather than `-x`, which would print "-0" for a viewBox starting at zero.
    let body = format!("<g transform=\"translate({} {})\">{svg}</g>", 0.0 - min_x, 0.0 - min_y);
    Icon { name, view, body: Box::leak(body.into_boxed_str()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sym_returns_an_element_for_every_listed_name() {
        for (name, _) in SOURCES {
            let _ = sym(name, 16.0);
        }
    }

    #[test]
    fn material_viewbox_is_moved_to_the_origin() {
        let icon = lookup("search");
        assert_eq!(icon.view, 960.0);
        assert!(icon.body.starts_with("<g transform=\"translate(0 960)\">"));
    }
}
