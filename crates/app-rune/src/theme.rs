//! Colour scheme and shade. The appearance setting (scheme, theme) plus the OS dark mode resolve to one
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
    cx.caption_color(crate::settings::color("#242424"));
    cx.focus_ring(2.0, 2.0, crate::settings::color("#5B5FC7"));
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
    fn rune_uses_the_chosen_color_scheme() {
        let mut rt = rt(false);
        assert_eq!(schemes::active_scheme().id, schemes::default_scheme().id);
        rt.update(|s, _| s.apply(Update::Appearance(AppearanceSettings { color_scheme: &schemes::TEAMS, ..Default::default() })));
        rt.frame(1.0 / 60.0);
        assert_eq!(schemes::active_scheme().id, "teams");
        assert_eq!(schemes::active_palette().accent, 0x5b5fc7);
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
        // UI control literals used in the views outside the deliberately dark call scene.
        // `#FFF4CE` is the find-match highlight in `find::mark`; it used to leak a fixed light yellow.
        for hex in [
            "#FFFFFF", "#F5F5F5", "#EBEBEB", "#E0E0E0", "#242424", "#616161", "#5B5FC7", "#E8EBFA",
            "#DADDF5", "#C4314B", "#FFF4CE",
        ] {
            let raw = rune::color::hex(hex).expect("valid literal");
            assert_ne!(crate::settings::color(hex), raw, "{hex} leaks the light theme");
        }
    }

    /// The find highlight is a translucent wash, not an ink colour. `palette.selection` is the
    /// role for a highlighted row: in dark it is a 40%-alpha warm tint over the dark row surface,
    /// in light a 25%-alpha tint over the light surface, so the existing foreground stays legible.
    /// Its palette value is `RRGGBBAA`, which `rune::color::hex` accepts as an 8-digit hex literal.
    #[test]
    fn find_highlight_uses_the_palette_selection_role() {
        schemes::set_thread_scheme(Some("rusty-teams"));
        schemes::set_thread_shade(Some(Shade::Dark));
        let raw = rune::color::hex("#FFF4CE").expect("valid literal");
        let mapped = crate::settings::color("#FFF4CE");
        assert_ne!(mapped, raw, "the find highlight leaks a fixed light yellow");

        let palette = schemes::active_palette();
        let selection = rune::color::hex(&format!("#{:06X}", palette.selection)).expect("8-digit palette literal");
        assert_eq!(mapped, selection, "the highlight must follow the palette's selection role");

        // Also check the other shades: the mapping must never fall back to the hard-coded yellow.
        schemes::set_thread_shade(Some(Shade::Light));
        assert_ne!(crate::settings::color("#FFF4CE"), raw);
        schemes::set_thread_shade(Some(Shade::HighContrast));
        assert_ne!(crate::settings::color("#FFF4CE"), raw);
    }

    /// The 8-digit palette values are `RRGGBBAA`, and `rune::color::hex` decodes them with a real
    /// alpha channel (rather than rejecting them), which is why `selection` can be used directly.
    #[test]
    fn rune_hex_decodes_the_palette_selection_literal() {
        schemes::set_thread_scheme(Some("rusty-teams"));
        schemes::set_thread_shade(Some(Shade::Dark));
        let literal = format!("#{:06X}", schemes::active_palette().selection);
        let decoded = rune::color::hex(&literal).expect("rune accepts 8-digit #rrggbbaa");
        assert!(decoded.a < 1.0, "selection is intentionally translucent");
    }

    /// A secondary button's resting fill is a background, not the colour used for its hover state.
    /// `#F5F5F5` is the light-theme background literal; in dark it must resolve to `background`
    /// and not to `popup_hover` (which would make the button look already hovered).
    #[test]
    fn secondary_button_rest_background_is_not_the_hover_colour() {
        schemes::set_thread_scheme(Some("rusty-teams"));
        schemes::set_thread_shade(Some(Shade::Dark));
        let palette = schemes::active_palette();
        let background = rune::color::hex(&format!("#{:06X}", palette.background)).expect("palette literal");
        let hover = rune::color::hex(&format!("#{:06X}", palette.popup_hover)).expect("palette literal");
        assert_eq!(crate::settings::color("#F5F5F5"), background);
        assert_ne!(background, hover, "dark background and hover must be distinguishable");
        assert_ne!(crate::settings::color("#F5F5F5"), hover);
    }
}
