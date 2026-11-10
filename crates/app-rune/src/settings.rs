//! Appearance settings shared with the existing desktop client.

use app_backend::appearance::ThemeChoice;
use rune::tok::Family;
use rune::*;

use crate::types::Actions;
use crate::ui::UiState;

/// Resolve the light UI's semantic colors through the selected shared palette.
pub fn color(hex: &str) -> Color {
    let palette = app_backend::schemes::active_palette();
    let value = match hex {
        "#FFFFFF" => Some(palette.surface),
        "#F5F5F5" => Some(palette.background),
        "#EBEBEB" => Some(palette.row_hover),
        "#E0E0E0" => Some(palette.border),
        "#242424" => Some(palette.foreground),
        "#616161" => Some(palette.text_muted),
        "#5B5FC7" => Some(palette.accent),
        "#E8EBFA" => Some(palette.bubble_own),
        "#C4314B" => Some(palette.red),
        _ => None,
    };
    let mapped = value.map(|value| format!("#{value:06X}"));
    rune::color::hex(mapped.as_deref().unwrap_or(hex))
        .expect("valid palette color")
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut choices = col().w_full().gap(8.0);
    for choice in ThemeChoice::ALL {
        let selected = s.appearance.theme == choice;
        choices = choices.child(
            row()
                .key(format!("theme-{}", choice.key()))
                .w_full()
                .h(38.0)
                .px(12.0)
                .center_y()
                .on_press(move |s: &mut UiState<A>| {
                    s.appearance.theme = choice;
                    s.actions.set_appearance(s.appearance);
                })
                .child(text(format!("{}  {}", if selected { "●" } else { "○" }, choice.label())).color(color("#242424"))),
        );
    }
    col()
        .key("settings")
        .w_full()
        .h_full()
        .fill(color("#FFFFFF"))
        .padding(24.0)
        .gap(16.0)
        .child(text("Settings").font(TextStyle::new(20.0, 700.0).family(Family::Named("Segoe UI"))).color(color("#242424")))
        .child(text("Appearance").font(TextStyle::new(16.0, 600.0).family(Family::Named("Segoe UI"))).color(color("#242424")))
        .child(text("Theme").color(color("#242424")))
        .child(choices)
}
