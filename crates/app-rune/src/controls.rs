//! Shared button and control primitives for the Rune views.
//!
//! One place for a button's role, accessible name, focus, palette and geometry, so the screens
//! stop carrying a dozen local copies of the same row. Every colour comes from the active
//! `app_backend::schemes` palette; views never hardcode a hex value here.

use app_backend::schemes::active_palette;
use rune::*;

use crate::icons::sym;
use crate::types::Actions;
use crate::ui::{self, ts, UiState};

/// The visual weight of a text button. `Quiet` is the dimmed, disabled look; `Menu` is an
/// unfilled row that picks up the popup's hover colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Variant {
    Secondary,
    Primary,
    Danger,
    Quiet,
    Menu,
}

/// Geometry of a text button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Size {
    Sm,
    Md,
}

impl Size {
    fn metrics(self) -> (f32, f32, f32) {
        match self {
            Self::Sm => (24.0, 8.0, 12.0),
            Self::Md => (32.0, 12.0, 13.0),
        }
    }
}

fn tone(value: u32) -> Color {
    rune::color::hex(&format!("#{value:06X}")).expect("palette colour")
}

/// A text button with a role, accessible name, keyboard focus and a palette fill.
///
/// `screens::button(label, key, action)` can migrate by calling this with
/// `(key, label, Variant::Secondary, Size::Md, action)`.
pub(crate) fn button<A: Actions + 'static>(
    key: &str,
    label: &str,
    variant: Variant,
    size: Size,
    action: impl Fn(&mut UiState<A>, &mut Tx) + 'static,
) -> El {
    button_with_help(key, label, "", variant, size, action)
}

/// [`button`] with a tooltip / accessibility help. An empty `help` adds nothing.
pub(crate) fn button_with_help<A: Actions + 'static>(
    key: &str,
    label: &str,
    help: &str,
    variant: Variant,
    size: Size,
    action: impl Fn(&mut UiState<A>, &mut Tx) + 'static,
) -> El {
    let palette = active_palette();
    let (fill, hover, ink, disabled) = match variant {
        Variant::Secondary => (tone(palette.surface_raised), tone(palette.row_hover), tone(palette.foreground), false),
        Variant::Primary => (tone(palette.accent), tone(palette.accent_strong), tone(palette.on_accent), false),
        Variant::Danger => (tone(palette.red), tone(palette.red), tone(palette.on_accent), false),
        Variant::Quiet => (Color::TRANSPARENT, tone(palette.row_hover), tone(palette.text_muted), true),
        Variant::Menu => (Color::TRANSPARENT, tone(palette.popup_hover), tone(palette.foreground), false),
    };
    let (height, padding, font) = size.metrics();
    let mut el = row()
        .key(key.to_owned())
        .h(height)
        .px(padding)
        .center()
        .radius(6.0)
        .fill(fill)
        .hover(hover)
        .role("button")
        .label(label.to_owned())
        .focusable(!disabled)
        .focus_ring(2.0, 1.0, tone(palette.accent));
    if disabled {
        el = el.opacity(0.4).disabled(true);
    } else {
        el = el.on_press_tx(action);
    }
    if !help.is_empty() {
        el = el.help(help.to_owned());
    }
    el.child(ui::text(label.to_owned()).font(ts(font, 600.0)).color(ink))
}

/// A 32x32 icon-only button. `label` names it for assistive tech, `active` drives its checked
/// state and accent tint.
pub(crate) fn icon_button<A: Actions + 'static>(
    key: &str,
    glyph: &str,
    label: &str,
    active: bool,
    action: impl Fn(&mut UiState<A>, &mut Tx) + 'static,
) -> El {
    let palette = active_palette();
    let ink = if active { tone(palette.accent) } else { tone(palette.foreground) };
    icon_button_with_ink(key, glyph, label, active, ink, action)
}

/// [`icon_button`] with a caller-supplied icon ink. The fill, hover and focus ring still follow
/// the active palette.
pub(crate) fn icon_button_with_ink<A: Actions + 'static>(
    key: &str,
    glyph: &str,
    label: &str,
    active: bool,
    ink: Color,
    action: impl Fn(&mut UiState<A>, &mut Tx) + 'static,
) -> El {
    let palette = active_palette();
    icon_button_base(
        key, glyph, label, active, ink,
        (tone(palette.accent_soft), tone(palette.row_hover), tone(palette.accent)), action,
    )
}

/// An icon button on a surface whose colour is fixed by the scene rather than the theme — the
/// dark stage of a call. White ink and a translucent white active/hover fill keep it readable
/// even when the user forces the light theme, while the geometry and a11y stay shared.
pub(crate) fn icon_button_on_dark<A: Actions + 'static>(
    key: &str,
    glyph: &str,
    label: &str,
    active: bool,
    action: impl Fn(&mut UiState<A>, &mut Tx) + 'static,
) -> El {
    icon_button_base(
        key, glyph, label, active, Color::WHITE,
        (Color::WHITE.alpha(0.18), Color::WHITE.alpha(0.12), Color::WHITE.alpha(0.7)), action,
    )
}

fn icon_button_base<A: Actions + 'static>(
    key: &str,
    glyph: &str,
    label: &str,
    active: bool,
    ink: Color,
    (active_fill, hover, ring): (Color, Color, Color),
    action: impl Fn(&mut UiState<A>, &mut Tx) + 'static,
) -> El {
    row()
        .key(key.to_owned())
        .size(32.0, 32.0)
        .center()
        .radius(6.0)
        .fill(if active { active_fill } else { Color::TRANSPARENT })
        .hover(hover)
        .role("button")
        .label(label.to_owned())
        .checked(active)
        .focusable(true)
        .focus_ring(2.0, 1.0, ring)
        .on_press_tx(action)
        .child(sym(glyph, 18.0).color(ink))
}

/// A full-width menu row: no fill at rest, the popup's hover colour under the pointer.
pub(crate) fn menu_item<A: Actions + 'static>(
    key: &str,
    label: &str,
    glyph: Option<&str>,
    selected: bool,
    action: impl Fn(&mut UiState<A>) + 'static,
) -> El {
    let palette = active_palette();
    let ink = if selected { tone(palette.accent) } else { tone(palette.foreground) };
    row()
        .key(key.to_owned())
        .w_full()
        .h(32.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(6.0)
        .hover(tone(palette.popup_hover))
        .role("menuitem")
        .label(label.to_owned())
        .focusable(true)
        .focus_selected(selected)
        .on_press(action)
        .children(
            glyph
                .map(|g| sym(g, 16.0).color(ink))
                .into_iter()
                .chain([ui::text(label.to_owned()).font(ts(13.0, 400.0)).color(ink)]),
        )
}

/// A switch row with an accent track and a knob drawn from `el()` (no text glyphs).
pub(crate) fn switch_item<A: Actions + 'static>(
    key: &str,
    label: &str,
    on: bool,
    enabled: bool,
    press: impl Fn(&mut UiState<A>) + 'static,
) -> El {
    let palette = active_palette();
    let track = if on { tone(palette.accent) } else { tone(palette.surface_raised) };
    let knob = if on { tone(palette.on_accent) } else { tone(palette.text_muted) };
    let dot = || el().size(14.0, 14.0).radius(7.0).fill(knob);
    let spacer = || el().size(14.0, 14.0);
    let mut el = row()
        .key(key.to_owned())
        .w_full()
        .min_h(40.0)
        .px(12.0)
        .gap(12.0)
        .center_y()
        .radius(6.0)
        .role("switch")
        .label(label.to_owned())
        .checked(on)
        .focusable(enabled)
        .focus_ring(2.0, 1.0, tone(palette.accent));
    if enabled {
        el = el.hover(tone(palette.row_hover)).on_press(press);
    } else {
        el = el.opacity(0.4).disabled(true);
    }
    el.child(
            ui::text(label.to_owned())
                .font(ts(13.0, 400.0))
                .color(tone(palette.foreground))
                .wrap()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(
            row()
                .w(32.0)
                .h(18.0)
                .radius(9.0)
                .fill(track)
                .padding(2.0)
                .items_center()
                .justify_between()
                .child(if on { spacer() } else { dot() })
                .child(if on { dot() } else { spacer() }),
        )
}

/// A radio row whose circle is drawn with `el().radius`, never with a `●`/`○` text glyph.
pub(crate) fn radio_item<A: Actions + 'static>(
    key: &str,
    label: &str,
    selected: bool,
    press: impl Fn(&mut UiState<A>) + 'static,
) -> El {
    let palette = active_palette();
    let ring = if selected { tone(palette.accent) } else { tone(palette.border_strong) };
    row()
        .key(key.to_owned())
        .w_full()
        .min_h(40.0)
        .px(12.0)
        .gap(10.0)
        .center_y()
        .radius(6.0)
        .hover(tone(palette.row_hover))
        .role("radio")
        .label(label.to_owned())
        .checked(selected)
        .focusable(true)
        .focus_ring(2.0, 1.0, tone(palette.accent))
        .on_press(press)
        .child(
            el()
                .size(16.0, 16.0)
                .radius(8.0)
                .border(2.0, ring)
                .center()
                .child(if selected {
                    el().size(8.0, 8.0).radius(4.0).fill(tone(palette.accent))
                } else {
                    el().size(8.0, 8.0)
                }),
        )
        .child(
            ui::text(label.to_owned())
                .font(ts(13.0, 400.0))
                .color(tone(palette.foreground))
                .wrap()
                .grow(1.0)
                .min_w(0.0),
        )
}

/// A small pill toggle: 28 high, radius 14, accent fill when on.
pub(crate) fn chip<A: Actions + 'static>(
    key: &str,
    label: &str,
    on: bool,
    press: impl Fn(&mut UiState<A>) + 'static,
) -> El {
    let palette = active_palette();
    let (fill, ink) = if on {
        (tone(palette.accent), tone(palette.on_accent))
    } else {
        (tone(palette.surface_raised), tone(palette.foreground))
    };
    row()
        .key(key.to_owned())
        .h(28.0)
        .px(10.0)
        .center()
        .radius(14.0)
        .fill(fill)
        .hover(tone(palette.row_hover))
        .role("button")
        .label(label.to_owned())
        .checked(on)
        .focusable(true)
        .focus_ring(2.0, 1.0, tone(palette.accent))
        .on_press(press)
        .child(ui::text(label.to_owned()).font(ts(12.0, 600.0)).color(ink))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Actions, SendTarget};
    use crate::ui::UiState;
    use rune::{ElementInfo, Env, Runtime};

    struct Quiet;

    impl Actions for Quiet {
        fn open(&self, _conversation_id: &str) {}
        fn send(&self, _target: SendTarget, _text: String, _files: Vec<teams_core::FileReference>) {}
    }

    fn fixture(_s: &UiState<Quiet>, _cx: &mut Cx<UiState<Quiet>>) -> El {
        col()
            .key("controls-fixture")
            .w(400.0)
            .gap(4.0)
            .padding(8.0)
            .child(button::<Quiet>("btn-secondary", "Save", Variant::Secondary, Size::Md, |_, _| {}))
            .child(button_with_help::<Quiet>(
                "btn-primary",
                "Send",
                "Send the message",
                Variant::Primary,
                Size::Md,
                |_, _| {},
            ))
            .child(button::<Quiet>("btn-danger", "Delete", Variant::Danger, Size::Md, |_, _| {}))
            .child(button::<Quiet>("btn-quiet", "Cancel", Variant::Quiet, Size::Sm, |_, _| {}))
            .child(button::<Quiet>("btn-menu-icon", "i", Variant::Menu, Size::Md, |_, _| {}))
            .child(icon_button::<Quiet>("icon-settings", "settings", "Settings", true, |_, _| {}))
            .child(icon_button_on_dark::<Quiet>("icon-dark", "call", "Call", true, |_, _| {}))
            .child(menu_item::<Quiet>("menu-item", "Rename", Some("edit"), true, |_| {}))
            .child(switch_item::<Quiet>("switch-on", "Notifications", true, true, |_| {}))
            .child(switch_item::<Quiet>("switch-off", "Do not disturb", false, false, |_| {}))
            .child(radio_item::<Quiet>("radio-on", "Light", true, |_| {}))
            .child(radio_item::<Quiet>("radio-off", "Dark", false, |_| {}))
            .child(chip::<Quiet>("chip-on", "Today", true, |_| {}))
            .child(chip::<Quiet>("chip-off", "Tomorrow", false, |_| {}))
    }

    fn runtime() -> Runtime<UiState<Quiet>> {
        let mut rt = Runtime::new(
            UiState::new(Quiet),
            fixture,
            (1000.0, 700.0),
            Env { system_dark: false, scale: 1.0 },
        );
        rt.frame(0.0);
        rt
    }

    fn by_key<'a>(elements: &'a [ElementInfo], key: &str) -> &'a ElementInfo {
        elements
            .iter()
            .find(|e| e.path.ends_with(&format!("/{key}")) || e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("no element {key} in:\n{}", elements.iter().map(|e| e.path.as_str()).collect::<Vec<_>>().join("\n")))
    }

    fn a11y_by_key<'a>(node: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
        if node.get("key").and_then(|v| v.as_str()) == Some(key) {
            return Some(node);
        }
        node.get("children")
            .and_then(|c| c.as_array())
            .into_iter()
            .flatten()
            .find_map(|child| a11y_by_key(child, key))
    }

    fn states(node: &serde_json::Value) -> Vec<&str> {
        node.get("states")
            .and_then(|s| s.as_array())
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str())
            .collect()
    }

    #[test]
    fn buttons_expose_role_label_and_focus() {
        let rt = runtime();
        let elements = rt.elements();
        for (key, label) in [("btn-secondary", "Save"), ("btn-primary", "Send"), ("btn-danger", "Delete")] {
            let e = by_key(&elements, key);
            assert_eq!(e.role, "button", "{key}");
            assert!(e.focusable, "{key} must be keyboard reachable");
            assert_eq!(e.label.as_deref(), Some(label), "{key}");
            assert!((e.rect[3] - 32.0).abs() < 0.5, "{key} height {}", e.rect[3]);
            assert!((e.radius - 6.0).abs() < 0.5, "{key} radius {}", e.radius);
        }
    }

    #[test]
    fn help_is_exposed_only_when_supplied() {
        let rt = runtime();
        let elements = rt.elements();
        assert_eq!(by_key(&elements, "btn-primary").help.as_deref(), Some("Send the message"));
        assert_eq!(by_key(&elements, "btn-secondary").help.as_deref(), None);
    }

    #[test]
    fn variants_use_distinct_palette_fills() {
        let rt = runtime();
        let elements = rt.elements();
        let primary = by_key(&elements, "btn-primary");
        let danger = by_key(&elements, "btn-danger");
        let secondary = by_key(&elements, "btn-secondary");
        let quiet = by_key(&elements, "btn-quiet");
        assert_ne!(primary.fill, danger.fill, "Primary and Danger must not share a fill");
        assert_ne!(primary.fill, secondary.fill, "Primary and Secondary must not share a fill");
        assert_eq!(quiet.fill.a, 0.0, "Quiet has no fill at rest");
        assert!(quiet.opacity < 1.0, "Quiet is the disabled look");
    }

    #[test]
    fn menu_variant_is_available() {
        let rt = runtime();
        let elements = rt.elements();
        let e = by_key(&elements, "btn-menu-icon");
        assert_eq!(e.role, "button");
        assert_eq!(e.label.as_deref(), Some("i"));
        assert!(e.focusable);
        assert!((e.rect[3] - 32.0).abs() < 0.5);
        assert!((e.radius - 6.0).abs() < 0.5);
        assert_eq!(e.fill.a, 0.0, "Menu has no fill at rest");
    }

    #[test]
    fn sizes_change_height_and_icon_button_is_square() {
        let rt = runtime();
        let elements = rt.elements();
        assert!((by_key(&elements, "btn-quiet").rect[3] - 24.0).abs() < 0.5, "Sm height");
        assert!((by_key(&elements, "btn-secondary").rect[3] - 32.0).abs() < 0.5, "Md height");
        let icon = by_key(&elements, "icon-settings");
        assert!((icon.rect[2] - 32.0).abs() < 0.5 && (icon.rect[3] - 32.0).abs() < 0.5, "Icon button is 32x32");
    }

    #[test]
    fn icon_button_has_an_accessible_name_without_glyph_text() {
        let rt = runtime();
        let elements = rt.elements();
        let e = by_key(&elements, "icon-settings");
        assert_eq!(e.role, "button");
        assert_eq!(e.label.as_deref(), Some("Settings"));
        assert!(e.focusable);
        assert!(e.text.is_none(), "an icon button must not rely on glyph text");
        let tree = rt.accessibility_json();
        let node = a11y_by_key(&tree, "icon-settings").expect("icon button in the a11y tree");
        assert!(states(node).contains(&"checked"), "active icon button is checked");
    }

    #[test]
    fn icon_button_can_force_ink_for_a_dark_stage() {
        let rt = runtime();
        let elements = rt.elements();
        let button = elements.iter()
            .position(|element| element.key.as_deref() == Some("icon-dark"))
            .expect("dark-stage icon button");
        let icon = elements.iter()
            .find(|element| element.parent == Some(button) && element.text_color.is_some())
            .expect("icon inside the button");
        assert_eq!(icon.text_color, rune::color::hex("#FFFFFF"));
        assert!(elements[button].fill.a > 0.0, "the active fill must stay visible on a dark stage");
    }

    #[test]
    fn menu_item_is_full_width_unfilled_and_selected() {
        let rt = runtime();
        let elements = rt.elements();
        let e = by_key(&elements, "menu-item");
        assert_eq!(e.role, "menuitem");
        assert_eq!(e.label.as_deref(), Some("Rename"));
        assert!(e.focusable);
        assert!((e.rect[3] - 32.0).abs() < 0.5);
        assert!(e.rect[2] > 300.0, "menu row should stretch, got {}", e.rect[2]);
        assert_eq!(e.fill.a, 0.0, "no fill at rest");
        assert!(e.item_selected, "selected menu row is marked for assistive tech");
    }

    #[test]
    fn switch_items_expose_role_label_and_disabled_opacity() {
        let rt = runtime();
        let elements = rt.elements();
        let on = by_key(&elements, "switch-on");
        let off = by_key(&elements, "switch-off");
        assert_eq!(on.role, "switch");
        assert_eq!(on.label.as_deref(), Some("Notifications"));
        assert!(on.focusable);
        assert!((on.opacity - 1.0).abs() < 0.01);
        assert!((off.opacity - 0.4).abs() < 0.01, "disabled switch is dimmed");
        assert!(!off.focusable, "disabled switch leaves the tab order");
        let tree = rt.accessibility_json();
        let on_node = a11y_by_key(&tree, "switch-on").expect("switch-on a11y node");
        let off_node = a11y_by_key(&tree, "switch-off").expect("switch-off a11y node");
        assert!(states(on_node).contains(&"checked"));
        assert!(states(off_node).contains(&"unchecked"));
        assert!(states(off_node).contains(&"disabled"));
    }

    #[test]
    fn radio_items_use_drawn_circles_not_glyph_text() {
        let rt = runtime();
        let elements = rt.elements();
        let on = by_key(&elements, "radio-on");
        assert_eq!(on.role, "radio");
        assert_eq!(on.label.as_deref(), Some("Light"));
        assert!(on.focusable);
        assert!(
            elements.iter().all(|e| e.text.as_deref().map_or(true, |t| !t.contains('●') && !t.contains('○'))),
            "no text radio dots anywhere in the tree"
        );
        let tree = rt.accessibility_json();
        assert!(states(a11y_by_key(&tree, "radio-on").unwrap()).contains(&"checked"));
        assert!(states(a11y_by_key(&tree, "radio-off").unwrap()).contains(&"unchecked"));
    }

    #[test]
    fn chips_are_round_and_use_accent_when_on() {
        let rt = runtime();
        let elements = rt.elements();
        let on = by_key(&elements, "chip-on");
        let off = by_key(&elements, "chip-off");
        assert_eq!(on.role, "button");
        assert_eq!(on.label.as_deref(), Some("Today"));
        assert!(on.focusable);
        assert!((on.rect[3] - 28.0).abs() < 0.5, "chip height");
        assert!(on.radius >= 12.0, "chip radius {}", on.radius);
        assert_ne!(on.fill, off.fill, "on and off chips differ");
        let tree = rt.accessibility_json();
        assert!(states(a11y_by_key(&tree, "chip-on").unwrap()).contains(&"checked"));
        assert!(states(a11y_by_key(&tree, "chip-off").unwrap()).contains(&"unchecked"));
    }
}
