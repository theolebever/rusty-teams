//! Window chrome as the GPUI shell draws it: the title bar across the top (logo and name, the
//! global search box, the settings and account controls) and the navigation rail down the left
//! (`crates/app/src/views/title_bar.rs`, `rail.rs`). The chat list and the screens stay in `ui`.

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

use crate::icons::sym;
use crate::screens::{self, Screen};
use crate::types::Actions;
use crate::ui::{UiState, c, ts, INK, MUTED};

/// Height of the title bar the window draws over its caption buttons (`main.rs`).
pub const TITLE_BAR_HEIGHT: f32 = 40.0;
const RAIL_WIDTH: f32 = 60.0;
const RAIL_ITEM_WIDTH: f32 = 48.0;
const RAIL_ITEM_HEIGHT: f32 = 52.0;
const RAIL_RADIUS: f32 = 10.0;
const RAIL_ICON: f32 = 20.0;
const SEARCH_WIDTH: f32 = 380.0;
const SEARCH_HEIGHT: f32 = 28.0;
const BADGE_CAP: usize = 99;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RailItem {
    label: &'static str,
    outline: &'static str,
    filled: &'static str,
    screen: Screen,
    badge: usize,
    /// Shows a plain dot instead of the count in `badge` (unread channels).
    dot: bool,
}

fn rail_items(activity: usize, unread_chats: usize, channels: usize, agents: usize) -> Vec<RailItem> {
    vec![
        RailItem { label: "Activity", outline: "notifications", filled: "notifications", screen: Screen::Activity, badge: activity, dot: false },
        RailItem { label: "Chat", outline: "chat_bubble", filled: "chat_bubble_fill", screen: Screen::Chat, badge: unread_chats, dot: false },
        RailItem { label: "Teams", outline: "group", filled: "group", screen: Screen::Teams, badge: channels, dot: true },
        RailItem { label: "Calendar", outline: "calendar_month", filled: "calendar_month", screen: Screen::Calendar, badge: 0, dot: false },
        RailItem { label: "Calls", outline: "call", filled: "call", screen: Screen::Calls, badge: 0, dot: false },
        RailItem { label: "Agents", outline: "auto_fix_high", filled: "auto_fix_high", screen: Screen::Agents, badge: agents, dot: false },
    ]
}

/// The count on a rail item: none for zero, `99+` past the cap.
fn badge_text(count: usize) -> Option<String> {
    match count {
        0 => None,
        n if n > BADGE_CAP => Some(format!("{BADGE_CAP}+")),
        n => Some(n.to_string()),
    }
}

fn tone(value: u32) -> Color {
    rune::color::hex(&format!("#{value:06X}")).expect("palette colour")
}

/// The title bar across the top. `trailing` is the width the window's caption buttons take on the
/// right (`Cx::chrome_insets`), kept clear of the controls.
pub fn title_bar<A: Actions + 'static>(s: &UiState<A>, trailing: f32) -> El {
    let palette = active_palette();
    row()
        .key("title-bar")
        .w_full()
        .h(TITLE_BAR_HEIGHT)
        .fill(tone(palette.surface))
        .center_y()
        .window_drag()
        .child(
            row()
                .key("title-leading")
                .grow(1.0)
                .min_w(0.0)
                .h_full()
                .center_y()
                .gap(4.0)
                .pl(14.0)
                .pr(12.0)
                .child(el().key("title-logo").size(20.0, 20.0).pixels_fit(s.media.logo().pixels(), SurfaceFit::Fit))
                .child(text(app_base::APP_NAME).font(ts(13.0, 600.0)).color(tone(palette.foreground)).truncate())
                .children(s.read_only.then(|| mode_pill("Read-only")))
                .child(el().grow(1.0))
                .child(history_button::<A>("history-back", "keyboard_arrow_left", "Back", s.screens.can_go_back(), screens::go_back))
                .child(history_button::<A>("history-forward", "keyboard_arrow_right", "Forward", s.screens.can_go_forward(), screens::go_forward)),
        )
        .child(search_box::<A>())
        .child(
            row()
                .key("title-trailing")
                .grow(1.0)
                .min_w(0.0)
                .h_full()
                .center_y()
                .justify_end()
                .gap(12.0)
                .pr(trailing + 4.0)
                .child(
                    row()
                        .key("title-more")
                        .no_window_drag()
                        .size(28.0, 28.0)
                        .radius(6.0)
                        .center()
                        .hover(tone(palette.row_hover))
                        .label("Settings and more")
                        .help("Settings and more")
                        .on_press(|s: &mut UiState<A>| s.nav.settings_more = !s.nav.settings_more)
                        .child(sym("more_horiz", 18.0).color(c(INK))),
                )
                .child(crate::status_menu::account_button(s)),
        )
}

/// The amber mode pill shown after the app name ("Read-only").
fn mode_pill(label: &str) -> El {
    let amber = active_palette().amber;
    row()
        .key("title-mode")
        .h(18.0)
        .px(8.0)
        .radius(9.0)
        .center()
        .fill(Color { a: 0.14, ..tone(amber) })
        .child(text(label).font(ts(11.0, 600.0)).color(tone(amber)))
}

/// A back or forward button; muted and inert while `enabled` is false.
fn history_button<A: Actions + 'static>(
    key: &str,
    glyph: &str,
    label: &str,
    enabled: bool,
    go: fn(&mut UiState<A>),
) -> El {
    let palette = active_palette();
    let ink = tone(if enabled { palette.foreground } else { palette.text_muted });
    crate::controls::icon_button_with_ink::<A>(key, glyph, label, false, ink, move |s: &mut UiState<A>, _| go(s))
        .disabled(!enabled)
        .no_window_drag()
}

/// The global search box opens and focuses the switcher.
fn search_box<A: Actions + 'static>() -> El {
    let palette = active_palette();
    let muted = c(MUTED);
    row()
        .key("title-search")
        .no_window_drag()
        .w(SEARCH_WIDTH)
        .shrink()
        .min_w(80.0)
        .h(SEARCH_HEIGHT)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(6.0)
        .fill(tone(palette.surface))
        .on_press_tx(crate::switcher::open::<A>)
        .child(sym("search", 16.0).color(muted))
        .child(
            text("Search chats, people and channels")
                .font(ts(12.0, 400.0))
                .color(muted)
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        )
        // ponytail: always shown; GPUI hides it below 200px, Rune's view has no width to test.
        .child(
            row()
                .key("title-search-shortcut")
                .h(18.0)
                .px(6.0)
                .radius(4.0)
                .center()
                .fill(tone(palette.surface_raised))
                .child(text("Ctrl K").font(ts(11.0, 400.0)).color(muted)),
        )
}

/// The navigation rail down the left: icon over label, the selected screen on a raised pill.
pub fn rail<A: Actions + 'static>(s: &UiState<A>) -> El {
    let unread = s.chats.iter().filter(|chat| chat.unread).count();
    let mut rail = col().key("rail").w(RAIL_WIDTH).h_full().fill(tone(active_palette().surface)).items_center().gap(2.0);
    let activity = &s.screens.activity;
    let activity_unread = activity.items.iter().filter(|item| activity.counts_for_bell(item)).count();
    let channels = s.visible_channels().filter(|(_, channel)| channel.unread).count();
    for item in rail_items(activity_unread, unread, channels, usize::from(s.agent.running)) {
        rail = rail.child(rail_item::<A>(item, s.screens.screen == item.screen));
    }
    rail
}

fn rail_item<A: Actions + 'static>(item: RailItem, active: bool) -> El {
    let palette = active_palette();
    let ink = tone(if active { palette.accent_text } else { palette.text_muted });
    let screen = item.screen;
    let glyph = if active { item.filled } else { item.outline };
    let mut button = col()
        .key(format!("nav-{}", item.label))
        .w(RAIL_ITEM_WIDTH)
        .h(RAIL_ITEM_HEIGHT)
        .radius(RAIL_RADIUS)
        .pt(6.0)
        .gap(4.0)
        .items_center()
        .on_press_tx(move |s: &mut UiState<A>, tx| {
            screens::navigate_tab(s, screen, tx);
            if screen == Screen::Calendar { tx.scroll_to_instant("calendar-hours", 0.0, 7.0 * 48.0); }
        })
        .child(sym(glyph, RAIL_ICON).color(ink))
        .label(item.label)
        .help(format!("{} ({})", item.label, shortcut(screen)))
        .child(text(item.label).font(ts(9.0, 600.0)).color(ink).truncate());
    button = button.hover(tone(palette.row_hover));
    if active {
        button = button.fill(tone(palette.surface_raised));
    }
    if item.dot {
        if item.badge > 0 {
            let dot = el()
                .key(format!("nav-{}-dot", item.label))
                .size(7.0, 7.0)
                .radius(3.5)
                .fill(tone(palette.accent))
                .absolute(31.0, 8.0);
            button = button.child(dot);
        }
    } else if let Some(count) = badge_text(item.badge) {
        let badge = row()
            .key(format!("nav-{}-badge", item.label))
            .h(16.0)
            .min_w(16.0)
            .px(4.0)
            .radius(8.0)
            .center()
            .fill(tone(palette.accent))
            .child(text(count).font(ts(10.0, 600.0)).color(tone(palette.on_accent)));
        button = button.child(badge.absolute(28.0, 5.0));
    }
    button
}

fn shortcut(screen: Screen) -> &'static str {
    match screen {
        Screen::Activity => "Ctrl+1",
        Screen::Chat => "Ctrl+2",
        Screen::Teams => "Ctrl+3",
        Screen::Agents => "Ctrl+6",
        Screen::Calendar => "Ctrl+Shift+4",
        Screen::Calls => "Ctrl+5",
        Screen::Settings => "Ctrl+,",
        Screen::Agenda => "",
    }
}

pub fn more_menu<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    if !s.nav.settings_more { return None; }
    Some(col().key("settings-more-menu").w(220.0).padding(6.0).gap(4.0).radius(8.0)
        .fill(tone(active_palette().popup)).border(1.0, tone(active_palette().border))
        .anchored("title-more", Side::Bottom, Align::End, 4.0)
        .modal(true, |s: &mut UiState<A>| s.nav.settings_more = false)
        .child(crate::controls::menu_item::<A>("settings-more-settings", "Settings", None, false, |s: &mut UiState<A>| {
            s.nav.settings_more = false;
            screens::navigate(s, Screen::Settings);
        }))
        .child(crate::controls::menu_item::<A>("settings-more-shortcuts", "Keyboard shortcuts", None, false, |s: &mut UiState<A>| {
            s.nav.settings_more = false;
            s.nav.shortcuts_open = true;
        }))
        .child(crate::controls::menu_item::<A>("window-zoom-out", "Zoom out", None, false, |s: &mut UiState<A>| crate::shortcuts::zoom(s, -1)))
        .child(row().w_full().px(10.0).center_y()
            .child(text(format!("{}%", app_backend::appearance::zoom_percent(s.appearance.zoom))).font(ts(13.0, 400.0)).color(tone(active_palette().foreground))))
        .child(crate::controls::menu_item::<A>("window-zoom-in", "Zoom in", None, false, |s: &mut UiState<A>| crate::shortcuts::zoom(s, 1)))
        .child(crate::controls::menu_item::<A>("window-zoom-reset", "Reset", None, false, |s: &mut UiState<A>| crate::shortcuts::zoom(s, 0))))
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

    #[test]
    fn rail_lists_the_screens_in_order_and_counts_unread_chats_on_chat() {
        let items = rail_items(2, 3, 4, 1);
        let labels: Vec<_> = items.iter().map(|item| item.label).collect();
        assert_eq!(labels, ["Activity", "Chat", "Teams", "Calendar", "Calls", "Agents"]);
        assert_eq!(items[1].badge, 3);
        assert_eq!(items.iter().map(|item| item.badge).collect::<Vec<_>>(), [2, 3, 4, 0, 0, 1]);
        assert!(items.iter().filter(|item| item.dot).map(|item| item.label).eq(["Teams"]), "only Teams shows a dot");
    }

    #[test]
    fn rail_items_use_the_control_radius() {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Quiet), crate::ui::view::<Quiet>, (1000.0, 700.0), env);
        rt.frame(0.0);
        let nav = rt.elements().into_iter()
            .find(|element| element.key.as_deref() == Some("nav-Chat"))
            .expect("chat rail item");
        assert!((nav.radius - 10.0).abs() < 0.01, "nav item radius 10 as in GPUI");
    }

    #[test]
    fn title_bar_back_button_follows_the_rail() {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Quiet), crate::ui::view::<Quiet>, (1000.0, 700.0), env);
        rt.frame(0.0);
        // A disabled button drops out of the tab order.
        let disabled = |rt: &Runtime<UiState<Quiet>>, key: &str| !rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).expect("title bar button").focusable;
        assert!(disabled(&rt, "history-back"), "nothing to go back to yet");
        let (x, y) = rt.find("nav-Teams").expect("teams rail item");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert!(!disabled(&rt, "history-back"));
        let (x, y) = rt.find("history-back").expect("back button");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.screens.screen, Screen::Chat);
        assert!(disabled(&rt, "history-back") && !disabled(&rt, "history-forward"));
    }

    #[test]
    fn badge_text_hides_zero_and_caps_at_99() {
        assert_eq!(badge_text(0), None);
        assert_eq!(badge_text(7).as_deref(), Some("7"));
        assert_eq!(badge_text(100).as_deref(), Some("99+"));
    }

    fn menu_runtime() -> Runtime<UiState<Quiet>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Quiet), crate::ui::view::<Quiet>, (1000.0, 700.0), env);
        rt.update(|s, _| s.nav.settings_more = true);
        rt.frame(0.0);
        rt
    }

    fn by_key<'a>(elements: &'a [ElementInfo], key: &str) -> &'a ElementInfo {
        elements
            .iter()
            .find(|e| e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("no element {key}"))
    }

    fn a11y_names(node: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(name) = node.get("name").and_then(|v| v.as_str()) {
            out.push(name.to_owned());
        }
        for child in node.get("children").and_then(|c| c.as_array()).into_iter().flatten() {
            a11y_names(child, out);
        }
    }

    #[test]
    fn settings_more_menu_contains_only_unfilled_menu_items() {
        let rt = menu_runtime();
        let elements = rt.elements();
        let root = by_key(&elements, "settings-more-menu");
        let prefix = format!("{}/", root.path);
        let rows: Vec<_> = elements
            .iter()
            .filter(|e| e.path.starts_with(&prefix) && matches!(e.role, "button" | "menuitem"))
            .collect();
        assert!(!rows.is_empty(), "the menu should expose interactive rows");
        for row in rows {
            let key = row.key.as_deref().unwrap_or("?");
            assert_eq!(row.role, "menuitem", "{key} is still a plain button");
            assert!(row.label.is_some(), "{key} has no accessible name");
            assert!(row.focusable, "{key} is not keyboard reachable");
            assert_eq!(row.fill.a, 0.0, "{key} is filled at rest");
            assert!((row.rect[3] - 32.0).abs() < 0.5, "{key} height {}", row.rect[3]);
            assert!((row.radius - 6.0).abs() < 0.5, "{key} radius {}", row.radius);
        }
    }

    #[test]
    fn zoom_rows_name_their_actions_instead_of_bare_glyphs() {
        let rt = menu_runtime();
        let elements = rt.elements();
        for (key, label) in [("window-zoom-out", "Zoom out"), ("window-zoom-in", "Zoom in"), ("window-zoom-reset", "Reset")] {
            let e = by_key(&elements, key);
            assert_eq!(e.role, "menuitem", "{key}");
            assert_eq!(e.label.as_deref(), Some(label), "{key}");
            assert!(e.focusable, "{key}");
        }
        assert!(
            elements.iter().all(|e| e.text.as_deref().map_or(true, |t| t != "−" && t != "+")),
            "bare zoom glyphs must not be the only name in the tree"
        );
        let mut names = Vec::new();
        a11y_names(&rt.accessibility_json(), &mut names);
        assert!(names.iter().any(|name| name == "Zoom out"), "a11y names: {names:?}");
        assert!(names.iter().any(|name| name == "Zoom in"), "a11y names: {names:?}");
    }

    #[test]
    fn clicking_a_rail_item_opens_its_screen() {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Quiet), crate::ui::view::<Quiet>, (1000.0, 700.0), env);
        rt.frame(0.0);
        assert_eq!(rt.state.screens.screen, Screen::Chat);
        // Settings left the rail; the title bar's more menu is the way in.
        rt.update(|s, _| s.nav.settings_more = true);
        rt.frame(1.0 / 60.0);
        let (x, y) = rt.find("settings-more-settings").expect("settings menu item");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.screens.screen, Screen::Settings);
        for (label, screen, content) in [("Teams", Screen::Teams, "teams"), ("Agents", Screen::Agents, "agent-prompt")] {
            let (x, y) = rt.find(&format!("nav-{label}")).expect("rail item");
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(1.0);
            assert_eq!(rt.state.screens.screen, screen);
            assert!(rt.find(content).is_some());
        }
    }
}
