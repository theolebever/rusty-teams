//! Settings: sections on the left, the chosen pane on the right, as in the GPUI client.
//! Values persist through `Actions::save_setting` in the places GPUI keeps them: the account
//! for Teams settings, the local database for our own.

use app_backend::appearance::{AppIcon, ThemeChoice};
use app_backend::notify::rules;
use app_backend::notify::sounds::AlertSound;
use app_backend::schemes;
use calling::DeviceChoice;
use rune::*;
use teams_core::{ChatSettingChange, ChatSettingsDocument, EnterKeyBehavior};
use crate::ui::{text, ts};

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
        "#DADDF5" => Some(palette.reaction_on_own),
        "#C4314B" => Some(palette.red),
        _ => None,
    };
    let mapped = value.map(|value| format!("#{value:06X}"));
    rune::color::hex(mapped.as_deref().unwrap_or(hex))
        .expect("valid palette color")
}

/// Everything the panes show besides appearance, loaded from the store by the bridge.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Preferences {
    pub notify: rules::Settings,
    pub auto_start: bool,
    pub open_in_background: bool,
    pub chat: ChatSettingsDocument,
    pub reactions: Vec<String>,
    pub skin_tone: u8,
    pub camera: DeviceChoice,
    pub microphone: DeviceChoice,
    pub speaker: DeviceChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Camera,
    Microphone,
    Speaker,
}

/// One edit from a pane. The view applies it at once; the bridge saves it.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Notify(rules::Settings),
    AutoStart(bool),
    OpenInBackground(bool),
    Chat(ChatSettingChange),
    SkinTone(u8),
    DefaultReactions(Vec<String>),
    Device(Device, DeviceChoice),
}

impl Preferences {
    pub fn apply(&mut self, change: &Change) {
        match change {
            Change::Notify(settings) => self.notify = settings.clone(),
            Change::AutoStart(on) => self.auto_start = *on,
            Change::OpenInBackground(on) => self.open_in_background = *on,
            Change::Chat(change) => { self.chat.apply(*change); }
            Change::SkinTone(tone) => self.skin_tone = *tone,
            Change::DefaultReactions(ids) => self.reactions = ids.clone(),
            Change::Device(Device::Camera, choice) => self.camera = choice.clone(),
            Change::Device(Device::Microphone, choice) => self.microphone = choice.clone(),
            Change::Device(Device::Speaker, choice) => self.speaker = choice.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    General,
    Notifications,
    Appearance,
    Chats,
    Devices,
    Agents,
    About,
}

impl Section {
    const ALL: [Section; 7] = [Self::General, Self::Notifications, Self::Appearance, Self::Chats, Self::Devices, Self::Agents, Self::About];

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Notifications => "Notifications and activity",
            Self::Appearance => "Appearance and accessibility",
            Self::Chats => "Chats and channels",
            Self::Devices => "Devices",
            Self::Agents => "Agents",
            Self::About => "About",
        }
    }

    /// The setting titles on the pane, for the search box (GPUI `settings/nav.rs`).
    fn terms(self) -> &'static [&'static str] {
        match self {
            Self::General => &[AUTO_START, BACKGROUND, CLOSE_TO_TRAY],
            Self::Notifications => &[PREVIEW, FLASH, SOUND, URGENT, CALL_SOUND, BUSY_SOUND, HIDE_IN_CALLS],
            Self::Appearance => &["Theme", "Color scheme", "App icon"],
            Self::Chats => &[RECEIPTS, ENTER, REACTIONS, SKIN_TONE],
            Self::Devices => &["Speaker", "Microphone", "Camera"],
            Self::Agents => &["Agent", "Action", "Resume interrupted runs"],
            Self::About => &["Version", "Build"],
        }
    }

    fn matches(self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty() || std::iter::once(self.label()).chain(self.terms().iter().copied()).any(|term| term.to_lowercase().contains(&query))
    }
}

const AUTO_START: &str = "Auto-start Teams";
const BACKGROUND: &str = "Open application in background";
const CLOSE_TO_TRAY: &str = "On close, keep the application running";
const PREVIEW: &str = "Show message preview";
const FLASH: &str = "Flash taskbar when notifications are received";
const SOUND: &str = "Play sounds with notifications";
const URGENT: &str = "Play sounds with urgent and priority contact notifications";
const CALL_SOUND: &str = "Play sounds with incoming calls, requests to join, and meeting start notifications";
const BUSY_SOUND: &str = "Play sounds when I'm busy or in a meeting";
const HIDE_IN_CALLS: &str = "Hide notifications during calls and meetings";
const RECEIPTS: &str = "Read receipts";
const ENTER: &str = "When writing a message, press Enter to";
const REACTIONS: &str = "Default reactions";
const SKIN_TONE: &str = "Skin tone";
/// Reactions offered for the quick bar; Teams keeps up to `DEFAULT_REACTION_SLOTS`.
const REACTION_CHOICES: [&str; 8] = ["like", "heart", "laugh", "surprised", "sad", "angry", "yes", "clap"];

#[derive(Default)]
pub struct State {
    pub section: Section,
    pub search: TextEdit,
    pub prefs: Preferences,
    pub(crate) agent_form: crate::agent_settings::Form,
}

/// Applies `change` to the view and hands it to the bridge to save.
pub fn change<A: Actions>(s: &mut UiState<A>, change: Change) {
    s.settings.prefs.apply(&change);
    match &change {
        Change::Device(Device::Camera, choice) => s.calls.selected_camera = choice.clone(),
        Change::Device(Device::Microphone, choice) => s.calls.selected_input = choice.clone(),
        Change::Device(Device::Speaker, choice) => s.calls.selected_output = choice.clone(),
        _ => {}
    }
    s.actions.save_setting(change);
}

pub(crate) fn notify<A: Actions>(s: &mut UiState<A>, edit: impl FnOnce(&mut rules::Settings)) {
    let mut settings = s.settings.prefs.notify.clone();
    edit(&mut settings);
    change(s, Change::Notify(settings));
}

pub(crate) fn caption(label: &str) -> El {
    text(label.to_owned()).font(ts(12.0, 600.0)).color(color("#616161")).pt(16.0)
}

fn switch(on: bool) -> El {
    let knob = el().w(14.0).h(14.0).radius(7.0).fill(color(if on { "#FFFFFF" } else { "#616161" }));
    let track = row().w(40.0).h(20.0).radius(10.0).px(3.0).center_y();
    let track = if on { track.fill(color("#5B5FC7")).child(el().grow(1.0)) } else { track.border(1.0, color("#616161")) };
    track.child(knob)
}

/// A setting row with a switch on the right; the whole row toggles.
pub(crate) fn toggle<A: Actions + 'static>(key: &str, title: &str, on: bool, enabled: bool, press: impl Fn(&mut UiState<A>) + 'static) -> El {
    let row = row().key(key.to_owned()).w_full().min_h(40.0).px(12.0).gap(12.0).center_y().radius(6.0)
        .child(text(title.to_owned()).font(ts(14.0, 400.0)).color(color("#242424")).wrap().grow(1.0).min_w(0.0))
        .child(switch(on));
    if enabled { row.hover(color("#EBEBEB")).on_press(press) } else { row.opacity(0.4) }
}

/// A radio row in a choice list.
pub(crate) fn choice<A: Actions + 'static>(key: String, label: &str, selected: bool, press: impl Fn(&mut UiState<A>) + 'static) -> El {
    row().key(key).w_full().h(34.0).px(12.0).gap(10.0).center_y().radius(6.0).hover(color("#EBEBEB")).on_press(press)
        .child(text(if selected { "●" } else { "○" }).color(color(if selected { "#5B5FC7" } else { "#616161" })))
        .child(text(label.to_owned()).font(ts(14.0, 400.0)).color(color("#242424")).truncate())
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    let query = s.settings.search.text.clone();
    let mut nav = col().key("settings-nav").w(260.0).h_full().padding(12.0).gap(2.0).fill(color("#F5F5F5"))
        .child(text("Settings").font(ts(20.0, 600.0)).color(color("#242424")).px(12.0).pb(12.0))
        .child(text_input(&s.settings.search, ts(14.0, 400.0)).key("settings-search").label("Search settings")
            .placeholder("Search settings", color("#616161")).color(color("#242424")).w_full()
            .on_edit(|s: &mut UiState<A>| &mut s.settings.search)
            .h(32.0).px(10.0).radius(6.0).fill(color("#FFFFFF")).mb(8.0));
    let shown: Vec<Section> = Section::ALL.into_iter().filter(|section| section.matches(&query)).collect();
    if shown.is_empty() {
        nav = nav.child(text("No settings found").font(ts(13.0, 400.0)).color(color("#616161")).px(12.0));
    }
    for section in &shown {
        let section = *section;
        let active = section == s.settings.section;
        let item = row().key(format!("settings-nav-{section:?}")).w_full().h(32.0).px(12.0).center_y().radius(6.0)
            .on_press(move |s: &mut UiState<A>| select(s, section))
            .child(text(section.label()).font(ts(14.0, if active { 600.0 } else { 400.0 })).color(color("#242424")).truncate());
        nav = nav.child(if active { item.fill(color("#EBEBEB")) } else { item.hover(color("#EBEBEB")) });
    }
    // A search that hides the open section shows its first match instead.
    let section = if shown.contains(&s.settings.section) { s.settings.section } else { shown.first().copied().unwrap_or(s.settings.section) };
    let pane = match section {
        Section::General => general(s),
        Section::Notifications => notifications(s),
        Section::Appearance => appearance(s),
        Section::Chats => chats(s),
        Section::Devices => devices(s),
        Section::Agents => crate::agent_settings::pane(s),
        Section::About => about(),
    };
    row().key("settings").w_full().h_full().fill(color("#FFFFFF"))
        .child(nav)
        .child(col().key("settings-pane").grow(1.0).min_w(0.0).h_full().scroll(ScrollSpec { stick_bottom: false })
            .child(col().w_full().max_w(720.0).padding(24.0).gap(4.0)
                .child(text(section.label()).font(ts(18.0, 600.0)).color(color("#242424")).pb(8.0))
                .child(pane)))
}

fn select<A: Actions>(s: &mut UiState<A>, section: Section) {
    s.settings.section = section;
    if section == Section::Devices { s.actions.load_devices(); }
}

fn general<A: Actions + 'static>(s: &UiState<A>) -> El {
    let prefs = &s.settings.prefs;
    let (auto_start, background, close_to_tray) = (prefs.auto_start, prefs.open_in_background, prefs.notify.close_to_tray);
    col().w_full()
        .child(caption("SYSTEM"))
        .child(toggle("settings-auto-start", AUTO_START, auto_start, true, move |s: &mut UiState<A>| change(s, Change::AutoStart(!auto_start))))
        .child(toggle("settings-open-in-background", BACKGROUND, background, auto_start, move |s: &mut UiState<A>| change(s, Change::OpenInBackground(!background))))
        .child(toggle("settings-close-to-tray", CLOSE_TO_TRAY, close_to_tray, true, |s: &mut UiState<A>| notify(s, |n| n.close_to_tray = !n.close_to_tray)))
}

fn sound_list<A: Actions + 'static>(key: &'static str, sounds: &[AlertSound], current: AlertSound, field: fn(&mut rules::Settings) -> &mut AlertSound) -> El {
    let mut list = col().w_full().pl(24.0);
    for sound in sounds.iter().copied() {
        list = list.child(choice(format!("{key}-{}", sound.name()), sound.label(), sound == current, move |s: &mut UiState<A>| {
            notify(s, |n| *field(n) = sound);
            crate::sound::play(sound.file_name());
        }));
    }
    list
}

fn notifications<A: Actions + 'static>(s: &UiState<A>) -> El {
    let prefs = &s.settings.prefs;
    let n = &prefs.notify;
    let preview = prefs.chat.settings().notifications.show_preview;
    let sounds = &n.sounds;
    let mut pane = col().w_full()
        .child(caption("APPEARANCE AND SOUND"))
        .child(toggle("settings-preview", PREVIEW, preview, true, move |s: &mut UiState<A>| change(s, Change::Chat(ChatSettingChange::ShowPreview(!preview)))))
        .child(toggle("settings-flash", FLASH, n.flash, true, |s: &mut UiState<A>| notify(s, |n| n.flash = !n.flash)))
        .child(caption("SOUND"))
        .child(toggle("settings-notification-sound", SOUND, sounds.notification_sound, true, |s: &mut UiState<A>| notify(s, |n| n.sounds.notification_sound = !n.sounds.notification_sound)));
    if sounds.notification_sound {
        pane = pane.child(sound_list::<A>("settings-sound", &AlertSound::NORMAL, sounds.notification_sound_type, |n| &mut n.sounds.notification_sound_type));
    }
    pane = pane.child(toggle("settings-urgent-sound", URGENT, sounds.urgent_sound, true, |s: &mut UiState<A>| notify(s, |n| n.sounds.urgent_sound = !n.sounds.urgent_sound)));
    if sounds.urgent_sound {
        pane = pane.child(sound_list::<A>("settings-urgent", &AlertSound::URGENT, sounds.urgent_sound_type, |n| &mut n.sounds.urgent_sound_type));
    }
    pane.child(toggle("settings-call-sound", CALL_SOUND, sounds.call_and_meeting_sound, true, |s: &mut UiState<A>| notify(s, |n| n.sounds.call_and_meeting_sound = !n.sounds.call_and_meeting_sound)))
        .child(toggle("settings-busy-sound", BUSY_SOUND, sounds.sound_when_busy, true, |s: &mut UiState<A>| notify(s, |n| n.sounds.sound_when_busy = !n.sounds.sound_when_busy)))
        .child(caption("CALLS AND MEETINGS"))
        .child(toggle("settings-hide-in-calls", HIDE_IN_CALLS, n.mute_in_calls, true, |s: &mut UiState<A>| notify(s, |n| n.mute_in_calls = !n.mute_in_calls)))
}

fn appearance<A: Actions + 'static>(s: &UiState<A>) -> El {
    let current = s.appearance;
    let mut pane = col().w_full().child(caption("THEME"));
    for theme in ThemeChoice::ALL {
        pane = pane.child(choice(format!("theme-{}", theme.key()), theme.label(), current.theme == theme, move |s: &mut UiState<A>| {
            s.appearance.theme = theme;
            s.actions.set_appearance(s.appearance);
        }));
    }
    pane = pane.child(caption("COLOR SCHEME"));
    for scheme in schemes::SCHEMES {
        pane = pane.child(choice(format!("settings-color-scheme-{}", scheme.id), scheme.name, current.color_scheme.id == scheme.id, move |s: &mut UiState<A>| {
            s.appearance.color_scheme = scheme;
            s.actions.set_appearance(s.appearance);
        }));
    }
    pane = pane.child(caption("APP ICON"));
    for icon in AppIcon::ALL {
        pane = pane.child(choice(format!("settings-app-icon-{}", icon.key()), icon.label(), current.app_icon == icon, move |s: &mut UiState<A>| {
            s.appearance.app_icon = icon;
            s.actions.set_appearance(s.appearance);
        }));
    }
    pane
}

fn chats<A: Actions + 'static>(s: &UiState<A>) -> El {
    let prefs = &s.settings.prefs;
    let account = prefs.chat.settings();
    let receipts = account.read_receipts;
    let mut pane = col().w_full()
        .child(caption("CHATS"))
        .child(toggle("settings-read-receipts", RECEIPTS, receipts, true, move |s: &mut UiState<A>| change(s, Change::Chat(ChatSettingChange::ReadReceipts(!receipts)))))
        .child(text(ENTER).font(ts(14.0, 400.0)).color(color("#242424")).px(12.0).pt(8.0));
    for (label, behavior) in [("Send the message", EnterKeyBehavior::Send), ("Start a new line (to send, select Send or use Ctrl+Enter).", EnterKeyBehavior::NewLine)] {
        pane = pane.child(choice(format!("settings-enter-{behavior:?}"), label, account.enter_key == behavior, move |s: &mut UiState<A>| {
            change(s, Change::Chat(ChatSettingChange::EnterKey(behavior)));
        }));
    }
    pane = pane.child(caption(&REACTIONS.to_uppercase()));
    let mut reactions = row().w_full().wrap().gap(6.0).px(12.0);
    for id in REACTION_CHOICES {
        let on = prefs.reactions.iter().any(|chosen| chosen == id);
        let glyph = chatsvc::reaction_glyph_for_key(id).unwrap_or_else(|| id.to_owned());
        let chip = row().key(format!("settings-reaction-{id}")).h(32.0).px(10.0).center().radius(6.0)
            .on_press(move |s: &mut UiState<A>| {
                let mut ids = s.settings.prefs.reactions.clone();
                if on { ids.retain(|chosen| chosen != id); } else if ids.len() < teams_core::DEFAULT_REACTION_SLOTS { ids.push(id.to_owned()); } else { return; }
                change(s, Change::DefaultReactions(ids));
            })
            .child(text(glyph).font(ts(16.0, 400.0)));
        reactions = reactions.child(if on { chip.fill(color("#E8EBFA")).border(1.0, color("#5B5FC7")) } else { chip.fill(color("#F5F5F5")).hover(color("#EBEBEB")) });
    }
    pane = pane.child(reactions).child(caption(&SKIN_TONE.to_uppercase()));
    let mut tones = row().w_full().gap(6.0).px(12.0);
    for tone in 0..=5u8 {
        let glyph = chatsvc::reaction_glyph_for_key(&if tone == 0 { "like".to_owned() } else { format!("like-tone{tone}") }).unwrap_or_else(|| tone.to_string());
        let chip = row().key(format!("settings-skin-tone-{tone}")).h(32.0).w(36.0).center().radius(6.0)
            .on_press(move |s: &mut UiState<A>| change(s, Change::SkinTone(tone)))
            .child(text(glyph).font(ts(16.0, 400.0)));
        tones = tones.child(if prefs.skin_tone == tone { chip.fill(color("#E8EBFA")).border(1.0, color("#5B5FC7")) } else { chip.hover(color("#EBEBEB")) });
    }
    pane.child(tones)
}

fn devices<A: Actions + 'static>(s: &UiState<A>) -> El {
    let prefs = &s.settings.prefs;
    let mut pane = col().w_full();
    for (device, title, entries, selected) in [
        (Device::Speaker, "Speaker", s.calls.devices.output_entries(), &prefs.speaker),
        (Device::Microphone, "Microphone", s.calls.devices.input_entries(), &prefs.microphone),
        (Device::Camera, "Camera", calling::camera_entries(&s.calls.cameras), &prefs.camera),
    ] {
        pane = pane.child(caption(&title.to_uppercase()));
        for entry in entries {
            let pick = entry.choice.clone();
            pane = pane.child(choice(format!("settings-{title}-{}", entry.label), &entry.label, &entry.choice == selected, move |s: &mut UiState<A>| {
                change(s, Change::Device(device, pick.clone()));
            }));
        }
    }
    pane
}

fn about() -> El {
    col().w_full()
        .child(caption("RUSTY TEAMS"))
        .child(text(format!("Version {}", env!("CARGO_PKG_VERSION"))).font(ts(14.0, 400.0)).color(color("#242424")).px(12.0).pt(8.0))
        .child(text(format!("Build {}", app_backend::version::build_version())).font(ts(14.0, 400.0)).color(color("#616161")).px(12.0).pt(4.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake { saved: RefCell<Vec<Change>>, devices: RefCell<usize> }
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn save_setting(&self, change: Change) { self.saved.borrow_mut().push(change); }
        fn load_devices(&self) { *self.devices.borrow_mut() += 1; }
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("{key} is shown"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn panes_save_general_notification_chat_and_device_settings() {
        let mut state = UiState::new(Fake::default());
        state.screens.screen = crate::screens::Screen::Settings;
        let mut rt = Runtime::new(state, crate::ui::view::<Fake>, (1100.0, 800.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(1.0);
        click(&mut rt, "settings-auto-start");
        click(&mut rt, "settings-close-to-tray");
        assert!(rt.state.settings.prefs.auto_start);
        assert!(!rt.state.settings.prefs.notify.close_to_tray);
        click(&mut rt, "settings-nav-Notifications");
        click(&mut rt, "settings-flash");
        assert!(!rt.state.settings.prefs.notify.flash);
        click(&mut rt, "settings-nav-Chats");
        click(&mut rt, "settings-enter-NewLine");
        assert_eq!(rt.state.settings.prefs.chat.settings().enter_key, EnterKeyBehavior::NewLine);
        click(&mut rt, "settings-skin-tone-3");
        click(&mut rt, "settings-nav-Devices");
        assert_eq!(*rt.state.actions.devices.borrow(), 1);
        click(&mut rt, "settings-Camera-Default camera");
        let saved = rt.state.actions.saved.borrow();
        assert!(matches!(saved[0], Change::AutoStart(true)));
        assert!(matches!(&saved[1], Change::Notify(n) if !n.close_to_tray));
        assert!(matches!(&saved[2], Change::Notify(n) if !n.flash));
        assert_eq!(saved[3], Change::Chat(ChatSettingChange::EnterKey(EnterKeyBehavior::NewLine)));
        assert_eq!(saved[4], Change::SkinTone(3));
        assert_eq!(saved[5], Change::Device(Device::Camera, DeviceChoice::SystemDefault));
    }

    #[test]
    fn search_narrows_sections_and_opens_the_first_match() {
        let mut state = UiState::new(Fake::default());
        state.screens.screen = crate::screens::Screen::Settings;
        let mut rt = Runtime::new(state, crate::ui::view::<Fake>, (1100.0, 800.0), Env { system_dark: false, scale: 1.0 });
        rt.update(|s, _| s.settings.search = TextEdit::new("receipts"));
        rt.frame(1.0);
        assert!(rt.find("settings-nav-General").is_none());
        assert!(rt.find("settings-read-receipts").is_some());
        assert!(Section::Devices.matches("MICRO"));
        assert!(!Section::Appearance.matches("receipts"));
    }
}
