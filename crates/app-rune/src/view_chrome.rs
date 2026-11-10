//! Window chrome as the GPUI shell draws it: the title bar across the top (logo and name, the
//! global search box, the settings and account controls) and the navigation rail down the left
//! (`crates/app/src/views/title_bar.rs`, `rail.rs`). The chat list and the screens stay in `ui`.

use app_backend::schemes::active_palette;
use rune::*;

use crate::avatar::avatar;
use crate::icons::sym;
use crate::screens::{self, Screen};
use crate::types::Actions;
use crate::ui::{UiState, c, ts, INK, MUTED};

/// Height of the title bar the window draws over its caption buttons (`main.rs`).
pub const TITLE_BAR_HEIGHT: f32 = 40.0;
const RAIL_WIDTH: f32 = 60.0;
const RAIL_ITEM_WIDTH: f32 = 48.0;
const RAIL_ITEM_HEIGHT: f32 = 52.0;
const RAIL_ICON: f32 = 20.0;
const SEARCH_WIDTH: f32 = 380.0;
const SEARCH_HEIGHT: f32 = 28.0;
const BADGE_CAP: usize = 99;
const LIST: &str = "#F5F5F5";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RailItem {
    label: &'static str,
    outline: &'static str,
    filled: &'static str,
    screen: Screen,
    badge: usize,
}

fn rail_items(unread_chats: usize) -> Vec<RailItem> {
    vec![
        RailItem { label: "Activity", outline: "notifications", filled: "notifications", screen: Screen::Activity, badge: 0 },
        RailItem { label: "Chat", outline: "chat_bubble", filled: "chat_bubble_fill", screen: Screen::Chat, badge: unread_chats },
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
        .fill(c(LIST))
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
                        .on_press_tx(|s: &mut UiState<A>, _| screens::navigate(s, Screen::Settings))
                        .child(sym("more_horiz", 18.0).color(c(INK))),
                )
                // Placeholder until the bridge exposes the signed-in user and presence.
                .child(
                    row()
                        .key("title-account")
                        .no_window_drag()
                        .size(36.0, 36.0)
                        .radius(18.0)
                        .center()
                        .child(avatar("You", 28.0, None)),
                ),
        )
}

/// The global search box, shown as the GPUI title bar does. Pressing it opens the chat screen,
/// where the search field lives.
fn search_box<A: Actions + 'static>() -> El {
    let palette = active_palette();
    let muted = c(MUTED);
    row()
        .key("title-search")
        .no_window_drag()
        .w(SEARCH_WIDTH)
        .h(SEARCH_HEIGHT)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(6.0)
        .fill(tone(palette.surface))
        .on_press_tx(|s: &mut UiState<A>, _| screens::navigate(s, Screen::Chat))
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
    let mut rail = col().key("rail").w(RAIL_WIDTH).h_full().fill(c(LIST)).items_center().gap(2.0);
    for item in rail_items(unread) {
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
        .radius(10.0)
        .pt(2.0)
        .gap(4.0)
        .items_center()
        .on_press_tx(move |s: &mut UiState<A>, _| screens::navigate(s, screen))
        .child(sym(glyph, RAIL_ICON).color(ink))
        .child(text(item.label).font(ts(9.0, 600.0)).color(ink).truncate());
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
        let items = rail_items(3);
        let labels: Vec<_> = items.iter().map(|item| item.label).collect();
        assert_eq!(labels, ["Activity", "Chat", "Calendar", "Calls", "Settings"]);
        assert_eq!(items[1].badge, 3);
        assert!(items.iter().enumerate().all(|(index, item)| index == 1 || item.badge == 0));
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
    }
}
