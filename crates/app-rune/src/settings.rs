//! Settings: sections on the left, the chosen pane on the right, as in the GPUI client.
//! Values persist through `Actions::save_setting` in the places GPUI keeps them: the account
//! for Teams settings, the local database for our own.

use app_backend::appearance::{AppIcon, ThemeChoice};
use app_backend::notify::rules;
use app_backend::notify::sounds::AlertSound;
use app_backend::schemes;
use app_base::ui_locale::tr;
use calling::{CameraEffects, DeviceChoice};
use rune::*;
use teams_core::{ChatSettingChange, ChatSettingsDocument, EnterKeyBehavior};
use crate::controls::{self, Size, Variant};
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
        // Find's match highlight is a tinted row wash, not a fixed yellow that would be
        // illegible in dark mode. `selection` is the role for a highlighted row; its
        // `#rrggbbaa` value is decoded by `rune::color::hex` with the intended alpha.
        "#FFF4CE" => Some(palette.selection),
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
    pub markdown: bool,
    pub actions_on_right_click: bool,
    pub camera: DeviceChoice,
    pub microphone: DeviceChoice,
    pub speaker: DeviceChoice,
    pub effects: CameraEffects,
    pub translation: Option<teams_core::LanguageSettings>,
    pub languages: Vec<teams_core::Language>,
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
    Markdown(bool),
    ActionsOnRightClick(bool),
    MutedSection(bool),
    Translation(serde_json::Value),
    Device(Device, DeviceChoice),
    Effects(CameraEffects),
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
            Change::Markdown(on) => self.markdown = *on,
            Change::ActionsOnRightClick(on) => self.actions_on_right_click = *on,
            Change::MutedSection(_) => {},
            Change::Translation(_) => {},
            Change::Device(Device::Camera, choice) => self.camera = choice.clone(),
            Change::Device(Device::Microphone, choice) => self.microphone = choice.clone(),
            Change::Device(Device::Speaker, choice) => self.speaker = choice.clone(),
            Change::Effects(effects) => self.effects = *effects,
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
    Calls,
    Agents,
    About,
}

impl Section {
    /// The navigation groups in GPUI's order (`settings/nav.rs`): Teams settings, then the
    /// "Rusty Teams" group, then About pinned at the bottom.
    const TEAMS: [Section; 6] = [Self::General, Self::Notifications, Self::Appearance, Self::Chats, Self::Calls, Self::Devices];
    const RUSTY: [Section; 1] = [Self::Agents];
    const ALL: [Section; 8] = [Self::General, Self::Notifications, Self::Appearance, Self::Chats, Self::Calls, Self::Devices, Self::Agents, Self::About];

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Notifications => "Notifications and activity",
            Self::Appearance => "Appearance and accessibility",
            Self::Chats => "Chats and channels",
            Self::Devices => "Devices",
            Self::Calls => "Calls",
            Self::Agents => "Agents",
            Self::About => "About",
        }
    }

    /// The setting titles on the pane, for the search box (GPUI `settings/nav.rs`).
    fn terms(self) -> &'static [&'static str] {
        match self {
            Self::General => &[AUTO_START, BACKGROUND, SHOW_IN_TRAY, CLOSE_TO_TRAY, "Up arrow in compose box", "Edit last message", "Navigate to messages", "Stay in compose", "Translation", "Language"],
            Self::Notifications => &[PREVIEW, FLASH, SOUND, URGENT, CALL_SOUND, BUSY_SOUND, HIDE_IN_CALLS],
            Self::Appearance => &["Theme", "Color scheme", "App icon"],
            Self::Chats => &[RECEIPTS, ENTER, REACTIONS, SKIN_TONE, "Message density", "Compact", "Comfy", "Show message actions on right click", "Muted items"],
            Self::Devices => &["Speaker", "Microphone", "Camera", "Mirror my video", "Auto framing", "Adjust brightness", "Soft focus"],
            Self::Calls => &["Ringtones", "Forwarded calls", "Delegated calls", "Call queue"],
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
const SHOW_IN_TRAY: &str = "Show icon in the tray";
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
        Change::Effects(effects) => {
            s.calls.effects = *effects;
            if s.calls.state.is_some() {
                s.actions.call_command(calling::CallCommand::SetCameraEffects(*effects));
            }
        }
        _ => {}
    }
    s.actions.save_setting(change);
}

pub(crate) fn notify<A: Actions>(s: &mut UiState<A>, edit: impl FnOnce(&mut rules::Settings)) {
    let mut settings = s.settings.prefs.notify.clone();
    edit(&mut settings);
    change(s, Change::Notify(settings));
}

/// Section captions are sentence case across every pane. Callers that still pass an all-caps
/// legacy string (translation and agent panes) are normalised here so the look stays consistent.
pub(crate) fn caption(label: &str) -> El {
    text(sentence_case(label)).font(ts(12.0, 600.0)).color(color("#616161")).pt(16.0)
}

/// Lowercases an all-caps label and capitalises only its first letter; anything else is kept.
fn sentence_case(label: &str) -> String {
    if label.is_empty() || !label.chars().all(|c| !c.is_lowercase() && (c.is_uppercase() || c.is_whitespace())) {
        return label.to_owned();
    }
    let lower = label.to_lowercase();
    let mut chars = lower.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().collect::<String>() + chars.as_str())
}

/// The muted help line under a setting row.
fn detail(help: &str) -> El {
    text(help.to_owned()).font(ts(12.0, 400.0)).color(color("#616161")).px(12.0).pb(4.0)
}

/// A setting row with a switch on the right; the whole row toggles.
///
/// The shared primitive supplies the switch role, accessible name, checked state, focus and the
/// dimmed disabled look that this row used to approximate with local `el()` drawing.
pub(crate) fn toggle<A: Actions + 'static>(key: &str, title: &str, on: bool, enabled: bool, press: impl Fn(&mut UiState<A>) + 'static) -> El {
    controls::switch_item(key, title, on, enabled, press)
}

/// A radio row in a choice list. The circle is drawn by the primitive, never a text glyph.
pub(crate) fn choice<A: Actions + 'static>(key: String, label: &str, selected: bool, press: impl Fn(&mut UiState<A>) + 'static) -> El {
    controls::radio_item(&key, label, selected, press)
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
    let item = |section: Section| controls::menu_item(
        &format!("settings-nav-{section:?}"),
        section.label(),
        None,
        section == s.settings.section,
        move |s: &mut UiState<A>| select(s, section),
    );
    let shown_in = |group: &[Section]| group.iter().copied().filter(|section| shown.contains(section)).collect::<Vec<_>>();
    for section in shown_in(&Section::TEAMS) {
        nav = nav.child(item(section));
    }
    let rusty = shown_in(&Section::RUSTY);
    if !rusty.is_empty() {
        nav = nav.child(el().key("settings-nav-rusty-separator").w_full().h(1.0).mt(8.0).fill(color("#E0E0E0")))
            .child(text("RUSTY TEAMS").font(ts(12.0, 600.0)).color(color("#616161")).px(12.0).pt(12.0).pb(6.0));
        for section in rusty {
            nav = nav.child(item(section));
        }
    }
    // About stays at the bottom of the navigation, as in GPUI.
    if shown.contains(&Section::About) {
        nav = nav.child(el().grow(1.0)).child(item(Section::About));
    }
    // A search that hides the open section shows its first match instead.
    let section = if shown.contains(&s.settings.section) { s.settings.section } else { shown.first().copied().unwrap_or(s.settings.section) };
    let pane = match section {
        Section::General => general(s),
        Section::Notifications => notifications(s),
        Section::Appearance => appearance(s),
        Section::Chats => chats(s),
        Section::Devices => devices(s),
        Section::Calls => calls(s),
        Section::Agents => crate::agent_settings::pane(s),
        Section::About => about::<A>(),
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
    let (auto_start, background, close_to_tray, show_in_tray) = (prefs.auto_start, prefs.open_in_background, prefs.notify.close_to_tray, prefs.notify.show_in_tray);
    let mut pane = col().w_full()
        .child(caption("System"))
        .child(toggle("settings-auto-start", AUTO_START, auto_start, true, move |s: &mut UiState<A>| change(s, Change::AutoStart(!auto_start))))
        .child(toggle("settings-open-in-background", BACKGROUND, background, auto_start, move |s: &mut UiState<A>| change(s, Change::OpenInBackground(!background))))
        .child(toggle("settings-show-in-tray", &tr("settings.general.show_in_tray"), show_in_tray, true, |s: &mut UiState<A>| notify(s, |n| n.show_in_tray = !n.show_in_tray)))
        .child(detail(&tr("settings.general.show_in_tray_detail")));
    if show_in_tray {
        pane = pane
            .child(toggle("settings-close-to-tray", CLOSE_TO_TRAY, close_to_tray, true, |s: &mut UiState<A>| notify(s, |n| n.close_to_tray = !n.close_to_tray)))
            .child(detail(&tr("settings.general.close_to_tray_detail")));
    }
    pane = pane.child(caption("Compose box")).child(text("Up arrow in compose box").font(ts(14.0, 400.0)).color(color("#242424")).px(12.0).pt(8.0));
    for (label, behavior) in [("Edit last message", teams_core::UpArrowBehavior::EditLastMessage), ("Navigate to messages", teams_core::UpArrowBehavior::NavigateToMessages), ("Stay in compose", teams_core::UpArrowBehavior::StayInCompose)] {
        pane = pane.child(choice(format!("settings-up-{behavior:?}"), label, prefs.chat.settings().up_arrow == behavior, move |s: &mut UiState<A>| change(s, Change::Chat(ChatSettingChange::UpArrow(behavior)))));
    }
    pane.child(crate::translation::settings(s))
}

fn calls<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut pane = col().w_full().child(caption("Ringtones"));
    for call_type in chatsvc::sounds::RingtoneCallType::ALL {
        let current = s.settings.prefs.notify.sounds.ringtones.get(call_type);
        pane = pane.child(text(call_type.label()).font(ts(14.0, 600.0)).padding(12.0));
        for ringtone in chatsvc::sounds::Ringtone::ALL {
            pane = pane.child(choice(format!("settings-ring-{}-{}", call_type.name(), ringtone.name()), ringtone.label(), ringtone == current, move |s: &mut UiState<A>| {
                notify(s, |n| n.sounds.ringtones.set(call_type, ringtone));
                crate::sound::play(ringtone.file_name());
            }));
        }
    }
    pane
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
        .child(caption("Appearance and sound"))
        .child(toggle("settings-preview", PREVIEW, preview, true, move |s: &mut UiState<A>| change(s, Change::Chat(ChatSettingChange::ShowPreview(!preview)))))
        .child(toggle("settings-flash", FLASH, n.flash, true, |s: &mut UiState<A>| notify(s, |n| n.flash = !n.flash)))
        .child(caption("Sound"))
        .child(toggle("settings-notification-sound", SOUND, sounds.notification_sound, true, |s: &mut UiState<A>| notify(s, |n| n.sounds.notification_sound = !n.sounds.notification_sound)));
    if sounds.notification_sound {
        pane = pane.child(sound_list::<A>("settings-sound", &AlertSound::NORMAL, sounds.notification_sound_type, |n| &mut n.sounds.notification_sound_type));
    }
    pane = pane.child(toggle("settings-urgent-sound", URGENT, sounds.urgent_sound, true, |s: &mut UiState<A>| notify(s, |n| n.sounds.urgent_sound = !n.sounds.urgent_sound)));
    if sounds.urgent_sound {
        pane = pane.child(sound_list::<A>("settings-urgent", &AlertSound::URGENT, sounds.urgent_sound_type, |n| &mut n.sounds.urgent_sound_type));
    }
    pane = pane.child(toggle("settings-call-sound", CALL_SOUND, sounds.call_and_meeting_sound, true, |s: &mut UiState<A>| notify(s, |n| n.sounds.call_and_meeting_sound = !n.sounds.call_and_meeting_sound)))
        .child(toggle("settings-busy-sound", BUSY_SOUND, sounds.sound_when_busy, true, |s: &mut UiState<A>| notify(s, |n| n.sounds.sound_when_busy = !n.sounds.sound_when_busy)))
        .child(caption("Calls and meetings"))
        .child(toggle("settings-hide-in-calls", HIDE_IN_CALLS, n.mute_in_calls, true, |s: &mut UiState<A>| notify(s, |n| n.mute_in_calls = !n.mute_in_calls)));
    let account = prefs.chat.settings().notifications;
    pane = pane.child(caption("Activity"));
    for preset in teams_core::NotificationPreset::ALL {
        let label = match preset { teams_core::NotificationPreset::AllActivity => "All activity", teams_core::NotificationPreset::MentionsAndReplies => "Mentions and replies", teams_core::NotificationPreset::Customised => "Customised" };
        pane = pane.child(choice(format!("settings-preset-{preset:?}"), label, account.preset == preset, move |s: &mut UiState<A>| change(s, Change::Chat(ChatSettingChange::Preset(preset)))));
    }
    for (key, label, current, update) in [
        ("chats", "Chat messages", account.chat_messages, ChatSettingChange::ChatMessages as fn(_) -> _),
        ("mentions", "Personal mentions", account.personal_mentions, ChatSettingChange::PersonalMentions as fn(_) -> _),
        ("everyone", "Everyone mentions", account.everyone_mentions, ChatSettingChange::EveryoneMentions as fn(_) -> _),
        ("likes", "Likes and reactions", account.likes, ChatSettingChange::Likes as fn(_) -> _),
    ] {
        pane = pane.child(text(label).font(ts(14.0, 600.0)).padding(12.0));
        for (label, alert) in [("Banner and feed", teams_core::ActivityAlert::BannerAndFeed), ("Only feed", teams_core::ActivityAlert::OnlyFeed), ("Only banner", teams_core::ActivityAlert::OnlyBanner), ("Off", teams_core::ActivityAlert::Off)] {
            pane = pane.child(choice(format!("settings-alert-{key}-{alert:?}"), label, current == alert, move |s: &mut UiState<A>| change(s, Change::Chat(update(alert)))));
        }
    }
    pane = pane.child(caption("Meeting chats"));
    for (label, mode) in [("Mute", teams_core::MeetingChatNotifications::Mute), ("Unmute", teams_core::MeetingChatNotifications::Unmute), ("Mute until I join", teams_core::MeetingChatNotifications::MuteUntilJoin)] {
        pane = pane.child(choice(format!("settings-meeting-chat-{mode:?}"), label, account.meeting_chats == mode, move |s: &mut UiState<A>| change(s, Change::Chat(ChatSettingChange::MeetingChats(mode)))));
    }
    pane = pane.child(caption("Status notifications"));
    for person in &s.profile.followed {
        let id = person.user_id.clone();
        let name = person.name.clone();
        pane = pane.child(choice(format!("settings-unfollow-{id}"), &format!("{} · Turn off", person.name), false, move |s: &mut UiState<A>| s.actions.follow_presence(&id, &name, false)));
    }
    pane
}

fn appearance<A: Actions + 'static>(s: &UiState<A>) -> El {
    let current = s.appearance;
    let mut pane = col().w_full().child(caption("Theme"));
    for theme in ThemeChoice::ALL {
        pane = pane.child(choice(format!("theme-{}", theme.key()), theme.label(), current.theme == theme, move |s: &mut UiState<A>| {
            s.appearance.theme = theme;
            s.actions.set_appearance(s.appearance);
        }));
    }
    pane = pane.child(caption("Color scheme"));
    for scheme in schemes::SCHEMES {
        pane = pane.child(choice(format!("settings-color-scheme-{}", scheme.id), scheme.name, current.color_scheme.id == scheme.id, move |s: &mut UiState<A>| {
            s.appearance.color_scheme = scheme;
            s.actions.set_appearance(s.appearance);
        }));
    }
    pane = pane.child(caption("App icon"));
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
        .child(caption("Chat list"))
        .child(toggle("settings-muted-section", "Show muted items in their own section", s.section_settings.enabled(teams_core::ChatSection::Muted), true, |s: &mut UiState<A>| change(s, Change::MutedSection(!s.section_settings.enabled(teams_core::ChatSection::Muted)))))
        .child(caption("Chats"))
        .child(toggle("settings-markdown", "Format messages using mark-down", prefs.markdown, true, |s: &mut UiState<A>| change(s, Change::Markdown(!s.settings.prefs.markdown))))
        .child(toggle("settings-actions-right-click", "Show message actions on right click", prefs.actions_on_right_click, true, |s: &mut UiState<A>| change(s, Change::ActionsOnRightClick(!s.settings.prefs.actions_on_right_click))))
        .child(toggle("settings-read-receipts", RECEIPTS, receipts, true, move |s: &mut UiState<A>| change(s, Change::Chat(ChatSettingChange::ReadReceipts(!receipts)))))
        .child(text(ENTER).font(ts(14.0, 400.0)).color(color("#242424")).px(12.0).pt(8.0));
    for (label, behavior) in [("Send the message", EnterKeyBehavior::Send), ("Start a new line (to send, select Send or use Ctrl+Enter).", EnterKeyBehavior::NewLine)] {
        pane = pane.child(choice(format!("settings-enter-{behavior:?}"), label, account.enter_key == behavior, move |s: &mut UiState<A>| {
            change(s, Change::Chat(ChatSettingChange::EnterKey(behavior)));
        }));
    }
    pane = pane.child(caption("Message density"));
    for (label, density) in [("Comfy", teams_core::MessageDensity::Comfy), ("Compact", teams_core::MessageDensity::Compact)] {
        pane = pane.child(choice(format!("settings-density-{density:?}"), label, account.message_density == density, move |s: &mut UiState<A>| change(s, Change::Chat(ChatSettingChange::MessageDensity(density)))));
    }
    pane = pane.child(caption(REACTIONS));
    let mut reactions = row().w_full().wrap().gap(6.0).px(12.0);
    for id in REACTION_CHOICES {
        let on = prefs.reactions.iter().any(|chosen| chosen == id);
        let glyph = chatsvc::reaction_glyph_for_key(id).unwrap_or_else(|| id.to_owned());
        reactions = reactions.child(controls::chip(
            &format!("settings-reaction-{id}"),
            &glyph,
            on,
            move |s: &mut UiState<A>| {
                let mut ids = s.settings.prefs.reactions.clone();
                if on { ids.retain(|chosen| chosen != id); } else if ids.len() < teams_core::DEFAULT_REACTION_SLOTS { ids.push(id.to_owned()); } else { return; }
                change(s, Change::DefaultReactions(ids));
            },
        ));
    }
    pane = pane.child(reactions).child(caption(SKIN_TONE));
    let mut tones = row().w_full().gap(6.0).px(12.0);
    for tone in 0..=5u8 {
        let glyph = chatsvc::reaction_glyph_for_key(&if tone == 0 { "like".to_owned() } else { format!("like-tone{tone}") }).unwrap_or_else(|| tone.to_string());
        tones = tones.child(controls::chip(
            &format!("settings-skin-tone-{tone}"),
            &glyph,
            prefs.skin_tone == tone,
            move |s: &mut UiState<A>| change(s, Change::SkinTone(tone)),
        ));
    }
    pane.child(tones)
}

fn devices<A: Actions + 'static>(s: &UiState<A>) -> El {
    let prefs = &s.settings.prefs;
    let mut pane = col().w_full();
    for (device, title, entries, selected) in [
        (Device::Speaker, "Speaker", s.calls.devices.output_entries_for(&prefs.speaker), &prefs.speaker),
        (Device::Microphone, "Microphone", s.calls.devices.input_entries_for(&prefs.microphone), &prefs.microphone),
        (Device::Camera, "Camera", calling::camera_entries_for(&s.calls.cameras, &prefs.camera), &prefs.camera),
    ] {
        pane = pane.child(caption(title));
        for entry in entries {
            let pick = entry.choice.clone();
            pane = pane.child(choice(format!("settings-{title}-{}", entry.label), &entry.label, &entry.choice == selected, move |s: &mut UiState<A>| {
                change(s, Change::Device(device, pick.clone()));
            }));
        }
    }
    pane.child(camera_effects::<A>(prefs.effects))
}

/// GPUI's strength choices for the soft focus effect (`settings/devices.rs`).
const SOFT_FOCUS_LEVELS: [(&str, u8); 3] = [("Low", 25), ("Medium", 50), ("High", 75)];

fn camera_effects<A: Actions + 'static>(effects: CameraEffects) -> El {
    type Field = fn(&mut CameraEffects) -> &mut bool;
    let rows: [(&str, &str, bool, Field); 4] = [
        ("mirror", "Mirror my video", effects.mirror, |e| &mut e.mirror),
        ("framing", "Auto framing", effects.auto_framing, |e| &mut e.auto_framing),
        ("brightness", "Adjust brightness", effects.adjust_brightness, |e| &mut e.adjust_brightness),
        ("soft-focus", "Soft focus", effects.soft_focus, |e| &mut e.soft_focus),
    ];
    let mut pane = col().w_full();
    for (key, label, on, field) in rows {
        pane = pane.child(toggle(&format!("settings-effect-{key}"), label, on, true, move |s: &mut UiState<A>| {
            let mut changed = s.settings.prefs.effects;
            *field(&mut changed) = !on;
            change(s, Change::Effects(changed));
        }));
    }
    if effects.soft_focus {
        let mut levels = row().w_full().gap(6.0).px(12.0);
        for (label, level) in SOFT_FOCUS_LEVELS {
            levels = levels.child(controls::chip(
                &format!("settings-soft-focus-{level}"),
                label,
                effects.soft_focus_strength == level,
                move |s: &mut UiState<A>| change(s, Change::Effects(CameraEffects { soft_focus_strength: level, ..s.settings.prefs.effects })),
            ));
        }
        pane = pane.child(caption("Soft focus strength")).child(levels);
    }
    pane
}

fn about<A: Actions + 'static>() -> El {
    col().w_full()
        .child(caption("Rusty Teams"))
        .child(text(format!("Version {}", env!("CARGO_PKG_VERSION"))).font(ts(14.0, 400.0)).color(color("#242424")).px(12.0).pt(8.0))
        .child(text(format!("Build {}", app_backend::version::build_version())).font(ts(14.0, 400.0)).color(color("#616161")).px(12.0).pt(4.0))
        .child(controls::button::<A>(
            "settings-copy-build",
            "Copy build",
            Variant::Secondary,
            Size::Md,
            |_: &mut UiState<A>, tx| tx.copy(app_backend::version::build_version()),
        ))
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
        assert!(Section::General.matches("up arrow"));
        assert!(Section::Chats.matches("density"));
        assert!(Section::Chats.matches("right click"));
    }

    #[test]
    fn navigation_follows_gpui_order_with_the_rusty_teams_group() {
        let rt = settings_runtime();
        let elements = rt.elements();
        let y = |key: &str| by_key(&elements, key).rect[1];
        let order = ["General", "Notifications", "Appearance", "Chats", "Calls", "Devices"].map(|name| y(&format!("settings-nav-{name}")));
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "Teams group is in order");
        assert!(order[5] < y("settings-nav-rusty-separator"));
        assert!(y("settings-nav-rusty-separator") < y("settings-nav-Agents"));
        assert!(y("settings-nav-Agents") < y("settings-nav-About"), "About is pinned below the group");
        assert!(elements.iter().any(|e| e.text.as_deref() == Some("RUSTY TEAMS")));
    }

    #[test]
    fn a_search_that_hides_the_rusty_group_hides_its_separator_and_title() {
        let mut rt = settings_runtime();
        rt.update(|s, _| s.settings.search = TextEdit::new("receipts"));
        rt.frame(0.0);
        let elements = rt.elements();
        assert!(rt.find("settings-nav-rusty-separator").is_none());
        assert!(elements.iter().all(|e| e.text.as_deref() != Some("RUSTY TEAMS")));
    }

    #[test]
    fn show_in_tray_is_saved_and_gates_the_close_to_tray_row() {
        let mut rt = settings_runtime();
        assert!(rt.find("settings-close-to-tray").is_some());
        click(&mut rt, "settings-show-in-tray");
        assert!(!rt.state.settings.prefs.notify.show_in_tray);
        assert!(rt.find("settings-close-to-tray").is_none(), "close to tray needs the icon");
        assert!(matches!(rt.state.actions.saved.borrow().last(), Some(Change::Notify(n)) if !n.show_in_tray));
        assert!(Section::General.matches("tray"));
    }

    #[test]
    fn camera_effects_are_saved_and_found_by_search() {
        let mut rt = settings_runtime();
        rt.update(|s, _| s.settings.section = Section::Devices);
        rt.frame(0.0);
        click(&mut rt, "settings-effect-brightness");
        let expected = CameraEffects { adjust_brightness: true, ..CameraEffects::default() };
        assert_eq!(rt.state.calls.effects, expected, "the next call starts with it");
        assert_eq!(rt.state.actions.saved.borrow().last(), Some(&Change::Effects(expected)));
        click(&mut rt, "settings-effect-soft-focus");
        click(&mut rt, "settings-soft-focus-25");
        assert_eq!(rt.state.settings.prefs.effects.soft_focus_strength, 25);
        assert!(Section::Devices.matches("mirror my video"));
    }

    #[test]
    fn caption_sentence_cases_all_caps_labels_keeping_accents() {
        assert_eq!(sentence_case("PRÉFÉRENCES"), "Préférences");
        assert_eq!(sentence_case("UP ARROW"), "Up arrow");
        assert_eq!(sentence_case("Already fine"), "Already fine");
    }

    // --- Shared-control migration tests -------------------------------------------------

    fn by_key<'a>(elements: &'a [rune::ElementInfo], key: &str) -> &'a rune::ElementInfo {
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

    fn control_fixture(_s: &UiState<Fake>, _cx: &mut Cx<UiState<Fake>>) -> El {
        col()
            .key("settings-controls-fixture")
            .w(400.0)
            .gap(4.0)
            .padding(8.0)
            .child(toggle::<Fake>("fixture-switch-on", "Notifications", true, true, |_| {}))
            .child(toggle::<Fake>("fixture-switch-off", "Do not disturb", false, false, |_| {}))
            .child(choice::<Fake>("fixture-radio-on".to_owned(), "Light", true, |_| {}))
            .child(choice::<Fake>("fixture-radio-off".to_owned(), "Dark", false, |_| {}))
    }

    fn control_runtime() -> Runtime<UiState<Fake>> {
        let mut rt = Runtime::new(UiState::new(Fake::default()), control_fixture, (1000.0, 700.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        rt
    }

    fn settings_runtime() -> Runtime<UiState<Fake>> {
        let mut state = UiState::new(Fake::default());
        state.screens.screen = crate::screens::Screen::Settings;
        let mut rt = Runtime::new(state, crate::ui::view::<Fake>, (1100.0, 800.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        rt
    }

    #[test]
    fn shared_controls_toggle_exposes_switch_role_and_checked_state() {
        let rt = control_runtime();
        let elements = rt.elements();
        let on = by_key(&elements, "fixture-switch-on");
        assert_eq!(on.role, "switch");
        assert_eq!(on.label.as_deref(), Some("Notifications"));
        assert!(on.focusable, "an enabled switch is keyboard reachable");
        let tree = rt.accessibility_json();
        let on_node = a11y_by_key(&tree, "fixture-switch-on").expect("switch-on in a11y tree");
        let off_node = a11y_by_key(&tree, "fixture-switch-off").expect("switch-off in a11y tree");
        assert!(states(on_node).contains(&"checked"), "on switch is checked");
        assert!(states(off_node).contains(&"unchecked"), "off switch is unchecked");
    }

    #[test]
    fn shared_controls_choice_exposes_radio_role_without_glyphs() {
        let rt = control_runtime();
        let elements = rt.elements();
        let on = by_key(&elements, "fixture-radio-on");
        assert_eq!(on.role, "radio");
        assert_eq!(on.label.as_deref(), Some("Light"));
        assert!(on.focusable);
        assert!(
            elements.iter().all(|e| e.text.as_deref().is_none_or(|t| !t.contains('●') && !t.contains('○'))),
            "radio dots must be drawn, not text glyphs"
        );
        let tree = rt.accessibility_json();
        assert!(states(a11y_by_key(&tree, "fixture-radio-on").unwrap()).contains(&"checked"));
        assert!(states(a11y_by_key(&tree, "fixture-radio-off").unwrap()).contains(&"unchecked"));
    }

    #[test]
    fn shared_controls_disabled_toggle_is_dimmed_and_not_focusable() {
        let rt = control_runtime();
        let elements = rt.elements();
        let off = by_key(&elements, "fixture-switch-off");
        assert_eq!(off.role, "switch");
        assert_eq!(off.label.as_deref(), Some("Do not disturb"));
        assert!(off.opacity < 1.0, "disabled row is dimmed");
        assert!(!off.focusable, "disabled row leaves the tab order");
    }

    #[test]
    fn shared_controls_nav_rows_are_menu_items() {
        let rt = settings_runtime();
        let elements = rt.elements();
        let nav = by_key(&elements, "settings-nav-General");
        assert_eq!(nav.role, "menuitem");
        assert_eq!(nav.label.as_deref(), Some("General"));
        assert!(nav.focusable);
        assert!(nav.item_selected, "the open section is selected");
    }

    #[test]
    fn shared_controls_chips_expose_checked_state() {
        let mut rt = settings_runtime();
        rt.update(|s, _| {
            s.settings.section = Section::Chats;
            s.settings.prefs.reactions = vec!["like".to_owned()];
        });
        rt.frame(0.0);
        let elements = rt.elements();
        for key in ["settings-reaction-like", "settings-skin-tone-0"] {
            let chip = by_key(&elements, key);
            assert_eq!(chip.role, "button", "{key}");
            assert!(chip.focusable, "{key}");
            assert!((chip.rect[3] - 28.0).abs() < 0.5, "{key} chip height");
        }
        let tree = rt.accessibility_json();
        assert!(states(a11y_by_key(&tree, "settings-reaction-like").unwrap()).contains(&"checked"));
        assert!(states(a11y_by_key(&tree, "settings-skin-tone-3").unwrap()).contains(&"unchecked"));
    }

    #[test]
    fn captions_render_uppercase_callers_in_sentence_case() {
        fn view(_: &UiState<Fake>, _: &mut Cx<UiState<Fake>>) -> El {
            caption("NEVER TRANSLATE THESE LANGUAGES").key("caption-test")
        }
        let mut rt = Runtime::new(UiState::new(Fake::default()), view, (800.0, 600.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        let found = rt.elements().into_iter()
            .find(|element| element.key.as_deref() == Some("caption-test"))
            .expect("caption");
        assert_eq!(found.text.as_deref(), Some("Never translate these languages"));
    }

    #[test]
    fn shared_controls_captions_are_sentence_case() {
        let mut rt = settings_runtime();
        let mut texts = Vec::new();
        for section in Section::ALL {
            rt.update(|s, _| s.settings.section = section);
            rt.frame(0.0);
            texts.extend(rt.elements().into_iter().filter_map(|e| e.text));
        }
        for expected in [
            "System", "Theme", "Ringtones", "Sound", "Calls and meetings", "Activity", "Meeting chats",
            "Status notifications", "Rusty Teams", "Chat list", "Chats", "Message density", "Default reactions",
            "Skin tone", "Color scheme", "App icon",
        ] {
            assert!(texts.iter().any(|t| t == expected), "missing sentence-case caption {expected:?}");
        }
    }

    #[test]
    fn shared_controls_copy_build_is_a_secondary_button() {
        let mut rt = settings_runtime();
        rt.update(|s, _| s.settings.section = Section::About);
        rt.frame(0.0);
        let elements = rt.elements();
        let button = by_key(&elements, "settings-copy-build");
        assert_eq!(button.role, "button");
        assert_eq!(button.label.as_deref(), Some("Copy build"));
        assert!(button.focusable);
    }
}
