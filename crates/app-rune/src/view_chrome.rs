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
pub const TITLE_BAR_HEIGHT: f32 = 48.0;
const RAIL_WIDTH: f32 = 68.0;
const RAIL_ITEM_WIDTH: f32 = 56.0;
const RAIL_ITEM_HEIGHT: f32 = 64.0;
const RAIL_ICON: f32 = 24.0;
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
}

fn rail_items(activity: usize, unread_chats: usize, channels: usize, agents: usize) -> Vec<RailItem> {
    vec![
        RailItem { label: "Activity", outline: "notifications", filled: "notifications", screen: Screen::Activity, badge: activity },
        RailItem { label: "Chat", outline: "chat_bubble", filled: "chat_bubble_fill", screen: Screen::Chat, badge: unread_chats },
        RailItem { label: "Teams", outline: "group", filled: "group", screen: Screen::Teams, badge: channels },
        RailItem { label: "Agents", outline: "auto_fix_high", filled: "auto_fix_high", screen: Screen::Agents, badge: agents },
        RailItem { label: "Calendar", outline: "calendar_month", filled: "calendar_month", screen: Screen::Calendar, badge: 0 },
        RailItem { label: "Calls", outline: "call", filled: "call", screen: Screen::Calls, badge: 0 },
        RailItem { label: "Settings", outline: "settings", filled: "settings", screen: Screen::Settings, badge: 0 },
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
                .gap(12.0)
                .pl(14.0)
                .child(el().key("title-logo").size(18.0, 18.0).pixels_fit(s.media.logo().pixels(), SurfaceFit::Fit))
                .child(text(crate::TITLE).font(ts(13.0, 600.0)).color(c(INK))),
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
        .child(sym("search", 14.0).color(muted))
        .child(
            text("Search chats, people and channels")
                .font(ts(12.0, 400.0))
                .color(muted)
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(
            row()
                .key("title-search-shortcut")
                .px(6.0)
                .py(1.0)
                .radius(4.0)
                .fill(tone(palette.surface_raised))
                .child(text("Ctrl K").font(ts(11.0, 400.0)).color(c(INK))),
        )
}

/// The navigation rail down the left: icon over label, the selected screen on a raised pill.
pub fn rail<A: Actions + 'static>(s: &UiState<A>) -> El {
    let unread = s.chats.iter().filter(|chat| chat.unread).count();
    let mut rail = col().key("rail").w(RAIL_WIDTH).h_full().fill(tone(active_palette().surface)).items_center().gap(2.0);
    let activity = &s.screens.activity;
    let activity_unread = activity.items.iter().filter(|item| activity.counts_for_bell(item)).count();
    let channels = s.teams.iter().flat_map(|team| &team.channels).filter(|channel| channel.unread && !channel.hidden).count();
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
        .radius(4.0)
        .pt(2.0)
        .gap(4.0)
        .items_center()
        .on_press_tx(move |s: &mut UiState<A>, tx| {
            screens::navigate(s, screen);
            if screen == Screen::Calendar { tx.scroll_to_instant("calendar-hours", 0.0, 7.0 * 48.0); }
        })
        .child(sym(glyph, RAIL_ICON).color(ink))
        .label(item.label)
        .help(format!("{} ({})", item.label, shortcut(screen)))
        .child(text(item.label).font(ts(11.0, if active { 600.0 } else { 400.0 })).color(ink).truncate());
    button = if active { button.fill(tone(palette.surface_raised)) } else { button.hover(tone(palette.row_hover)) };
    if let Some(count) = badge_text(item.badge) {
        let badge = row()
            .key(format!("nav-{}-badge", item.label))
            .h(16.0)
            .min_w(16.0)
            .px(4.0)
            .radius(8.0)
            .center()
            .fill(tone(palette.accent))
            .child(text(count).font(ts(10.0, 700.0)).color(tone(palette.on_accent)));
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
        .child(screens::button("Settings", "settings-more-settings", |s: &mut UiState<A>, _| {
            s.nav.settings_more = false;
            screens::navigate(s, Screen::Settings);
        }))
        .child(screens::button("Keyboard shortcuts", "settings-more-shortcuts", |s: &mut UiState<A>, _| {
            s.nav.settings_more = false;
            s.nav.shortcuts_open = true;
        }))
        .child(row().w_full().gap(6.0).center_y()
            .child(crate::screens::button("−", "window-zoom-out", |s: &mut UiState<A>, _| crate::shortcuts::zoom(s, -1)).label("Zoom out"))
            .child(text(format!("{}%", app_backend::appearance::zoom_percent(s.appearance.zoom))).font(ts(13.0, 400.0)).color(tone(active_palette().foreground)))
            .child(crate::screens::button("+", "window-zoom-in", |s: &mut UiState<A>, _| crate::shortcuts::zoom(s, 1)).label("Zoom in"))
            .child(crate::screens::button("Reset", "window-zoom-reset", |s: &mut UiState<A>, _| crate::shortcuts::zoom(s, 0)))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Actions, SendTarget};
    use crate::ui::UiState;
    use rune::{Env, Runtime};

    struct Quiet;

    impl Actions for Quiet {
        fn open(&self, _conversation_id: &str) {}
        fn send(&self, _target: SendTarget, _text: String, _files: Vec<teams_core::FileReference>) {}
    }

    #[test]
    fn rail_lists_the_screens_in_order_and_counts_unread_chats_on_chat() {
        let items = rail_items(2, 3, 4, 1);
        let labels: Vec<_> = items.iter().map(|item| item.label).collect();
        assert_eq!(labels, ["Activity", "Chat", "Teams", "Agents", "Calendar", "Calls", "Settings"]);
        assert_eq!(items[1].badge, 3);
        assert_eq!(items.iter().map(|item| item.badge).collect::<Vec<_>>(), [2, 3, 4, 1, 0, 0, 0]);
    }

    #[test]
    fn badge_text_hides_zero_and_caps_at_99() {
        assert_eq!(badge_text(0), None);
        assert_eq!(badge_text(7).as_deref(), Some("7"));
        assert_eq!(badge_text(100).as_deref(), Some("99+"));
    }

    #[test]
    fn clicking_a_rail_item_opens_its_screen() {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Quiet), crate::ui::view::<Quiet>, (1000.0, 700.0), env);
        rt.frame(0.0);
        assert_eq!(rt.state.screens.screen, Screen::Chat);
        let (x, y) = rt.find("nav-Settings").expect("settings rail item");
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
