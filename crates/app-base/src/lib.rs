pub mod agent_api;
pub mod agents;
#[cfg(feature = "gpui")]
pub mod assets;
pub mod avatar_image;
pub mod blocks;
pub mod channel_files;
pub mod demo_cards;
pub mod demo_chart_cards;
pub mod demo_gifs;
pub mod demo_input_cards;
pub mod demo_library;
pub mod demo_recap;
pub mod downloads;
pub mod emoji;
pub mod emoticons;
#[cfg(feature = "gpui")]
pub mod frame_log;
pub mod fuzzy;
#[cfg(feature = "gpui")]
pub mod image_clipboard;
pub mod image_view_math;
pub mod inline_image;
#[cfg(target_os = "linux")]
pub mod linux_desktop;
#[cfg(target_os = "linux")]
pub mod linux_fonts;
#[cfg(target_os = "linux")]
mod linux_idle;
pub mod native_dumps;
pub mod notify;
pub mod read_state;
pub mod recap;
pub mod regional;
pub mod release_feed;
pub mod remote_image;
pub mod schedule_time;
pub mod syntax;
pub mod single_instance;
pub mod status_choice;
pub mod stickers;
pub mod task_dialog;
#[cfg(feature = "gpui")]
pub mod theme;
pub mod translation;
pub mod ui_locale {
    pub use app_backend::ui_locale::*;

    pub fn apply_locale(locale: &str) {
        app_backend::ui_locale::apply_locale(locale);
        #[cfg(feature = "gpui")]
        gpui_kit::component::set_locale(locale);
    }

    /// Serialises this crate's tests that read or set the global UI language.
    #[cfg(test)]
    pub(crate) fn locale_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|error| error.into_inner())
    }
}
pub mod updater;
pub mod user_activity;
pub use app_backend::{appearance, autostart, crash_log, diagnostics, format, orgs, presence_link, runtime, support, version};
#[cfg(feature = "gpui")]
pub mod zoom;
pub const APP_NAME: &str = "Rusty Teams";

