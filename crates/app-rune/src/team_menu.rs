//! Team and channel menus use the same account operations as the GPUI sidebar.

use app_backend::schemes::active_palette;
use rune::*;

use crate::types::Actions;
use crate::ui::{text, ts, UiState};
use crate::view_chatlist::tone;

#[derive(Default)]
pub struct State {
    menu: Option<(String, bool, f32, f32)>,
    anchor: (f32, f32),
    create_team: Option<String>,
    name: TextEdit,
    description: TextEdit,
    private: bool,
}

pub fn open<A: Actions>(s: &mut UiState<A>, id: &str, channel: bool, x: f32, y: f32) {
    s.chatlist.team_menu.menu = Some((id.into(), channel, x, y));
}

pub fn more<A: Actions + 'static>(id: &str, channel: bool) -> El {
    let id = id.to_owned();
    row().key(format!("team-more-{channel}-{id}")).size(24.0, 24.0).center().radius(4.0)
        .label("More options").help("More options").hover(tone(active_palette().row_hover))
        .on_pointer_move(|s: &mut UiState<A>, at| s.chatlist.team_menu.anchor = (at.x, at.y))
        .on_press(move |s: &mut UiState<A>| { let (x, y) = s.chatlist.team_menu.anchor; open(s, &id, channel, x, y); })
        .child(crate::icons::sym("more_horiz", 14.0).color(tone(active_palette().text_soft)))
}

fn item<A: Actions + 'static>(label: &str, key: &str, action: impl Fn(&mut UiState<A>) + 'static) -> El {
    let p = active_palette();
    row().key(key.to_owned()).w_full().h(32.0).px(8.0).center_y().radius(4.0).label(label.to_owned())
        .hover(tone(p.popup_hover)).on_press(move |s: &mut UiState<A>| { s.chatlist.team_menu.menu = None; action(s); })
        .child(text(label.to_owned()).font(ts(13.0, 400.0)).color(tone(p.foreground)))
}

pub fn layer<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    if let Some(team) = &s.chatlist.team_menu.create_team { return Some(create_dialog(s, team)); }
    let (id, channel, x, y) = s.chatlist.team_menu.menu.as_ref()?;
    let p = active_palette();
    let mut menu = col().key("team-channel-menu").w(260.0).padding(6.0).gap(2.0).radius(8.0)
        .fill(tone(p.popup)).border(1.0, tone(p.border_strong));
    let copy_id = id.clone();
    let is_channel = *channel;
    menu = menu.child(item("Copy link", "team-copy-link", move |s: &mut UiState<A>| s.actions.copy_team_link(&copy_id, is_channel)));
    if !s.read_only {
        if *channel {
            let row = s.channel(id)?;
            let pinned = s.chatlist.pinned_channels.contains(id);
            let chat = id.clone();
            menu = menu.child(item(if pinned { "Unpin channel" } else { "Pin channel" }, "channel-pin", move |s: &mut UiState<A>| s.actions.set_channel_pinned(&chat, !pinned)));
            let (chat, shown) = (id.clone(), row.hidden);
            menu = menu.child(item(if shown { "Show" } else { "Hide" }, "channel-show", move |s: &mut UiState<A>| s.actions.set_channel_shown(&chat, shown)));
            let chat = id.clone();
            menu = menu.child(item("Mark all as read", "channel-read", move |s: &mut UiState<A>| s.actions.set_chat_read(&chat, true)));
            let current = s.chatlist.channel_details.get(id).map(|details| details.notifications).unwrap_or_default();
            menu = menu.child(text("Channel notifications").font(ts(11.0, 600.0)).color(tone(p.text_muted)).padding(8.0));
            for (key, label, level) in [
                ("banner", "Banner and feed", store::ChannelNotificationLevel::BannerAndFeed),
                ("feed", "Only show in feed", store::ChannelNotificationLevel::Feed),
                ("off", "Off", store::ChannelNotificationLevel::Off),
            ] {
                let chat = id.clone();
                let label = if current.level == level { format!("✓ {label}") } else { label.into() };
                menu = menu.child(item(&label, &format!("channel-notify-{key}"), move |s: &mut UiState<A>| s.actions.set_channel_notifications(&chat, store::ChannelNotifications { level, ..current })));
            }
            let chat = id.clone();
            menu = menu.child(item(if current.include_replies { "✓ Include replies" } else { "Include replies" }, "channel-notify-replies", move |s: &mut UiState<A>| {
                s.actions.set_channel_notifications(&chat, store::ChannelNotifications { include_replies: !current.include_replies, ..current });
            }));
        } else {
            let team = s.teams.iter().find(|team| &team.id == id)?;
            let shown = s.chatlist.team_details.get(id).is_some_and(|details| details.hidden);
            let team_id = id.clone();
            menu = menu.child(item(if shown { "Show" } else { "Hide" }, "team-show", move |s: &mut UiState<A>| s.actions.set_team_shown(&team_id, shown)));
            let channels: Vec<_> = team.channels.iter().map(|channel| channel.id.clone()).collect();
            menu = menu.child(item("Mark all as read", "team-read", move |s: &mut UiState<A>| {
                for channel in &channels { s.actions.set_chat_read(channel, true); }
            }));
            if s.chatlist.team_details.get(id).is_some_and(|details| details.rights.owner || details.rights.add_channel) {
                let team_id = id.clone();
                menu = menu.child(item("Add channel", "team-create-channel", move |s: &mut UiState<A>| {
                    s.chatlist.team_menu.create_team = Some(team_id.clone());
                    s.chatlist.team_menu.name.clear();
                    s.chatlist.team_menu.description.clear();
                    s.chatlist.team_menu.private = false;
                }));
            }
        }
    }
    Some(el().key("team-menu-layer").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .child(el().key("team-menu-backdrop").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).on_press(|s: &mut UiState<A>| s.chatlist.team_menu.menu = None))
        .child(menu.pin(Some(*x), Some(y.min(350.0)), None, None)))
}

fn create_dialog<A: Actions + 'static>(s: &UiState<A>, team: &str) -> El {
    let p = active_palette();
    let state = &s.chatlist.team_menu;
    let target = team.to_owned();
    let mut dialog = col().key("create-channel-dialog").w(420.0).padding(20.0).gap(12.0).radius(10.0)
        .role("dialog").label("Create channel").fill(tone(p.popup))
        .modal(true, |s: &mut UiState<A>| s.chatlist.team_menu.create_team = None)
        .child(text("Create channel").font(ts(18.0, 600.0)).color(tone(p.foreground)))
        .child(text_input(&state.name, ts(14.0, 400.0)).key("channel-name").w_full().label("Channel name")
            .placeholder("Channel name", tone(p.text_muted)).color(tone(p.foreground))
            .on_edit(|s: &mut UiState<A>| &mut s.chatlist.team_menu.name))
        .child(text_input(&state.description, ts(14.0, 400.0)).key("channel-description").w_full().label("Description")
            .placeholder("Description", tone(p.text_muted)).color(tone(p.foreground))
            .on_edit(|s: &mut UiState<A>| &mut s.chatlist.team_menu.description));
    if s.chatlist.team_details.get(team).is_some_and(|details| details.rights.owner || details.rights.add_private_channel) {
        dialog = dialog.child(item(if state.private { "✓ Private channel" } else { "Private channel" }, "channel-private", |s: &mut UiState<A>| s.chatlist.team_menu.private = !s.chatlist.team_menu.private));
    }
    dialog = dialog.child(row().gap(8.0).justify_end()
        .child(crate::screens::button("Cancel", "channel-create-cancel", |s: &mut UiState<A>, _| s.chatlist.team_menu.create_team = None))
        .child(crate::screens::button("Create", "channel-create-confirm", move |s: &mut UiState<A>, _| {
            let draft = &s.chatlist.team_menu;
            if draft.name.text.trim().is_empty() || draft.name.text.trim().chars().count() > teams_core::MAX_CHANNEL_NAME_LENGTH
                || draft.description.text.chars().count() > teams_core::MAX_CHANNEL_DESCRIPTION_LENGTH {
                s.toast.show("Enter a channel name of 1–50 characters and a description of at most 1024 characters.");
                return;
            }
            s.actions.create_channel(&target, &draft.name.text, &draft.description.text, draft.private);
            s.chatlist.team_menu.create_team = None;
        })));
    el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().fill(Color::BLACK.alpha(0.2)).child(dialog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake(std::cell::RefCell<Vec<String>>);
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn set_channel_pinned(&self, id: &str, pinned: bool) { self.0.borrow_mut().push(format!("pin {id} {pinned}")); }
    }

    #[test]
    fn channel_menu_pins_the_channel() {
        let mut s = UiState::new(Fake::default());
        s.screens.screen = crate::screens::Screen::Teams;
        s.teams.push(crate::types::TeamRow { id: "team".into(), name: "Team".into(), channels: vec![crate::types::ChannelRow { id: "channel".into(), name: "General".into(), hidden: false, unread: false }] });
        open(&mut s, "channel", true, 200.0, 100.0);
        let mut rt = Runtime::new(s, crate::ui::view::<Fake>, (1280.0, 800.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(1.0);
        let (x, y) = rt.find("channel-pin").expect("pin menu item");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(*rt.state.actions.0.borrow(), ["pin channel true"]);
        assert!(rt.state.chatlist.team_menu.menu.is_none());
    }
}
