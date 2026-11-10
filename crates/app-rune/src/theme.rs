//! Dark and high-contrast theme. The appearance setting plus the OS dark mode resolve to one
//! shade of the shared palette (`app_backend::schemes`), which the light-hex helper in
//! `settings::color` reads. Rune's own widgets (text fields, placeholders, carets) take their
//! ink from `Cx::foreground`, so it is set from the same palette.

use app_backend::appearance::{AppearanceSettings, SystemTheme};
use app_backend::schemes;
use rune::*;

/// Resolves this view's colours from `settings` and the OS. Call first in every view run.
pub fn apply<S>(cx: &mut Cx<S>, settings: &AppearanceSettings) {
    let system = SystemTheme {
        dark: cx.system_dark(),
        high_contrast: app_backend::appearance::system_high_contrast(),
    };
    schemes::set_thread_scheme(Some(settings.color_scheme.id));
    schemes::set_thread_shade(Some(settings.theme.shade(system)));
    cx.foreground(crate::settings::color("#242424"));
}

/// Identifies the resolved theme, for keys that cache built rows (`virtual_rows_version`).
pub fn version() -> String {
    format!("{}:{:?}", schemes::active_scheme().id, schemes::active_shade())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Actions, SendTarget, Update};
    use crate::ui::UiState;
    use app_backend::appearance::ThemeChoice;
    use app_backend::schemes::Shade;
    use rune::{Env, Runtime};

    fn shade() -> Shade {
        schemes::active_shade()
    }

    struct Quiet;

    impl Actions for Quiet {
        fn open(&self, _conversation_id: &str) {}
        fn send(&self, _target: SendTarget, _text: String, _files: Vec<teams_core::FileReference>) {}
    }

    fn rt(system_dark: bool) -> Runtime<UiState<Quiet>> {
        let env = Env { system_dark, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Quiet), crate::ui::view::<Quiet>, (1000.0, 700.0), env);
        rt.frame(0.0);
        rt
    }

    fn choose(rt: &mut Runtime<UiState<Quiet>>, theme: ThemeChoice) {
        rt.update(move |s, _| {
            s.apply(Update::Appearance(AppearanceSettings { theme, ..Default::default() }));
        });
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn default_is_dark_whatever_the_os_says() {
        let _ = rt(false);
        assert_eq!(shade(), Shade::Dark);
    }

    #[test]
    fn follow_os_tracks_the_system_dark_mode() {
        let mut dark_os = rt(true);
        choose(&mut dark_os, ThemeChoice::FollowOperatingSystem);
        assert_eq!(shade(), Shade::Dark);

        let mut light_os = rt(false);
        choose(&mut light_os, ThemeChoice::FollowOperatingSystem);
        assert_eq!(shade(), Shade::Light);
    }

    #[test]
    fn explicit_choice_ignores_the_system() {
        let mut light_app = rt(true);
        choose(&mut light_app, ThemeChoice::Light);
        assert_eq!(shade(), Shade::Light);

        let mut dark_app = rt(false);
        choose(&mut dark_app, ThemeChoice::Dark);
        assert_eq!(shade(), Shade::Dark);
    }

    #[test]
    fn version_changes_with_the_resolved_theme() {
        let mut dark_app = rt(false);
        let dark = version();
        choose(&mut dark_app, ThemeChoice::Light);
        assert_ne!(version(), dark);
    }

    #[test]
    fn every_light_literal_maps_to_the_palette_in_dark() {
        schemes::set_thread_scheme(Some("rusty-teams"));
        schemes::set_thread_shade(Some(Shade::Dark));
        for hex in [
            "#FFFFFF", "#F5F5F5", "#EBEBEB", "#E0E0E0", "#242424", "#616161", "#5B5FC7", "#E8EBFA",
            "#DADDF5", "#C4314B",
        ] {
            let raw = rune::color::hex(hex).expect("valid literal");
            assert_ne!(crate::settings::color(hex), raw, "{hex} leaks the light theme");
        }
    }
}
