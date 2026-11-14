//! Loads and saves the settings panes' values in the places the GPUI client uses: the local
//! database for our own settings, the Teams account for Teams settings.

use std::sync::Arc;

use app_backend::autostart::{self, StartupSettings};
use app_backend::backend::Engine;
use app_backend::notify::{rules, settings as notify_settings};
use calling::DeviceChoice;
use store::Store;

use crate::bridge::Shared;
use crate::settings::{Change, Device, Preferences};
use crate::types::Update;

/// The Run key value; GPUI registers `APP_NAME` for its own executable.
pub const AUTOSTART_NAME: &str = "Rusty Teams (Rune)";
const DEFAULT_DEVICE: &str = "default";

fn device_key(device: Device) -> &'static str {
    match device {
        Device::Camera => "call_camera",
        Device::Microphone => "call_input",
        Device::Speaker => "call_output",
    }
}

fn load_device(store: &Store, device: Device) -> DeviceChoice {
    match store.meta(device_key(device)).ok().flatten() {
        Some(key) if !key.is_empty() && key != DEFAULT_DEVICE => DeviceChoice::Device(key),
        _ => DeviceChoice::SystemDefault,
    }
}

pub fn load(store: &Store) -> Preferences {
    let startup = StartupSettings::load(store, AUTOSTART_NAME, true);
    Preferences {
        notify: notify_settings::load(store),
        auto_start: startup.auto_start,
        open_in_background: startup.open_in_background,
        chat: teams_core::stored_chat_settings(store),
        reactions: teams_core::default_reactions(store),
        skin_tone: teams_core::skin_tone(store),
        markdown: state::chat_settings_actions::stored_markdown_mode(store),
        actions_on_right_click: state::chat_settings_actions::stored_actions_on_right_click(store),
        camera: load_device(store, Device::Camera),
        microphone: load_device(store, Device::Microphone),
        speaker: load_device(store, Device::Speaker),
        translation: store.meta("translation_settings").ok().flatten()
            .and_then(|json| serde_json::from_str(&json).ok()).and_then(|value| teams_core::parse_language_settings(&value).ok()),
        languages: store.meta("translation_languages").ok().flatten()
            .and_then(|json| serde_json::from_str::<Vec<(String, String)>>(&json).ok())
            .unwrap_or_default().into_iter().map(|(code, name)| teams_core::Language { code, name }).collect(),
    }
}

fn republish(shared: &Shared, error: impl std::fmt::Display) {
    shared.publish(Update::Status(format!("Setting not saved: {error}")));
    shared.publish(Update::Preferences(Box::new(load(&shared.store))));
}

/// Saves `change`; account writes run on the backend runtime and undo the local copy on failure.
pub fn save(shared: &Arc<Shared>, change: Change) {
    let store = &shared.store;
    let engine = shared.engine();
    let shared = shared.clone();
    match change {
        Change::Notify(settings) => {
            let previous = notify_settings::load(store);
            notify_settings::save(store, &settings);
            if previous.sounds == settings.sounds { return; }
            let (Some(engine), Some(mri)) = (engine, store.meta("me_user_id").ok().flatten().map(|id| chatsvc::members::user_mri(&id))) else { return };
            app_backend::runtime::handle().spawn(async move {
                if let Err(error) = engine.save_sound_settings(&mri, &previous.sounds, &settings.sounds).await {
                    // Other notification settings may have changed since; undo only the sounds.
                    let current = notify_settings::load(&shared.store);
                    notify_settings::save(&shared.store, &rules::Settings { sounds: previous.sounds, ..current });
                    republish(&shared, error);
                }
            });
        }
        Change::AutoStart(enabled) => {
            if let Err(error) = autostart::set_enabled(AUTOSTART_NAME, enabled) { republish(&shared, error); }
        }
        Change::OpenInBackground(enabled) => StartupSettings { auto_start: true, open_in_background: enabled }.save_background(store),
        Change::Chat(change) => account(shared, engine.map(|engine| async move { engine.save_chat_setting(change).await.map(drop) })),
        Change::Translation(patch) => {
            let Some(engine) = engine else { return republish(&shared, "not connected") };
            app_backend::runtime::handle().spawn(async move {
                if let Err(error) = engine.patch_language_settings(patch).await { return republish(&shared, error); }
                refresh_translation(&shared, &engine).await;
                shared.publish(Update::Preferences(Box::new(load(&shared.store))));
            });
        }
        Change::MutedSection(enabled) => {
            let Some(engine) = engine else { return republish(&shared, "not connected") };
            app_backend::runtime::handle().spawn(async move {
                if let Err(error) = engine.set_section_enabled(teams_core::ChatSection::Muted, enabled).await {
                    shared.publish(Update::Status(format!("Setting not saved: {error}")));
                }
                shared.publish(Update::SectionSettings(teams_core::stored_section_settings(&shared.store)));
            });
        }
        Change::SkinTone(tone) => account(shared, engine.map(|engine| async move { engine.set_skin_tone(tone).await })),
        Change::DefaultReactions(ids) => account(shared, engine.map(|engine| async move { engine.set_default_reactions(&ids).await })),
        Change::Markdown(on) | Change::ActionsOnRightClick(on) => {
            let key = if matches!(change, Change::Markdown(_)) { state::chat_settings_actions::META_MARKDOWN_MODE } else { state::chat_settings_actions::META_ACTIONS_ON_RIGHT_CLICK };
            if let Err(error) = store.set_meta(key, if on { "true" } else { "false" }) { republish(&shared, error); }
        }
        Change::Device(device, choice) => {
            let value = match &choice { DeviceChoice::SystemDefault => DEFAULT_DEVICE, DeviceChoice::Device(key) => key };
            if let Err(error) = store.set_meta(device_key(device), value) { return republish(&shared, error); }
            if device == Device::Camera && let Some(launcher) = shared.launcher() { launcher.set_default_camera(choice); }
        }
    }
}

/// Runs an account write; the backend stores the value only once the account took it.
fn account<E: std::fmt::Display>(shared: Arc<Shared>, write: Option<impl Future<Output = Result<(), E>> + Send + 'static>) {
    let Some(write) = write else { return republish(&shared, "not connected") };
    app_backend::runtime::handle().spawn(async move {
        if let Err(error) = write.await { republish(&shared, error); }
    });
}

/// Reads the account's chat and sound settings once connected, as GPUI does at sign-in.
pub fn refresh(shared: Arc<Shared>, engine: Arc<Engine>) {
    app_backend::runtime::handle().spawn(async move {
        if let Err(error) = engine.refresh_chat_settings().await {
            app_backend::diagnostics::record(format!("Rune: chat settings not read: {error}"));
        }
        match engine.sound_settings().await {
            Ok(sounds) => {
                let mut settings = notify_settings::load(&shared.store);
                settings.sounds = sounds;
                notify_settings::save(&shared.store, &settings);
            }
            Err(error) => app_backend::diagnostics::record(format!("Rune: sound settings not read: {error}")),
        }
        refresh_translation(&shared, &engine).await;
        shared.publish(Update::Preferences(Box::new(load(&shared.store))));
    });
}

async fn refresh_translation(shared: &Shared, engine: &Engine) {
    match engine.language_settings().await {
        Ok(settings) => {
            if let Err(error) = shared.store.set_meta("translation_settings", &settings.to_account_value().to_string()) { republish(shared, error); }
        }
        Err(error) => shared.publish(Update::Status(format!("Translation settings unavailable: {error}"))),
    }
    if let Ok(languages) = engine.translation_languages("en").await {
        let pairs: Vec<_> = languages.into_iter().map(|language| (language.code, language.name)).collect();
        if let Err(error) = shared.store.set_meta("translation_languages", &serde_json::json!(pairs).to_string()) { republish(shared, error); }
    }
}

/// Lists devices off the UI thread for the Devices pane.
pub fn list_devices(shared: Arc<Shared>) {
    crate::calls::list_devices(move |update| shared.publish(Update::Call(update)));
}
