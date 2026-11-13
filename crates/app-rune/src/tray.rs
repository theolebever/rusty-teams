//! The tray icon and its menu, as in the GPUI client (`app_base::notify::platform::Tray`):
//! Open, Mute sound, Do not disturb, Notifications settings, Quit; a click shows or hides the
//! window, and a red dot marks an unread @mention.

use std::collections::HashSet;
use std::sync::Arc;

use app_backend::appearance::AppIcon;
use rune::{AppHandle, Tray, TrayEvent, TrayIcon, TrayItem, TraySpec};

use crate::types::Actions;
use crate::ui::UiState;

const ID: &str = "rusty-teams-rune";
const TITLE: &str = "Rusty Teams";
const SIZE: u32 = 32;
const DOT_RADIUS: f32 = 6.0;

type Handler = Arc<dyn Fn(TrayEvent) + Send + Sync>;

/// What the icon and menu show; the tray is rebuilt when it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shown {
    sound: bool,
    do_not_disturb: bool,
    mention: bool,
    icon: AppIcon,
}

#[derive(Default)]
pub struct State {
    tray: Option<(Tray, Handler)>,
    shown: Option<Shown>,
    /// Chats with an @mention that arrived while they were not in front (GPUI `unread_mentions`).
    pub mentions: HashSet<String>,
}

fn icon(app_icon: AppIcon, mention: bool) -> Option<TrayIcon> {
    let mut image = ::image::load_from_memory(app_icon.png()).ok()?
        .resize_exact(SIZE, SIZE, ::image::imageops::FilterType::Lanczos3).to_rgba8();
    if mention {
        let center = SIZE as f32 - DOT_RADIUS - 1.0;
        for (column, row, pixel) in image.enumerate_pixels_mut() {
            if (column as f32 - center).hypot(row as f32 - center) <= DOT_RADIUS { *pixel = ::image::Rgba([0xef, 0x44, 0x44, 0xff]); }
        }
    }
    Some(TrayIcon::rgba(SIZE, SIZE, image.into_raw()))
}

fn menu(shown: Shown) -> Vec<TrayItem> {
    let check = |on: bool, label: &str| format!("{}{label}", if on { "✓ " } else { "" });
    vec![
        TrayItem::action("open", "Open"),
        TrayItem::Separator,
        TrayItem::action("sound", check(!shown.sound, "Mute sound")),
        TrayItem::action("dnd", check(shown.do_not_disturb, "Do not disturb")),
        TrayItem::action("settings", "Notifications"),
        TrayItem::Separator,
        TrayItem::action("quit", "Quit"),
    ]
}

fn create(shown: Shown, handler: &Handler) -> Option<Tray> {
    let handler = handler.clone();
    let spec = TraySpec::new(ID, TITLE, icon(shown.icon, shown.mention)?).tooltip(TITLE).menu(menu(shown));
    Tray::new(spec, move |event| handler(event))
        .map_err(|error| app_backend::diagnostics::record(format!("System tray unavailable: {error}")))
        .ok()
}

fn toggle_sound<A: Actions>(s: &mut UiState<A>) {
    let mut settings = s.settings.prefs.notify.clone();
    settings.sounds.notification_sound = !settings.sounds.notification_sound;
    crate::settings::change(s, crate::settings::Change::Notify(settings));
}

fn toggle_do_not_disturb<A: Actions>(s: &mut UiState<A>) {
    let mut settings = s.settings.prefs.notify.clone();
    settings.do_not_disturb = !settings.do_not_disturb;
    crate::settings::change(s, crate::settings::Change::Notify(settings));
}

/// Puts the tray up for the app behind `handle`. False when the system has no tray.
pub fn install<A: Actions + 'static>(s: &mut UiState<A>, handle: AppHandle<UiState<A>>) -> bool {
    let handler: Handler = Arc::new(move |event| match event {
        TrayEvent::Activate { .. } if rune_notify::app_focused() => handle.hide(),
        TrayEvent::Activate { .. } => handle.open(),
        TrayEvent::Menu(id) => match id.as_str() {
            "open" => handle.open(),
            "sound" => handle.update(|s, _| toggle_sound(s)),
            "dnd" => handle.update(|s, _| toggle_do_not_disturb(s)),
            "settings" => {
                handle.open();
                handle.update(|s, _| {
                    crate::screens::navigate(s, crate::screens::Screen::Settings);
                    s.settings.section = crate::settings::Section::Notifications;
                });
            }
            "quit" => handle.quit(),
            _ => {}
        },
        _ => {}
    });
    let shown = shown(s);
    let Some(tray) = create(shown, &handler) else { return false };
    s.tray = State { tray: Some((tray, handler)), shown: Some(shown), mentions: std::mem::take(&mut s.tray.mentions) };
    true
}

fn shown<A: Actions>(s: &UiState<A>) -> Shown {
    let notify = &s.settings.prefs.notify;
    Shown { sound: notify.sounds.notification_sound, do_not_disturb: notify.do_not_disturb, mention: !s.tray.mentions.is_empty(), icon: s.appearance.app_icon }
}

/// Drops mentions that were read or are in front, then redraws the tray if anything changed.
pub fn sync<A: Actions>(s: &mut UiState<A>) {
    let foreground = s.foreground_chat().map(str::to_owned);
    let (chats, teams) = (&s.chats, &s.teams);
    let unread = |id: &str| chats.iter().any(|chat| chat.unread && chat.id == id)
        || teams.iter().flat_map(|team| &team.channels).any(|channel| channel.unread && channel.id == id);
    s.tray.mentions.retain(|id| unread(id) && foreground.as_deref() != Some(id.as_str()));
    let shown = shown(s);
    if s.tray.shown == Some(shown) { return; }
    let previous = s.tray.shown.replace(shown);
    let Some((tray, handler)) = &mut s.tray.tray else { return };
    // ponytail: Rune's tray cannot swap its icon, so a new dot or app icon rebuilds the tray.
    if previous.is_some_and(|previous| previous.mention == shown.mention && previous.icon == shown.icon) {
        tray.set_menu(menu(shown));
    } else if let Some(rebuilt) = create(shown, handler) {
        *tray = rebuilt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_checks_mute_and_do_not_disturb_and_the_dot_marks_a_mention() {
        let shown = Shown { sound: false, do_not_disturb: true, mention: false, icon: AppIcon::RustyTeams };
        let labels: Vec<String> = menu(shown).into_iter().filter_map(|item| match item { TrayItem::Action { label, .. } => Some(label), _ => None }).collect();
        assert_eq!(labels, ["Open", "✓ Mute sound", "✓ Do not disturb", "Notifications", "Quit"]);
        let plain = icon(AppIcon::RustyTeams, false).unwrap();
        assert_ne!(icon(AppIcon::RustyTeams, true).unwrap(), plain);
    }

    #[test]
    fn a_mention_is_kept_until_its_chat_is_read_or_opened() {
        struct Quiet;
        impl Actions for Quiet {
            fn open(&self, _: &str) {}
            fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        }
        let mut s = UiState::new(Quiet);
        s.chats = vec![crate::types::ChatRow { id: "a".into(), title: "A".into(), preview: String::new(), unread: true, when: String::new() }];
        s.tray.mentions.insert("a".into());
        sync(&mut s);
        assert!(shown(&s).mention);
        s.chats[0].unread = false;
        sync(&mut s);
        assert!(!shown(&s).mention);
        let channel = crate::types::ChannelRow { hidden: false, id: "c".into(), name: "General".into(), unread: true };
        s.teams = vec![crate::types::TeamRow { id: "t".into(), name: "T".into(), channels: vec![channel] }];
        s.tray.mentions.insert("c".into());
        sync(&mut s);
        assert!(shown(&s).mention, "an unread channel keeps its mention");
    }
}
