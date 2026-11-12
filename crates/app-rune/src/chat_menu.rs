//! Chat management overlays: the chat context menu (right-click or the "..." on a row), the
//! leave confirmation, the new-chat panel and the members panel. All four float above the window
//! over a click-away backdrop. GPUI: `views/chat_menu.rs`.
//!

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

use crate::icons::sym;
use crate::types::Actions;
use crate::ui::{open_chat, ts, UiState};
use crate::view_chatlist::tone;

const MENU_W: f32 = 220.0;
const ITEM_H: f32 = 32.0;
/// The menu's height is not measured; keep its anchor this far above the window bottom guess.
const MAX_ANCHOR_Y: f32 = 480.0;
/// Where the panels sit: under the chat list header, and under the conversation header.
const NEW_CHAT_AT: (f32, f32) = (76.0, 100.0);
const MEMBERS_TOP: f32 = 110.0;

/// Open overlays and the pending chat to open. Lives on `ChatListState`.
#[derive(Default)]
pub struct Manage {
    pub submenu: Option<&'static str>,
    pub remove_history: Option<String>,
    /// Chat id and window point of the open context menu.
    pub menu: Option<(String, f32, f32)>,
    /// Last pointer point over a row's "..." button, where its menu opens.
    pub anchor: (f32, f32),
    /// The group the leave confirmation asks about.
    pub leave: Option<String>,
    pub new_chat_open: bool,
    pub new_chat: crate::new_chat::Draft,
    pub members_open: bool,
    pub adding: bool,
    pub add: crate::new_chat::Draft,
    /// A chat the bridge just created; `drain` opens it and focuses the composer.
    pub pending_open: Option<String>,
}

impl Manage {
    fn any(&self) -> bool {
        self.menu.is_some() || self.leave.is_some() || self.remove_history.is_some() || self.new_chat_open || self.members_open
    }

    fn close(&mut self) {
        self.menu = None;
        self.submenu = None;
        self.remove_history = None;
        self.leave = None;
        self.new_chat_open = false;
        self.members_open = false;
        self.adding = false;
    }
}

/// Called from the app's poll loop, where a `Tx` exists: opens a freshly created chat.
pub fn drain<A: Actions>(s: &mut UiState<A>, tx: &mut Tx) {
    if let Some(id) = s.chatlist.manage.pending_open.take() {
        open_chat(s, &id, tx);
        tx.focus("composer", false);
    }
}

pub fn open_menu<A: Actions>(s: &mut UiState<A>, id: &str, x: f32, y: f32) {
    s.chatlist.manage.submenu = None;
    s.chatlist.manage.menu = Some((id.to_owned(), x, y));
}

/// The floating layer; an empty element while nothing is open.
pub fn overlay<A: Actions + 'static>(s: &UiState<A>) -> El {
    let manage = &s.chatlist.manage;
    if !manage.any() {
        return el();
    }
    let layer = el().key("overlay").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0));
    let backdrop = el()
        .key("overlay-backdrop")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .on_press(|s: &mut UiState<A>| s.chatlist.manage.close());
    let layer = layer.child(backdrop);
    if let Some(id) = &manage.remove_history { return layer.child(confirm_dialog(s, id, true)); }
    if let Some(id) = &manage.leave {
        return layer.child(confirm_dialog(s, id, false));
    }
    if let Some((id, x, y)) = &manage.menu {
        return layer.child(menu(s, id, *x, *y));
    }
    if manage.new_chat_open {
        return layer.child(crate::new_chat::panel(s).pin(Some(NEW_CHAT_AT.0), Some(NEW_CHAT_AT.1), None, None));
    }
    match s.selected.as_deref() {
        Some(id) => layer.child(crate::members::panel(s, id).pin(None, Some(MEMBERS_TOP), Some(16.0), None)),
        None => layer,
    }
}

fn is_group<A: Actions>(s: &UiState<A>, id: &str) -> bool {
    s.details.get(id).is_some_and(|d| d.members.iter().filter(|m| !m.is_bot()).count() > 2)
}

fn menu<A: Actions + 'static>(s: &UiState<A>, id: &str, x: f32, y: f32) -> El {
    let p = active_palette();
    let unread = s.chats.iter().find(|chat| chat.id == id).is_some_and(|chat| chat.unread);
    let muted = s.details.get(id).is_some_and(|d| d.muted);
    let notes = teams_core::is_notes_chat(id);
    if let Some(page) = s.chatlist.manage.submenu {
        let mut panel = col().key("chat-menu").w(280.0).padding(4.0).radius(8.0)
            .fill(tone(p.popup)).border(1.0, tone(p.border_strong))
            .child(submenu_button::<A>("menu-back", "Back", None));
        if page == "sections" {
            let current = s.chatlist.folders.iter().find(|folder| folder.conversation_ids.iter().any(|chat| chat == id));
            for folder in s.chatlist.folders.iter().filter(|folder| matches!(folder.kind, teams_core::FolderKind::Favorites | teams_core::FolderKind::UserCreated)) {
                let (chat, target) = (id.to_owned(), folder.id.clone());
                if current.is_some_and(|current| current.id == folder.id) { continue; }
                let label = if folder.kind == teams_core::FolderKind::Favorites { "Favorites" } else { &folder.name };
                panel = panel.child(item(&format!("menu-move-{}", folder.id), "folder", label, false,
                    move |s: &mut UiState<A>| s.actions.move_to_folder(&chat, &target)));
            }
        } else {
            let mode = if muted { teams_core::ChatNotificationMode::Mute } else { teams_core::ChatNotificationMode::AllNewMessages };
            let desktop = s.details.get(id).map(|details| details.desktop_notifications).unwrap_or_default();
            for (key, label, value) in [("all", "All new messages", teams_core::ChatNotificationMode::AllNewMessages), ("mute", "Mute", teams_core::ChatNotificationMode::Mute)] {
                let chat = id.to_owned();
                panel = panel.child(item(&format!("menu-notify-{key}"), if value == mode { "check" } else { "notifications" }, label, false,
                    move |s: &mut UiState<A>| s.actions.set_chat_notifications(&chat, value, desktop)));
            }
            for (key, label, value) in [("match", "Match settings used for all chats", teams_core::DesktopNotifications::MatchAllChats), ("off", "Don't get desktop notifications", teams_core::DesktopNotifications::Off)] {
                let chat = id.to_owned();
                panel = panel.child(item(&format!("menu-desktop-{key}"), if value == desktop { "check" } else { "notifications" }, label, false,
                    move |s: &mut UiState<A>| s.actions.set_chat_notifications(&chat, mode, value)));
            }
        }
        return el().pin(Some(x), Some(y.min(MAX_ANCHOR_Y)), None, None).child(panel);
    }
    let mut panel = col()
        .key("chat-menu")
        .w(MENU_W)
        .padding(4.0)
        .radius(8.0)
        .fill(tone(p.surface))
        .border(1.0, tone(p.border_strong))
        .child(item("menu-read", if unread { "done_all" } else { "chat_bubble" }, if unread { "Mark as read" } else { "Mark as unread" }, false, {
            let id = id.to_owned();
            move |s: &mut UiState<A>| s.actions.set_chat_read(&id, unread)
        }));
    if !s.chatlist.folders.is_empty() {
        panel = panel.child(submenu_button::<A>("menu-sections", "Move to section", Some("sections")));
        if let Some(folder) = s.chatlist.folders.iter().find(|folder| folder.conversation_ids.iter().any(|chat| chat == id)) {
            let (chat, folder) = (id.to_owned(), folder.id.clone());
            panel = panel.child(item("menu-remove-section", "folder", "Remove from section", false,
                move |s: &mut UiState<A>| s.actions.remove_from_folder(&chat, &folder)));
        }
    }
    if !notes {
        panel = panel.child(submenu_button::<A>("menu-notifications", "Manage notifications", Some("notifications")));
        panel = panel.child(item("menu-mute", if muted { "notifications" } else { "notifications_off" }, if muted { "Unmute" } else { "Mute" }, false, {
            let id = id.to_owned();
            move |s: &mut UiState<A>| s.actions.set_chat_muted(&id, !muted)
        }));
        let chat = id.to_owned();
        panel = panel.child(row().key("menu-copy-link").w_full().h(ITEM_H).px(8.0).gap(10.0).center_y().radius(6.0).hover(tone(p.popup_hover))
            .label("Copy link").on_press_tx(move |s: &mut UiState<A>, tx| {
                tx.copy(teams_core::chat_link(&chat));
                s.chatlist.manage.close();
                s.toast.show("Link copied");
            }).child(sym("link", 16.0).color(tone(p.foreground)))
            .child(text("Copy link").font(ts(13.0, 400.0)).color(tone(p.foreground))));
        if !is_group(s, id) {
            let chat = id.to_owned();
            panel = panel.child(item("menu-remove-history", "delete", "Remove chat history", true,
                move |s: &mut UiState<A>| s.chatlist.manage.remove_history = Some(chat.clone())));
        }
    }
    panel = panel.child(item("menu-hide", "close", "Hide", false, {
        let id = id.to_owned();
        move |s: &mut UiState<A>| s.actions.hide_chat(&id)
    }));
    if is_group(s, id) && !notes {
        panel = panel.child(item("menu-leave", "close", "Leave chat...", true, {
            let id = id.to_owned();
            move |s: &mut UiState<A>| s.chatlist.manage.leave = Some(id.clone())
        }));
    }
    el().pin(Some(x), Some(y.min(MAX_ANCHOR_Y)), None, None).child(panel)
}

fn submenu_button<A: Actions + 'static>(key: &str, label: &str, page: Option<&'static str>) -> El {
    let p = active_palette();
    row().key(key.to_owned()).w_full().h(ITEM_H).px(8.0).gap(10.0).center_y().radius(6.0).hover(tone(p.popup_hover))
        .label(label.to_owned()).on_press(move |s: &mut UiState<A>| s.chatlist.manage.submenu = page)
        .child(sym(if page.is_some() { "keyboard_arrow_right" } else { "keyboard_arrow_left" }, 16.0).color(tone(p.foreground)))
        .child(text(label.to_owned()).font(ts(13.0, 400.0)).color(tone(p.foreground)))
}

/// One menu row. The menu closes after the action unless the action opens a dialog.
fn item<A: Actions + 'static>(key: &str, glyph: &str, label: &str, danger: bool, act: impl Fn(&mut UiState<A>) + 'static) -> El {
    let p = active_palette();
    let color = tone(if danger { p.red_soft } else { p.foreground });
    row()
        .key(key.to_owned())
        .w_full()
        .h(ITEM_H)
        .px(8.0)
        .gap(10.0)
        .center_y()
        .radius(6.0)
        .hover(tone(p.popup_hover))
        .on_press(move |s: &mut UiState<A>| {
            s.chatlist.manage.menu = None;
            act(s);
        })
        .child(sym(glyph, 16.0).color(color))
        .child(text(label.to_owned()).font(ts(13.0, 400.0)).color(color))
}

fn confirm_dialog<A: Actions + 'static>(s: &UiState<A>, id: &str, history: bool) -> El {
    let p = active_palette();
    let title = s.chats.iter().find(|chat| chat.id == id).map_or(id, |chat| chat.title.as_str());
    let others = s.details.get(id).map_or(0, |d| d.members.len().saturating_sub(1));
    let leave_id = id.to_owned();
    let dialog = col()
        .key(if history { "remove-history-dialog" } else { "leave-dialog" })
        .w(360.0)
        .padding(16.0)
        .gap(10.0)
        .radius(10.0)
        .fill(tone(p.surface))
        .border(1.0, tone(p.border_strong))
        .child(text(if history { "Remove chat history?".into() } else { format!("Leave \"{title}\"?") }).font(ts(15.0, 600.0)).color(tone(p.foreground)))
        .child(text(if history { "This will remove the chat from your list and search. It affects you only.".into() } else { format!("The {others} other members see that you left. You can only come back if someone adds you.") })
            .font(ts(13.0, 400.0)).color(tone(p.text_soft)))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .child(el().grow(1.0))
                .child(crate::screens::button("Cancel", if history { "remove-history-cancel" } else { "leave-cancel" }, |s: &mut UiState<A>, _| s.chatlist.manage.close()))
                .child(crate::screens::button(if history { "Remove" } else { "Leave" }, if history { "remove-history-confirm" } else { "leave-confirm" }, move |s: &mut UiState<A>, _| {
                    if history { s.actions.remove_chat_history(&leave_id); } else { s.actions.leave_chat(&leave_id); }
                    s.chatlist.manage.close();
                })),
        );
    // Centred on the pointer-free middle of the window; `pin` has no centring, so offset from the top left.
    el().pin(Some(420.0), Some(200.0), None, None).child(dialog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatDetails, ChatRow, Person, SendTarget, Update};
    use rune::{Env, Runtime};
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake { calls: RefCell<Vec<String>> }

    impl Fake {
        fn log(&self, line: String) { self.calls.borrow_mut().push(line); }
    }

    impl Actions for Fake {
        fn open(&self, id: &str) { self.log(format!("open {id}")); }
        fn send(&self, _t: SendTarget, _x: String, _f: Vec<teams_core::FileReference>) {}
        fn set_chat_muted(&self, id: &str, muted: bool) { self.log(format!("mute {id} {muted}")); }
        fn set_chat_read(&self, id: &str, read: bool) { self.log(format!("read {id} {read}")); }
        fn hide_chat(&self, id: &str) { self.log(format!("hide {id}")); }
        fn leave_chat(&self, id: &str) { self.log(format!("leave {id}")); }
        fn search_people(&self, q: &str) { self.log(format!("people {q}")); }
        fn create_chat(&self, ids: Vec<String>, _t: Option<String>) { self.log(format!("create {}", ids.join(","))); }
        fn add_members(&self, id: &str, people: Vec<store::MemberRecord>) { self.log(format!("add {id} {}", people.len())); }
        fn remove_member(&self, id: &str, user: &str) { self.log(format!("remove {id} {user}")); }
    }

    fn member(id: &str) -> store::MemberRecord {
        store::MemberRecord { user_id: Some(id.into()), display_name: id.to_uppercase() }
    }

    fn rt() -> Runtime<UiState<Fake>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Fake::default()), crate::ui::view::<Fake>, (1280.0, 800.0), env);
        rt.frame(0.0);
        rt.update(|s, _| {
            let row = |id: &str, unread| ChatRow { id: id.into(), title: id.into(), preview: String::new(), unread, when: String::new() };
            s.apply(Update::Chats(vec![row("duo", true), row("crew", false)]));
            s.apply(Update::Details(HashMap::from([
                ("duo".into(), ChatDetails { members: vec![member("me"), member("ann")], ..Default::default() }),
                ("crew".into(), ChatDetails { members: vec![member("me"), member("ann"), member("bob")], muted: true, ..Default::default() }),
            ])));
        });
        rt.frame(1.0 / 60.0);
        rt
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn right_click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.secondary_down(x, y);
        rt.secondary_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn calls(rt: &Runtime<UiState<Fake>>) -> Vec<String> { rt.state.actions.calls.borrow().clone() }

    #[test]
    fn right_click_menu_reads_mutes_hides_and_leaves_groups_only() {
        let mut rt = rt();
        right_click(&mut rt, "chat-duo");
        assert!(rt.find("menu-read").is_some() && rt.find("menu-leave").is_none());
        click(&mut rt, "menu-read");
        assert!(rt.find("chat-menu").is_none());
        right_click(&mut rt, "chat-crew");
        click(&mut rt, "menu-mute");
        right_click(&mut rt, "chat-crew");
        click(&mut rt, "menu-hide");
        right_click(&mut rt, "chat-crew");
        click(&mut rt, "menu-leave");
        assert_eq!(calls(&rt), ["read duo true", "mute crew false", "hide crew"]);
        click(&mut rt, "leave-confirm");
        assert_eq!(calls(&rt).last().unwrap(), "leave crew");
        assert!(rt.find("leave-dialog").is_none());
    }

    #[test]
    fn the_more_button_opens_the_same_menu_and_the_backdrop_closes_it() {
        let mut rt = rt();
        click(&mut rt, "chat-more-crew");
        assert!(rt.find("chat-menu").is_some());
        click(&mut rt, "overlay-backdrop");
        assert!(rt.find("chat-menu").is_none());
        assert!(calls(&rt).is_empty());
    }

    #[test]
    fn new_chat_searches_people_and_creates_or_reopens_chats() {
        let mut rt = rt();
        click(&mut rt, "chat-new");
        click(&mut rt, "new-chat-query");
        rt.key(KeyEvent { key: Key::Text("zo".into()), mods: Mods::default() });
        rt.frame(1.0 / 60.0);
        assert_eq!(calls(&rt), ["people zo"]);
        let zoe = Person { user_id: "zoe".into(), name: "Zoe".into(), mail: None };
        rt.update(|s, _| s.apply(Update::People { query: "zo".into(), rows: vec![zoe] }));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "new-chat-result-0");
        click(&mut rt, "new-chat-start");
        assert_eq!(calls(&rt).last().unwrap(), "create zoe");
        rt.update(|s, tx| { s.apply(Update::ChatCreated("fresh".into())); drain(s, tx); });
        assert_eq!(rt.state.selected.as_deref(), Some("fresh"));

        // A known 1:1 is opened instead of created.
        click(&mut rt, "chat-new");
        let ann = Person { user_id: "ann".into(), name: "ANN".into(), mail: None };
        rt.update(|s, _| s.chatlist.manage.new_chat.chips.push(ann));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "new-chat-start");
        assert_eq!(rt.state.selected.as_deref(), Some("duo"));
        assert!(!calls(&rt).iter().any(|c| c == "create ann"));
    }

    #[test]
    fn members_panel_lists_adds_and_removes() {
        let mut rt = rt();
        click(&mut rt, "chat-crew");
        click(&mut rt, "participants");
        assert!(rt.find("member-2").is_some());
        click(&mut rt, "member-remove-2");
        assert_eq!(calls(&rt).last().unwrap(), "remove crew bob");
        click(&mut rt, "members-add");
        let cid = Person { user_id: "cid".into(), name: "Cid".into(), mail: None };
        rt.update(|s, _| s.chatlist.manage.add.chips.push(cid));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "members-add-confirm");
        assert_eq!(calls(&rt).last().unwrap(), "add crew 1");
        // 1:1 members cannot be removed.
        click(&mut rt, "overlay-backdrop");
        click(&mut rt, "chat-duo");
        click(&mut rt, "participants");
        assert!(rt.find("member-0").is_some() && rt.find("member-remove-0").is_none());
    }
}
