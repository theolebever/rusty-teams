//! The OneDrive picker above the composer (GPUI `composer/cloud_picker.rs`): Recent, My files and
//! Shared with me, folders with a breadcrumb, a search box, and several files picked at once. The
//! picked files join the draft's attachments as cloud file references; nothing is uploaded.
//! ponytail: no Quick access sites column; add `picker_sites` as a fourth tab when someone asks.

use app_backend::schemes::active_palette;
use rune::*;
use teams_core::{CloudFile, DriveEntry, DriveFolder};

use crate::avatar::rgb;
use crate::icons::sym;
use crate::types::{Actions, SendTarget};
use crate::ui::{INK, LINE, MUTED, UiState, c, text, ts};

const LIST_H: f32 = 240.0;
const ROW_H: f32 = 40.0;
const TABS: [(Place, &str, &str); 3] =
    [(Place::Recent, "Recent", "cloud-tab-recent"), (Place::MyFiles, "My files", "cloud-tab-mine"), (Place::Shared, "Shared with me", "cloud-tab-shared")];

/// What a listing asks the bridge for; the answer comes back as `Update::CloudFiles` with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    Recent,
    MyFiles,
    Shared,
    Folder(DriveFolder),
    Search(String),
}

#[derive(Default)]
enum Rows {
    #[default]
    Loading,
    Ready(Vec<DriveEntry>),
    Failed(String),
}

#[derive(Default)]
pub struct State {
    /// The small menu under the attach icon: upload from this device or attach cloud files.
    pub menu: bool,
    /// The draft the open picker attaches to.
    pub open: Option<SendTarget>,
    pub query: TextEdit,
    tab: usize,
    crumbs: Vec<(String, DriveFolder)>,
    /// The listing on screen; answers for any other place are dropped.
    asked: Option<Place>,
    rows: Rows,
    selected: Vec<DriveEntry>,
}

impl State {
    pub fn loaded(&mut self, place: Place, result: Result<Vec<DriveEntry>, String>) {
        if self.asked.as_ref() == Some(&place) {
            self.rows = match result {
                Ok(rows) => Rows::Ready(rows),
                Err(error) => Rows::Failed(error),
            };
        }
    }
}

/// A recent or found file as a picker row. Files found by search have no ids and are attached by link.
pub fn entry(file: CloudFile, owner: Option<String>) -> DriveEntry {
    DriveEntry {
        drive_id: file.drive_id.unwrap_or_default(),
        id: file.item_id.unwrap_or_default(),
        name: file.name,
        web_url: file.web_url,
        size: file.size,
        modified_at: file.last_opened,
        modified_by: owner,
        child_count: None,
        download_url: None,
    }
}

/// The file `attach_cloud_file` needs; an empty id means "find it by its link".
pub fn cloud_file(entry: &DriveEntry) -> CloudFile {
    let mut file = CloudFile::from(entry);
    file.item_id = file.item_id.filter(|id| !id.is_empty());
    file
}

fn show<A: Actions>(s: &mut UiState<A>, place: Place) {
    s.compose.cloud.asked = Some(place.clone());
    s.compose.cloud.rows = Rows::Loading;
    s.actions.list_cloud_files(place);
}

pub fn open<A: Actions>(s: &mut UiState<A>, target: SendTarget) {
    s.emoji_open = false;
    s.compose.gif.open = false;
    s.compose.cloud = State { open: Some(target), ..Default::default() };
    show(s, Place::Recent);
}

fn close<A: Actions>(s: &mut UiState<A>) {
    s.compose.cloud = State::default();
}

fn open_tab<A: Actions>(s: &mut UiState<A>, tab: usize) {
    let cloud = &mut s.compose.cloud;
    cloud.tab = tab;
    cloud.crumbs.clear();
    cloud.query = TextEdit::default();
    show(s, TABS[tab].0.clone());
}

fn search<A: Actions>(s: &mut UiState<A>) {
    let query = s.compose.cloud.query.text.trim().to_owned();
    s.compose.cloud.crumbs.clear();
    let tab = TABS[s.compose.cloud.tab].0.clone();
    show(s, if query.is_empty() { tab } else { Place::Search(query) });
}

fn press_row<A: Actions>(s: &mut UiState<A>, row: DriveEntry) {
    if row.is_folder() {
        s.compose.cloud.crumbs.push((row.name.clone(), row.folder()));
        show(s, Place::Folder(row.folder()));
        return;
    }
    let selected = &mut s.compose.cloud.selected;
    match selected.iter().position(|picked| picked.web_url == row.web_url) {
        Some(index) => { selected.remove(index); }
        None => selected.push(row),
    }
}

fn attach<A: Actions>(s: &mut UiState<A>) {
    let cloud = std::mem::take(&mut s.compose.cloud);
    let Some(target) = cloud.open.filter(|_| !s.read_only) else { return };
    for picked in cloud.selected {
        s.media.attach_cloud(target.clone(), picked.name.clone());
        s.actions.attach_cloud_file(target.clone(), cloud_file(&picked));
    }
}

/// The two rows under the attach icon.
pub fn menu<A: Actions + 'static>(target: &SendTarget) -> El {
    let palette = active_palette();
    let item = |key: &str, label: &str| {
        row().key(key.to_owned()).w_full().h(32.0).px(8.0).center_y().radius(6.0).hover(rgb(palette.popup_hover))
            .child(text(label.to_owned()).font(ts(13.0, 400.0)).color(c(INK)))
    };
    let upload = crate::media::open_picker::<A>(target);
    let cloud_target = target.clone();
    col().key("attach-menu").w(240.0).padding(4.0).radius(8.0)
        .border(1.0, rgb(palette.border_strong)).fill(rgb(palette.surface))
        .child(item("attach-upload", "Upload from this device").on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| {
            s.compose.cloud.menu = false;
            upload(s, tx);
        }))
        .child(item("attach-cloud", "Attach cloud files").on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| {
            open(s, cloud_target.clone());
            tx.focus("cloud-search", false);
        }))
}

fn detail(row: &DriveEntry) -> String {
    let when = row.modified_at.map(|at| at.format("%Y-%m-%d").to_string());
    [row.modified_by.clone(), when].into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

pub fn picker<A: Actions + 'static>(s: &UiState<A>) -> El {
    let palette = active_palette();
    let hover = rgb(palette.popup_hover);
    let cloud = &s.compose.cloud;
    let note = |message: String| text(message).font(ts(13.0, 400.0)).color(c(MUTED));

    let mut tabs = row().key("cloud-tabs").gap(4.0);
    for (index, (_, label, key)) in TABS.iter().enumerate() {
        let current = index == cloud.tab && !matches!(cloud.asked, Some(Place::Search(_)));
        tabs = tabs.child(row().key(*key).h(28.0).px(10.0).center_y().radius(6.0).hover(hover)
            .on_press(move |s: &mut UiState<A>| open_tab(s, index))
            .child(text(*label).font(ts(13.0, if current { 700.0 } else { 400.0 })).color(if current { rgb(palette.accent_text) } else { c(INK) })));
    }

    let search_box = row().w_full().h(32.0).px(8.0).center_y().radius(6.0).border(1.0, c(LINE)).child(
        text_input(&cloud.query, ts(13.0, 400.0))
            .key("cloud-search")
            .label("Search cloud files")
            .placeholder("Search cloud files", c(MUTED))
            .color(c(INK))
            .single_line()
            .w_full()
            .on_edit(|s: &mut UiState<A>| &mut s.compose.cloud.query)
            .on_submit(|s: &mut UiState<A>, _tx: &mut Tx| search(s)),
    );

    let crumbs = (!cloud.crumbs.is_empty()).then(|| {
        let tab = cloud.tab;
        let mut trail = row().key("cloud-crumbs").gap(4.0).center_y()
            .child(text(TABS[tab].1).key("cloud-crumb-root").font(ts(12.0, 600.0)).color(rgb(palette.accent_text))
                .on_press(move |s: &mut UiState<A>| open_tab(s, tab)));
        for (index, (name, folder)) in cloud.crumbs.iter().enumerate() {
            let folder = folder.clone();
            trail = trail.child(text("›").font(ts(12.0, 400.0)).color(c(MUTED)))
                .child(text(name.clone()).key(format!("cloud-crumb-{index}")).font(ts(12.0, 600.0)).color(rgb(palette.accent_text))
                    .on_press(move |s: &mut UiState<A>| {
                        s.compose.cloud.crumbs.truncate(index + 1);
                        show(s, Place::Folder(folder.clone()));
                    }));
        }
        trail
    });

    let mut list = col().key("cloud-list").w_full().h(LIST_H).scroll(ScrollSpec { stick_bottom: false });
    match &cloud.rows {
        Rows::Loading => list = list.child(note("Loading…".into())),
        Rows::Failed(error) => list = list.child(text(format!("Couldn't load files: {error}")).key("cloud-error")
            .font(ts(13.0, 400.0)).color(rgb(palette.red)).wrap()),
        Rows::Ready(rows) if rows.is_empty() => list = list.child(note(match cloud.asked {
            Some(Place::Search(_)) => "No files found.".into(),
            Some(Place::Folder(_)) => "This folder is empty.".into(),
            _ => "Nothing here yet.".into(),
        }).key("cloud-empty")),
        Rows::Ready(rows) => {
            for (index, entry) in rows.iter().enumerate() {
                let picked = cloud.selected.iter().any(|file| file.web_url == entry.web_url);
                let mark = if entry.is_folder() { sym("keyboard_arrow_right", 16.0).color(c(MUTED)) }
                    else if picked { sym("check", 16.0).color(rgb(palette.accent_text)) }
                    else { el().size(16.0, 16.0) };
                let pressed = entry.clone();
                list = list.child(row().key(format!("cloud-row-{index}")).w_full().h(ROW_H).px(8.0).gap(10.0).center_y().radius(6.0)
                    .hover(hover)
                    .on_press(move |s: &mut UiState<A>| press_row(s, pressed.clone()))
                    .child(mark)
                    .child(text(entry.name.clone()).font(ts(13.0, if picked { 600.0 } else { 400.0 })).color(c(INK)).grow(1.0).min_w(0.0))
                    .child(text(detail(entry)).font(ts(12.0, 400.0)).color(c(MUTED))));
            }
        }
    }

    let count = cloud.selected.len();
    let footer = row().key("cloud-footer").w_full().gap(8.0).center_y()
        .child(text(if count == 0 { String::new() } else { format!("{count} selected") }).font(ts(12.0, 400.0)).color(c(MUTED)).grow(1.0))
        .child(crate::screens::button("Cancel", "cloud-cancel", |s: &mut UiState<A>, tx| { close(s); tx.focus("composer", false); }))
        .child(row().key("cloud-attach").h(32.0).px(12.0).center().radius(6.0)
            .fill(if count == 0 { c(LINE) } else { rgb(palette.accent) })
            .on_press_tx(|s: &mut UiState<A>, tx: &mut Tx| {
                if s.compose.cloud.selected.is_empty() { return; }
                attach(s);
                tx.focus("composer", false);
            })
            .child(text("Attach").font(ts(13.0, 600.0)).color(if count == 0 { c(MUTED) } else { rgb(palette.on_accent) })));

    col()
        .key("cloud-picker")
        .w_full()
        .padding(8.0)
        .gap(6.0)
        .radius(6.0)
        .border(1.0, rgb(palette.border_strong))
        .fill(rgb(palette.surface))
        .child(tabs)
        .child(search_box)
        .children(crumbs)
        .child(list)
        .child(footer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(name: &str) -> CloudFile {
        CloudFile { name: name.into(), size: 1, web_url: format!("https://x/{name}"), drive_id: None, item_id: None, last_opened: None }
    }

    #[test]
    fn found_files_go_by_link_and_listed_ones_by_id() {
        let by_link = cloud_file(&entry(found("a.docx"), None));
        assert_eq!((by_link.drive_id, by_link.item_id), (None, None));
        let listed = CloudFile { drive_id: Some("d".into()), item_id: Some("i".into()), ..found("b.docx") };
        let by_id = cloud_file(&entry(listed, Some("Ada".into())));
        assert_eq!((by_id.drive_id.as_deref(), by_id.item_id.as_deref()), (Some("d"), Some("i")));
    }

    #[test]
    fn answers_for_another_place_are_dropped() {
        let mut state = State { asked: Some(Place::Shared), ..Default::default() };
        state.loaded(Place::Recent, Ok(vec![entry(found("a"), None)]));
        assert!(matches!(state.rows, Rows::Loading));
        state.loaded(Place::Shared, Err("403".into()));
        assert!(matches!(state.rows, Rows::Failed(_)));
    }

    #[test]
    fn detail_joins_who_and_when() {
        let mut row = entry(found("a"), Some("Ada".into()));
        assert_eq!(detail(&row), "Ada");
        row.modified_at = Some(chrono::DateTime::from_timestamp(0, 0).unwrap());
        assert_eq!(detail(&row), "Ada · 1970-01-01");
    }
}
