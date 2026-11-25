//! Rune uses this fork's portable executable, with a staging folder separate from GPUI.

use std::path::Path;
use app_base::updater::UpdatePaths;
use crate::{types::Actions, ui::{UiState, text, ts}};
use rune::*;

pub const RELEASES: &str = "https://api.github.com/repos/theolebever/rusty-teams/releases?per_page=10";
pub const ASSET_SUFFIX: &str = "-windows-amd64-portable.exe";

#[derive(Default)]
pub struct State { pub checking: bool, pub ready: Option<String>, pub error: Option<String> }

pub fn paths(exe: &Path) -> UpdatePaths {
    let directory = exe.parent().unwrap_or_else(|| Path::new("."));
    let staged = directory.join("update-rune");
    UpdatePaths { exe: exe.into(), old_exe: exe.with_extension("old.exe"), update_exe: staged.join("rune.exe"), version_file: staged.join("version.txt"), pdb: exe.with_extension("pdb"), update_pdb: staged.join("rune.pdb") }
}

pub fn check<A: Actions>(s: &mut UiState<A>) {
    if s.updates.checking { return; }
    if app_backend::version::release_tag().is_none() {
        s.updates.error = Some("Development build: automatic updates are available in tagged releases.".into()); return;
    }
    s.updates.checking = true;
    s.updates.error = None;
    s.actions.check_app_update();
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut body = col().w_full().gap(8.0).padding(12.0)
        .child(if s.updates.checking {
            crate::controls::button::<A>("updates-check", "Checking…", crate::controls::Variant::Quiet, crate::controls::Size::Md, |_, _| {})
                .on_press(|_: &mut UiState<A>| {})
        } else {
            crate::controls::button::<A>("updates-check", "Check for updates", crate::controls::Variant::Secondary, crate::controls::Size::Md, |s: &mut UiState<A>, _| check(s))
        });
    if let Some(tag) = &s.updates.ready {
        body = body.child(text(format!("Update {tag} is ready")).font(ts(14.0, 600.0)))
            .child(crate::controls::button::<A>("updates-install", "Install and restart", crate::controls::Variant::Primary, crate::controls::Size::Md, |s: &mut UiState<A>, tx| {
                let busy = s.calls.state.as_ref().is_some_and(calling::CallState::is_active) || s.calls.pending || !s.outbox.is_empty()
                    || crate::ui::active_target(s).is_some_and(|target| s.media.is_uploading(&target));
                if busy { s.updates.error = Some("Finish calls, uploads and outgoing messages before restarting.".into()); return; }
                crate::view_composer::persist_draft(s);
                let result = std::env::current_exe().map_err(|e| e.to_string()).and_then(|exe| app_base::updater::install_and_relaunch(&paths(&exe)));
                match result { Ok(()) => tx.quit(), Err(error) => s.updates.error = Some(error) }
            }));
    }
    if let Some(error) = &s.updates.error { body = body.child(text(error.clone()).font(ts(12.0, 400.0)).wrap()); }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::ElementInfo;

    struct Quiet;
    impl Actions for Quiet {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
    }

    fn updates_view(s: &UiState<Quiet>, _: &mut Cx<UiState<Quiet>>) -> El {
        view(s)
    }

    fn by_key<'a>(elements: &'a [ElementInfo], key: &str) -> &'a ElementInfo {
        elements.iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no element {key}"))
    }

    #[test]
    fn checking_for_updates_is_a_dimmed_waiting_button() {
        let mut state = UiState::new(Quiet);
        state.updates.checking = true;
        let mut rt = Runtime::new(state, updates_view, (800.0, 600.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        let elements = rt.elements();
        let e = by_key(&elements, "updates-check");
        assert_eq!(e.label.as_deref(), Some("Checking…"));
        assert!(!e.focusable, "a busy check leaves the tab order");
        assert!(e.opacity < 1.0);
        assert_eq!(e.fill.a, 0.0);
    }

    #[test]
    fn install_is_a_primary_action() {
        use app_backend::schemes::active_palette;
        let mut state = UiState::new(Quiet);
        state.updates.ready = Some("v9.0.0".into());
        let mut rt = Runtime::new(state, updates_view, (800.0, 600.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        let elements = rt.elements();
        let e = by_key(&elements, "updates-install");
        assert_eq!(e.role, "button");
        assert!(e.focusable);
        let accent = rune::color::hex(&format!("#{:06X}", active_palette().accent)).expect("accent colour");
        assert_eq!(e.fill, accent);
    }

    #[test]
    fn rune_updates_cannot_take_gpui_staging_or_installer_assets() {
        let exe = Path::new("client/rusty-teams-rune.exe");
        let rune = paths(exe);
        assert_ne!(rune.update_exe, UpdatePaths::beside(exe).update_exe);
        assert_eq!(rune.exe, exe);
        let release: app_base::release_feed::Release = serde_json::from_value(serde_json::json!({"tag_name":"v9.0.0", "assets":[{"name":"rusty-teams-v9.0.0-windows-amd64-setup.exe","browser_download_url":"https://example.test/setup", "size":10},{"name":"rusty-teams-v9.0.0-windows-amd64-portable.exe","browser_download_url":"https://example.test/rune", "size":10}]})).unwrap();
        let releases = [release];
        let (_, asset) = app_base::release_feed::newest_update_for("v1.0.0", &releases, ASSET_SUFFIX).unwrap();
        assert_eq!(asset.browser_download_url, "https://example.test/rune");
    }
}
