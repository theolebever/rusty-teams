//! The chat list pane: header, search, filter chips, and the collapsible Quick views, Teams and
//! Chats sections. Sizes and colours follow the GPUI sidebar (`crates/app/src/views/sidebar.rs`
//! and `sidebar/chat_filter_bar.rs`), read from the same palette the rest of the theme uses.

use std::borrow::Cow;
use std::rc::Rc;

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

use crate::avatar::avatar;
use crate::icons::sym;
use crate::types::{Actions, ChannelRow, ChatRow};
use crate::ui::{UiState, open_chat, ts};

const HEADER_H: f32 = 52.0;
const CHAT_ROW_H: f32 = 54.0;
const SECTION_H: f32 = 30.0;
const CHIP_H: f32 = 24.0;
const ICON_BUTTON: f32 = 32.0;
const AVATAR: f32 = 36.0;
const TEAM_AVATAR: f32 = 20.0;
const MEETING_PREFIX: &str = "19:meeting_";

const CHATS: &str = "chats";
const TEAMS: &str = "teams";
const QUICK_VIEWS: &str = "quick-views";
const QUICK: [(&str, &str); 3] = [
    ("Mentions", "alternate_email"),
    ("Drafts", "drafts"),
    ("Saved", "bookmark"),
];

/// The chip toggles and the collapsed sections. Lives on `UiState` between frames.
#[derive(Default)]
pub struct ChatListState {
    pub team_details: std::collections::HashMap<String, crate::types::TeamDetails>,
    pub channel_details: std::collections::HashMap<String, crate::types::ChannelDetails>,
    pub pinned_channels: Vec<String>,
    pub show_hidden_teams: bool,
    pub team_menu: crate::team_menu::State,
    pub copy_link: Option<String>,
    pub folders: Vec<teams_core::ChatFolder>,
    pub favorites: Vec<String>,
    pub has_older: bool,
    pub loading_older: bool,
    pub new_section: bool,
    pub section_name: TextEdit,
    pub more: bool,
    pub filter_open: bool,
    pub filter: TextEdit,
    pub quick_view: Option<crate::quick_views::QuickView>,
    pub saved: Option<Result<Vec<teams_core::SavedMessage>, String>>,
    pub show_hidden_channels: Vec<String>,
    pub filters: Vec<Pill>,
    /// Section ids (`CHATS`, `TEAMS`, `QUICK_VIEWS`) folded shut.
    pub collapsed: Vec<String>,
    /// Context menu, new-chat and members panels (`chat_menu.rs`).
    pub manage: crate::chat_menu::Manage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pill {
    Unread,
    Chats,
    Meetings,
    Mentions,
    Muted,
}

impl Pill {
    const ALL: [Pill; 3] = [Pill::Unread, Pill::Chats, Pill::Meetings];

    fn key(self) -> &'static str {
        match self {
            Pill::Unread => "unread",
            Pill::Chats => "chats",
            Pill::Meetings => "meetings",
            Pill::Mentions => "mentions",
            Pill::Muted => "muted",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Pill::Unread => "Unread",
            Pill::Chats => "Chats",
            Pill::Meetings => "Meeting chats",
            Pill::Mentions => "@Mentions",
            Pill::Muted => "Muted",
        }
    }
}

/// Indices into `chats` that the chips let through. Unread narrows to unread chats; the two kind
/// chips pick a side (both or neither means every kind). Teams meeting chats have ids that start
/// with `19:meeting_`, which is all the bridge row carries to tell them apart.
pub fn visible_chats(state: &ChatListState, chats: &[ChatRow]) -> Vec<usize> {
    let on = |pill| state.filters.contains(&pill);
    let (chats_on, meetings_on) = (on(Pill::Chats), on(Pill::Meetings));
    chats
        .iter()
        .enumerate()
        .filter(|(_, chat)| {
            let kind = match (chats_on, meetings_on) {
                (true, false) => !is_meeting(&chat.id),
                (false, true) => is_meeting(&chat.id),
                _ => true,
            };
            kind && (!on(Pill::Unread) || chat.unread)
                && chat.title.to_lowercase().contains(&state.filter.text.trim().to_lowercase())
        })
        .map(|(index, _)| index)
        .collect()
}

/// Turns a filter chip on or off; a chip whose opposite is on stays off.
pub(crate) fn toggle_filter<A: Actions>(s: &mut UiState<A>, pill: Pill) {
    let excludes = match pill {
        Pill::Mentions => Some(Pill::Muted), Pill::Muted => Some(Pill::Mentions),
        Pill::Chats => Some(Pill::Meetings), Pill::Meetings => Some(Pill::Chats), _ => None,
    };
    if !excludes.is_some_and(|other| s.chatlist.filters.contains(&other)) { flip(&mut s.chatlist.filters, &pill); }
}

pub(crate) fn filtered_chats<A: Actions>(s: &UiState<A>) -> Vec<usize> {
    visible_chats(&s.chatlist, &s.chats).into_iter().filter(|&i| {
        let id = &s.chats[i].id;
        (!s.chatlist.filters.contains(&Pill::Muted) || s.details.get(id).is_some_and(|d| d.muted))
            && (!s.chatlist.filters.contains(&Pill::Mentions) || s.screens.activity.items.iter().any(|item| crate::quick_views::is_mention(&item.activity_type) && !s.screens.activity.is_read(item) && &item.source_thread_id == id))
    }).collect()
}

pub(crate) fn open_filter<A: Actions>(s: &mut UiState<A>, tx: &mut Tx) {
    s.chatlist.filter_open = true;
    tx.focus("chat-filter-input", false);
}

fn filter_field<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    s.chatlist.filter_open.then(|| row().w_full().px(16.0).pb(8.0).gap(6.0).center_y()
        .child(text_input(&s.chatlist.filter, ts(13.0, 400.0)).key("chat-filter-input")
            .label("Filter by person or chat name").placeholder("Filter by person or chat name", tone(active_palette().text_muted))
            .color(tone(active_palette().foreground)).grow(1.0).min_w(0.0)
            .on_edit(|s: &mut UiState<A>| &mut s.chatlist.filter))
        .child(row().key("chat-filter-clear").size(24.0, 24.0).center().label("Clear filter text")
            .on_press(|s: &mut UiState<A>| s.chatlist.filter.clear())
            .child(sym("close", 14.0).color(tone(active_palette().text_muted)))))
}

fn is_meeting(id: &str) -> bool {
    id.starts_with(MEETING_PREFIX)
}

/// Colour from the active palette, as an rune colour.
pub(crate) fn tone(value: u32) -> Color {
    crate::avatar::rgb(value)
}

fn flip<T: PartialEq + Clone>(items: &mut Vec<T>, item: &T) {
    match items.iter().position(|existing| existing == item) {
        Some(index) => {
            items.remove(index);
        }
        None => items.push(item.clone()),
    }
}

fn collapsed<A: Actions>(s: &UiState<A>, id: &str) -> bool {
    s.chatlist.collapsed.iter().any(|item| item == id)
}

pub fn pane<A: Actions + 'static>(s: &UiState<A>, width: f32) -> El {
    let searching = s.global_search.results.is_some();
    let mut list = col()
        .key("chat-list")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .scroll(ScrollSpec { stick_bottom: false });
    if searching {
        list = list.child(crate::search::results_list(s));
    } else {
        let unread = s.chats.iter().filter(|chat| chat.unread).count() as u32;
        // Rune: a virtual/growth stack must be the last child of its scroll viewport.
        list = list
            .child(quick_views(s))
            .child(teams(s))
            .child(folder_rows(s))
            .child(col().w_full().px(6.0).child(section_header(s, CHATS, "Chats", unread)))
            .child(chat_rows(s));
    }
    col()
        .key("chats")
        .w(width)
        .h_full()
        .min_h(0.0)
        .fill(tone(active_palette().surface))
        .child(header::<A>())
        .child(status(s))
        .child(crate::search::search_box(s))
        .children(filter_field(s))
        .child(filter_chips(s))
        .child(list)
        .children(s.chatlist.has_older.then(|| crate::screens::button(
            if s.chatlist.loading_older { "Loading…" } else { "Load older chats" }, "chats-load-older", |s: &mut UiState<A>, _| {
                if !s.chatlist.loading_older { s.chatlist.loading_older = true; s.actions.load_older_chats(); }
            })))
}

fn header<A: Actions + 'static>() -> El {
    let p = active_palette();
    let mut bar = row()
        .key("chat-header")
        .w_full()
        .h(HEADER_H)
        .pl(16.0)
        .pr(12.0)
        .gap(6.0)
        .center_y()
        .child(
            text("Chat")
                .font(ts(16.0, 700.0))
                .color(tone(p.foreground))
                .grow(1.0)
                .min_w(0.0),
        );
    for (key, glyph) in [
        ("chat-more", "more_horiz"),
        ("chat-filter", "filter_alt"),
        ("chat-new", "edit"),
    ] {
        let mut button = row()
            .key(key)
            .size(ICON_BUTTON, ICON_BUTTON)
            .center()
            .radius(8.0)
            .hover(tone(p.popup_hover))
            .child(sym(glyph, 16.0).color(tone(p.text_soft)));
        button = match key {
            "chat-new" => button.label("New chat").help("New chat (Ctrl+N)").on_press(|s: &mut UiState<A>| crate::new_chat::open(s)),
            "chat-more" => button.label("More options").help("More options").on_press(|s: &mut UiState<A>| s.chatlist.more = !s.chatlist.more),
            _ => button.label("Filter chats").help("Filter chats (Ctrl+Shift+F)").on_press_tx(|s: &mut UiState<A>, tx| {
                if s.chatlist.filter_open {
                    s.chatlist.filter_open = false;
                    s.chatlist.filter.clear();
                } else { open_filter(s, tx); }
            }),
        };
        bar = bar.child(button);
    }
    bar
}

fn status<A: Actions>(s: &UiState<A>) -> El {
    if s.status.is_empty() {
        return el();
    }
    row().w_full().px(16.0).pb(4.0).child(
        text(s.status.clone())
            .font(ts(12.0, 400.0))
            .color(tone(active_palette().text_muted))
            .truncate()
            .grow(1.0)
            .min_w(0.0),
    )
}

fn filter_chips<A: Actions + 'static>(s: &UiState<A>) -> El {
    col().w_full().child(chips(s, &Pill::ALL)).children(s.chatlist.filter_open.then(|| chips(s, &[Pill::Mentions, Pill::Muted])))
}

fn chips<A: Actions + 'static>(s: &UiState<A>, pills: &[Pill]) -> El {
    let p = active_palette();
    let mut bar = row().w_full().px(16.0).pb(8.0).gap(6.0);
    for &pill in pills {
        let on = s.chatlist.filters.contains(&pill);
        bar = bar.child(
            row()
                .key(format!("filter-{}", pill.key()))
                .h(CHIP_H)
                .px(12.0)
                .center_y()
                .radius(999.0)
                .fill(tone(if on { p.accent } else { p.surface_raised }))
                .hover(tone(if on { p.accent } else { p.border_strong }))
                .label(pill.label())
                .on_press(move |s: &mut UiState<A>| toggle_filter(s, pill))
                .child(
                    text(pill.label())
                        .font(ts(12.0, 400.0))
                        .color(tone(if on { p.on_accent } else { p.text_soft })),
                ),
        );
    }
    bar
}

/// A collapsible header with a chevron. Shows the unread count as a pill while folded.
fn section_header<A: Actions + 'static>(s: &UiState<A>, id: &str, label: &str, unread: u32) -> El {
    let p = active_palette();
    let open = !collapsed(s, id);
    let key = id.to_owned();
    let mut header = row()
        .key(format!("section-{id}"))
        .w_full()
        .h(SECTION_H)
        .mt(4.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .hover(tone(p.row_hover))
        .on_press(move |s: &mut UiState<A>| flip(&mut s.chatlist.collapsed, &key))
        .child(
            sym(if open { "keyboard_arrow_down" } else { "keyboard_arrow_right" }, 12.0)
                .color(tone(p.text_muted)),
        )
        .child(
            text(label.to_owned())
                .font(ts(12.0, 600.0))
                .color(tone(p.text_soft))
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        );
    if !open && unread > 0 {
        header = header.child(count_pill(unread));
    }
    header
}

fn count_pill(count: u32) -> El {
    let p = active_palette();
    row()
        .h(18.0)
        .min_w(18.0)
        .px(5.0)
        .center()
        .radius(9.0)
        .fill(tone(p.accent))
        .child(text(count.to_string()).font(ts(11.0, 600.0)).color(tone(p.on_accent)))
}

fn quick_views<A: Actions + 'static>(s: &UiState<A>) -> El {
    let p = active_palette();
    let mut list = col()
        .key("quick-views")
        .w_full()
        .px(6.0)
        .child(section_header(s, QUICK_VIEWS, "Quick views", 0));
    if collapsed(s, QUICK_VIEWS) {
        return list;
    }
    for (label, glyph) in QUICK {
        list = list.child(
            row()
                .key(format!("quick-{label}"))
                .w_full()
                .h(32.0)
                .px(10.0)
                .gap(10.0)
                .center_y()
                .radius(8.0)
                .hover(tone(p.row_hover))
                .on_press(move |s: &mut UiState<A>| {
                    let view = match label {
                        "Mentions" => crate::quick_views::QuickView::Mentions,
                        "Drafts" => crate::quick_views::QuickView::Drafts,
                        _ => crate::quick_views::QuickView::Saved,
                    };
                    crate::screens::navigate(s, crate::screens::Screen::Chat);
                    s.chatlist.quick_view = Some(view);
                    if view == crate::quick_views::QuickView::Saved {
                        s.chatlist.saved = None;
                        s.actions.load_saved();
                    }
                })
                .child(sym(glyph, 18.0).color(tone(p.text_soft)))
                .child(
                    text(label)
                        .font(ts(13.0, 400.0))
                        .color(tone(p.foreground))
                        .truncate(),
                ),
        );
    }
    list
}

pub fn more_menu<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    if !s.chatlist.more { return None; }
    let p = active_palette();
    let mut menu = col().key("chat-more-menu").w(200.0).padding(6.0).gap(4.0).radius(8.0)
        .fill(tone(p.popup)).border(1.0, tone(p.border))
        .anchored("chat-more", Side::Bottom, Align::End, 4.0)
        .modal(true, |s: &mut UiState<A>| s.chatlist.more = false);
    if !s.read_only {
        menu = menu.child(crate::screens::button("Mark all as read", "chats-read-all", |s: &mut UiState<A>, _| {
            s.chatlist.more = false;
            for chat in s.chats.iter().filter(|chat| chat.unread) { s.actions.set_chat_read(&chat.id, true); }
        }));
        menu = menu.child(crate::screens::button("New section", "chats-new-section", |s: &mut UiState<A>, _| s.chatlist.new_section = true));
        if s.chatlist.new_section {
            menu = menu.child(text_input(&s.chatlist.section_name, ts(13.0, 400.0)).key("section-name").label("Section name")
                .placeholder("Section name", tone(p.text_muted)).color(tone(p.foreground)).w_full()
                .on_edit(|s: &mut UiState<A>| &mut s.chatlist.section_name))
                .child(crate::screens::button("Create", "section-create", |s: &mut UiState<A>, _| {
                    let name = s.chatlist.section_name.text.trim().to_owned();
                    if !name.is_empty() {
                        s.actions.change_folders(teams_core::FolderChange::Create { name });
                        s.chatlist.section_name.clear();
                        s.chatlist.new_section = false;
                        s.chatlist.more = false;
                    }
                }));
        }
    }
    Some(menu.child(crate::screens::button("Collapse all", "chats-collapse-all", |s: &mut UiState<A>, _| {
        s.chatlist.more = false;
        s.chatlist.collapsed = [QUICK_VIEWS, TEAMS, CHATS].map(str::to_owned).to_vec();
        s.expanded_teams.clear();
        for folder in s.chatlist.folders.iter().filter(|folder| folder.expanded) { s.actions.set_folder_expanded(&folder.id, false); }
    })))
}

fn folder_rows<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut list = col().key("chat-sections").w_full().px(6.0);
    let visible = filtered_chats(s);
    for folder in s.chatlist.folders.iter().filter(|folder| matches!(folder.kind, teams_core::FolderKind::Favorites | teams_core::FolderKind::UserCreated)) {
        let indices: Vec<_> = folder.conversation_ids.iter().filter_map(|id| visible.iter().copied().find(|&i| s.chats[i].id == *id)).collect();
        let id = folder.id.clone();
        let expanded = folder.expanded;
        let label = if folder.kind == teams_core::FolderKind::Favorites { "Favorites" } else { &folder.name };
        let mut section = col().key(format!("folder-{}", folder.id)).w_full()
            .child(row().key(format!("section-folder-{}", folder.id)).w_full().h(SECTION_H).px(10.0).gap(6.0).center_y()
                .label(label.to_owned()).hover(tone(active_palette().row_hover))
                .on_press(move |s: &mut UiState<A>| s.actions.set_folder_expanded(&id, !expanded))
                .child(sym(if expanded { "keyboard_arrow_down" } else { "keyboard_arrow_right" }, 12.0).color(tone(active_palette().text_muted)))
                .child(text(label.to_owned()).font(ts(12.0, 600.0)).color(tone(active_palette().text_soft))));
        if expanded {
            for i in indices { section = section.child(col().key(format!("chat-{}", s.chats[i].id)).w_full().child(chat_row(s, i))); }
        }
        list = list.child(section);
    }
    if s.chatlist.folders.is_empty() && !s.chatlist.favorites.is_empty() {
        list = list.child(section_header(s, "favorites", "Favorites", 0));
        if !collapsed(s, "favorites") {
            for &i in visible.iter().filter(|&&i| s.chatlist.favorites.contains(&s.chats[i].id)) {
                list = list.child(col().key(format!("chat-{}", s.chats[i].id)).w_full().child(chat_row(s, i)));
            }
        }
    }
    list
}

fn in_section<A: Actions>(s: &UiState<A>, id: &str) -> bool {
    if s.chatlist.folders.is_empty() { return s.chatlist.favorites.iter().any(|item| item == id); }
    s.chatlist.folders.iter().filter(|folder| matches!(folder.kind, teams_core::FolderKind::Favorites | teams_core::FolderKind::UserCreated))
        .any(|folder| folder.conversation_ids.iter().any(|item| item == id))
}

pub(crate) fn teams<A: Actions + 'static>(s: &UiState<A>) -> El {
    if s.teams.is_empty() {
        return el();
    }
    let p = active_palette();
    let mut list = col()
        .key("team-list")
        .w_full()
        .px(6.0);
    if s.screens.screen != crate::screens::Screen::Teams { list = list.child(section_header(s, TEAMS, "Teams", 0)); }
    if s.screens.screen != crate::screens::Screen::Teams && collapsed(s, TEAMS) {
        return list;
    }
    if !s.chatlist.pinned_channels.is_empty() {
        list = list.child(text("Pinned channels").font(ts(12.0, 600.0)).color(tone(p.text_soft)).padding(10.0));
        for id in &s.chatlist.pinned_channels {
            if let Some(channel) = s.teams.iter().flat_map(|team| &team.channels).find(|channel| &channel.id == id) {
                list = list.child(col().key(format!("pinned-channel-{id}")).w_full().child(channel_row(s, channel)));
            }
        }
    }
    for team in s.teams.iter().filter(|team| s.chatlist.show_hidden_teams || !s.chatlist.team_details.get(&team.id).is_some_and(|details| details.hidden)) {
        let expanded = s.expanded_teams.contains(&team.id);
        let id = team.id.clone();
        list = list.child(
            row()
                .key(format!("team-{}", team.id))
                .w_full()
                .h(36.0)
                .px(10.0)
                .gap(8.0)
                .center_y()
                .radius(8.0)
                .hover(tone(p.row_hover))
                .on_secondary_press({ let id = team.id.clone(); move |s: &mut UiState<A>, at, _| crate::team_menu::open(s, &id, false, at.x, at.y) })
                .on_press(move |s: &mut UiState<A>| flip(&mut s.expanded_teams, &id))
                .child(
                    sym(if expanded { "keyboard_arrow_down" } else { "keyboard_arrow_right" }, 12.0)
                        .color(tone(p.text_muted)),
                )
                .child(avatar(&team.name, TEAM_AVATAR, None))
                .child(
                    text(team.name.clone())
                        .font(ts(14.0, 600.0))
                        .color(tone(p.foreground))
                        .truncate()
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(crate::team_menu::more::<A>(&team.id, false)),
        );
        if expanded {
            let show_hidden = s.chatlist.show_hidden_channels.contains(&team.id);
            for channel in team.channels.iter().filter(|channel| !channel.hidden || show_hidden) {
                list = list.child(channel_row(s, channel));
            }
            let hidden = team.channels.iter().filter(|channel| channel.hidden).count();
            if hidden > 0 {
                let id = team.id.clone();
                list = list.child(row().key(format!("team-hidden-{}", team.id)).w_full().h(32.0).pl(56.0).center_y()
                    .hover(tone(p.row_hover)).on_press(move |s: &mut UiState<A>| flip(&mut s.chatlist.show_hidden_channels, &id))
                    .child(text(if show_hidden { "Hide hidden channels".into() } else { format!("See all channels ({hidden} hidden)") })
                        .font(ts(12.0, 400.0)).color(tone(p.accent))));
            }
        }
    }
    if s.chatlist.team_details.values().any(|details| details.hidden) {
        list = list.child(crate::screens::button(if s.chatlist.show_hidden_teams { "Hide hidden teams" } else { "See hidden teams" }, "teams-show-hidden",
            |s: &mut UiState<A>, _| s.chatlist.show_hidden_teams = !s.chatlist.show_hidden_teams));
    }
    list
}

fn channel_row<A: Actions + 'static>(s: &UiState<A>, channel: &ChannelRow) -> El {
    let p = active_palette();
    let selected = s.selected.as_deref() == Some(channel.id.as_str());
    let id = channel.id.clone();
    let mut r = row()
        .key(format!("channel-{}", channel.id))
        .w_full()
        .h(32.0)
        .pl(56.0)
        .pr(10.0)
        .center_y()
        .radius(8.0)
        .hover(tone(p.row_hover))
        .on_secondary_press({ let id = channel.id.clone(); move |s: &mut UiState<A>, at, _| crate::team_menu::open(s, &id, true, at.x, at.y) })
        .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| open_chat(s, &id, tx))
        .children(s.chatlist.channel_details.get(&channel.id).filter(|details| details.private)
            .map(|_| sym("lock", 12.0).color(tone(p.text_muted)).label("Private channel")))
        .child(
            text(format!("# {}", channel.name))
                .font(ts(13.0, if channel.unread { 700.0 } else { 400.0 }))
                .color(tone(p.foreground))
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        );
    if let Some(details) = s.chatlist.channel_details.get(&channel.id).filter(|details| !details.mentions.is_empty()) {
        r = r.child(count_pill(details.mentions.personal.saturating_add(details.mentions.channel).saturating_add(details.mentions.team)));
    }
    r = r.child(crate::team_menu::more::<A>(&channel.id, true));
    if selected {
        r = r.fill(tone(p.surface_raised));
    }
    r
}

fn chat_rows<A: Actions + 'static>(s: &UiState<A>) -> El {
    let visible = Rc::new(filtered_chats(s).into_iter().filter(|&i| !in_section(s, &s.chats[i].id)).collect::<Vec<_>>());
    let count = if collapsed(s, CHATS) { 0 } else { visible.len() };
    let fingerprint: Vec<String> = visible.iter().map(|&index| row_state(s, index)).collect();
    let keys = Rc::clone(&visible);
    col()
        .key("chat-rows")
        .w_full()
        .px(6.0)
        .growth(GrowthSpec::default())
        .virtual_rows(
            count,
            CHAT_ROW_H,
            move |s: &UiState<A>, i| Cow::Owned(format!("chat-{}", s.chats[keys[i]].id)),
            move |s: &UiState<A>, i| chat_row(s, visible[i]),
        )
        .virtual_rows_version((fingerprint, crate::theme::version()))
}

/// What a row paints, so a changed row re-keys the cached virtual list.
fn row_state<A: Actions>(s: &UiState<A>, index: usize) -> String {
    let chat = &s.chats[index];
    let selected = s.selected.as_deref() == Some(chat.id.as_str());
    format!("{}|{}|{}|{}|{}|{selected}|{:?}|{}", chat.id, chat.title, chat.preview, chat.unread, chat.when, s.details.get(&chat.id), preview_text(s, &chat.id, &chat.preview))
}

fn preview_text<A: Actions>(s: &UiState<A>, id: &str, fallback: &str) -> String {
    if s.chat_send_failed(id) { return "Not sent".into(); }
    if let Some(draft) = s.chat_draft(id) { return format!("Draft: {draft}"); }
    let names: Vec<_> = s.typing.values().filter(|event| event.conversation_id == id).map(|event| event.display_name.as_str()).collect();
    if !names.is_empty() { return format!("{} is typing…", names.join(", ")); }
    fallback.to_owned()
}

/// One chat: avatar, title and time, the preview, and an unread dot. Keyed by the caller.
pub(crate) fn chat_row<A: Actions + 'static>(s: &UiState<A>, i: usize) -> El {
    let p = active_palette();
    let ch = &s.chats[i];
    let selected = s.selected.as_deref() == Some(ch.id.as_str());
    let id = ch.id.clone();
    let title_weight = if ch.unread { 700.0 } else { 400.0 };
    let time_color = if ch.unread { p.accent_text } else { p.text_muted };
    let preview_color = if ch.unread { p.text_strong } else { p.text_muted };
    let line = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            text(ch.title.clone())
                .font(ts(14.0, title_weight))
                .color(tone(p.foreground))
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(text(ch.when.clone()).font(ts(11.0, 400.0)).color(tone(time_color)));
    let mut preview = row()
        .w_full()
        .h(18.0)
        .gap(8.0)
        .center_y()
        .child(
            text(preview_text(s, &ch.id, &ch.preview))
                .font(ts(12.5, 400.0))
                .color(tone(preview_color))
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        );
    if ch.unread {
        preview = preview.child(el().size(8.0, 8.0).radius(4.0).fill(tone(p.accent)));
    }
    if let Some(details) = s.details.get(&ch.id).filter(|details| details.importance.is_marked()) {
        preview = preview.child(sym("priority_high", 16.0).color(tone(p.red_soft)).label(format!("{:?}", details.importance)));
    }
    let lines = col().grow(1.0).min_w(0.0).gap(2.0).child(line).child(preview);
    let mut card = row()
        .w_full()
        .h(CHAT_ROW_H)
        .px(10.0)
        .gap(12.0)
        .center_y()
        .radius(8.0)
        .hover(tone(p.row_hover))
        .hover_group()
        .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| open_chat(s, &id, tx))
        .on_secondary_press({
            let id = ch.id.clone();
            move |s: &mut UiState<A>, at, _| crate::chat_menu::open_menu(s, &id, at.x, at.y)
        })
        .child(crate::avatar::chat(s, &ch.title, &ch.id, AVATAR))
        .children(s.details.get(&ch.id).filter(|d| d.muted).map(|_| sym("notifications_off", 14.0).color(tone(p.text_muted))))
        .child(lines)
        .child(more_button::<A>(&ch.id));
    if selected {
        card = card.fill(tone(p.surface_raised));
    }
    card
}

/// The "..." that shows while the row is hovered and opens its context menu. The pointer point
/// is tracked over the button because a press carries none.
fn more_button<A: Actions + 'static>(id: &str) -> El {
    let p = active_palette();
    let id = id.to_owned();
    row()
        .key(format!("chat-more-{id}"))
        .pin(None, Some(24.0), Some(6.0), None)
        .size(24.0, 24.0)
        .center()
        .radius(6.0)
        .fill(tone(p.surface_raised))
        .hover(tone(p.popup_hover))
        .group_opacity(0.0, 1.0)
        .on_pointer_move(|s: &mut UiState<A>, at| s.chatlist.manage.anchor = (at.x, at.y))
        .on_press(move |s: &mut UiState<A>| {
            let (x, y) = s.chatlist.manage.anchor;
            crate::chat_menu::open_menu(s, &id, x, y);
        })
        .child(sym("more_horiz", 14.0).color(tone(p.text_soft)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Update;
    use rune::{Env, Runtime};

    struct Quiet;

    impl Actions for Quiet {
        fn open(&self, _conversation_id: &str) {}
        fn send(&self, _target: crate::types::SendTarget, _text: String, _files: Vec<teams_core::FileReference>) {}
    }

    fn chat(id: &str, unread: bool) -> ChatRow {
        ChatRow { id: id.into(), title: id.into(), preview: String::new(), unread, when: String::new() }
    }

    fn rt() -> Runtime<UiState<Quiet>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Quiet), crate::ui::view::<Quiet>, (1000.0, 700.0), env);
        rt.frame(0.0);
        rt.update(|s, _| {
            s.apply(Update::Chats(vec![chat("a", true), chat("b", false), chat("19:meeting_m@thread.v2", true)]));
        });
        rt.frame(1.0 / 60.0);
        rt
    }

    fn click(rt: &mut Runtime<UiState<Quiet>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn hidden_channels_remain_discoverable_without_changing_account_settings() {
        let mut rt = rt();
        rt.update(|s, _| {
            s.screens.screen = crate::screens::Screen::Teams;
            s.teams = vec![crate::types::TeamRow { id: "t".into(), name: "Team".into(), channels: vec![
                crate::types::ChannelRow { id: "public".into(), name: "General".into(), hidden: false, unread: false },
                crate::types::ChannelRow { id: "hidden".into(), name: "Hidden".into(), hidden: true, unread: false },
            ] }];
            s.expanded_teams.push("t".into());
        });
        rt.frame(1.0 / 60.0);
        assert!(rt.find("channel-public").is_some());
        assert!(rt.find("channel-hidden").is_none());
        click(&mut rt, "team-hidden-t");
        assert!(rt.find("channel-hidden").is_some());
        assert!(rt.state.teams[0].channels[1].hidden);
    }

    #[test]
    fn unread_chip_keeps_unread_chats_and_kind_chips_split_meetings() {
        let chats = vec![chat("a", true), chat("b", false), chat("19:meeting_m@thread.v2", true)];
        let mut state = ChatListState::default();
        assert_eq!(visible_chats(&state, &chats), [0, 1, 2]);
        state.filters.push(Pill::Unread);
        assert_eq!(visible_chats(&state, &chats), [0, 2]);
        state.filters = vec![Pill::Meetings];
        assert_eq!(visible_chats(&state, &chats), [2]);
        state.filters = vec![Pill::Chats];
        assert_eq!(visible_chats(&state, &chats), [0, 1]);
    }

    #[test]
    fn clicking_a_chip_narrows_the_built_rows() {
        let mut rt = rt();
        click(&mut rt, "filter-unread");
        assert!(rt.find("chat-a").is_some());
        assert!(rt.find("chat-b").is_none());
    }

    #[test]
    fn filtered_rows_follow_unread_changes_and_reordering() {
        let mut rt = rt();
        click(&mut rt, "filter-unread");
        rt.update(|s, _| {
            s.apply(Update::Chats(vec![chat("b", true), chat("a", false), chat("19:meeting_m@thread.v2", true)]));
        });
        rt.frame(1.0 / 60.0);
        assert!(rt.find("chat-b").is_some());
        assert!(rt.find("chat-a").is_none());
        click(&mut rt, "chat-b");
        assert_eq!(rt.state.selected.as_deref(), Some("b"));
    }

    #[test]
    fn folding_the_chats_header_hides_the_rows() {
        let mut rt = rt();
        click(&mut rt, "section-chats");
        assert!(rt.find("chat-a").is_none());
        click(&mut rt, "section-chats");
        assert!(rt.find("chat-a").is_some());
    }
}
