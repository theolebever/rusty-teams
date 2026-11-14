//! Channel SharePoint libraries, with shared sorting and breadcrumb behavior.

use rune::*;
use teams_core::{DriveEntry, DriveFolder};
use app_base::channel_files::{Crumb, LibraryPath};
use crate::types::Actions;
use crate::ui::{UiState, text, ts};

#[derive(Default)]
pub struct State {
    pub channel: Option<String>,
    pub path: Option<LibraryPath>,
    pub asked: Option<DriveFolder>,
    pub rows: Vec<DriveEntry>,
    pub loading: bool,
    pub error: Option<String>,
}

fn start<A: Actions>(s: &mut UiState<A>) {
    let Some(channel) = s.selected.clone() else { return };
    s.library = State { channel: Some(channel.clone()), loading: true, ..Default::default() };
    s.actions.library_root(&channel);
}

fn load<A: Actions>(s: &mut UiState<A>) {
    let (Some(channel), Some(path)) = (&s.library.channel, &s.library.path) else { return };
    let folder = path.current().clone();
    s.library.asked = Some(folder.clone());
    s.library.loading = true;
    s.library.error = None;
    s.actions.library_rows(channel, folder);
}

pub fn root<A: Actions>(s: &mut UiState<A>, channel: String, result: Result<DriveEntry, String>) {
    if s.library.channel.as_ref() != Some(&channel) || s.selected.as_ref() != Some(&channel) { return; }
    match result {
        Ok(entry) => {
            s.library.path = Some(LibraryPath::new(Crumb { name: entry.name.clone(), folder: entry.folder(), web_url: entry.web_url }));
            load(s);
        }
        Err(error) => { s.library.loading = false; s.library.error = Some(error); }
    }
}

pub fn loaded<A: Actions>(s: &mut UiState<A>, channel: String, folder: DriveFolder, result: Result<Vec<DriveEntry>, String>) {
    if s.library.channel.as_ref() != Some(&channel) || s.library.asked.as_ref() != Some(&folder) { return; }
    s.library.loading = false;
    match result {
        Ok(rows) => { s.library.rows = app_base::channel_files::sorted_entries(rows); s.library.error = None; }
        Err(error) => s.library.error = Some(error),
    }
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut body = col().key("channel-library").grow(1.0).min_h(0.0).w_full().padding(16.0).gap(8.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .on_visible(|s: &mut UiState<A>, on, _| { if on && s.library.channel.is_none() { start(s); } });
    let mut crumbs = row().w_full().wrap().gap(6.0);
    if let Some(path) = &s.library.path {
        for (index, crumb) in path.crumbs().iter().enumerate() {
            crumbs = crumbs.child(crate::screens::button(&crumb.name, &format!("library-crumb-{index}"), move |s: &mut UiState<A>, _| {
                if let Some(path) = &mut s.library.path { path.go_to(index); } load(s);
            }));
        }
    }
    body = body.child(text("Channel files").font(ts(18.0, 600.0))).child(crumbs)
        .child(crate::screens::button("Refresh", "library-refresh", |s: &mut UiState<A>, _| { if s.library.path.is_some() { load(s); } else { start(s); } }));
    if s.library.loading { return body.child(text("Loading…")); }
    if let Some(error) = &s.library.error { return body.child(text(error.clone()).key("library-error").wrap()); }
    if s.library.rows.is_empty() { body = body.child(text("This folder is empty")); }
    for (index, entry) in s.library.rows.iter().enumerate() {
        let picked = entry.clone();
        body = body.child(row().key(format!("library-row-{index}")).w_full().h(40.0).gap(8.0).center_y()
            .on_press_tx(move |s: &mut UiState<A>, tx| {
                if picked.is_folder() {
                    if let Some(path) = &mut s.library.path { path.enter(&picked); } load(s);
                } else { crate::rich_text::open(s, tx, &picked.web_url); }
            })
            .child(text(format!("{} {}", if entry.is_folder() { "▸" } else { "" }, entry.name)).font(ts(14.0, 400.0)).grow(1.0).truncate())
            .child(text(app_base::channel_files::size_label(entry)).font(ts(12.0, 400.0))));
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake;
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
    }
    #[test]
    fn listings_from_another_channel_or_folder_cannot_replace_the_current_listing() {
        let mut s = UiState::new(Fake);
        let folder = DriveFolder { drive_id: "drive".into(), item_id: "folder".into() };
        s.library.channel = Some("channel".into());
        s.library.asked = Some(folder.clone());
        s.library.loading = true;
        loaded(&mut s, "other".into(), folder.clone(), Err("wrong channel".into()));
        assert!(s.library.loading);
        loaded(&mut s, "channel".into(), folder, Err("offline".into()));
        assert!(!s.library.loading);
        assert_eq!(s.library.error.as_deref(), Some("offline"));
    }
}
