//! Members panel opened from the header's participant count: who is in the chat, add people,
//! remove people (group chats). GPUI: `views/chat_members.rs`, `add_people_dialog.rs`.

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

use crate::icons::sym;
use crate::types::Actions;
use crate::ui::{ts, UiState};
use crate::view_chatlist::tone;

const ROW_H: f32 = 44.0;

pub fn toggle<A: Actions>(s: &mut UiState<A>) {
    let manage = &mut s.chatlist.manage;
    manage.members_open = !manage.members_open;
    manage.adding = false;
    manage.add = Default::default();
}

/// A chat with more than two people can lose members; a 1:1 cannot.
fn is_group(members: usize) -> bool {
    members > 2
}

pub fn panel<A: Actions + 'static>(s: &UiState<A>, id: &str) -> El {
    let p = active_palette();
    let members: Vec<&store::MemberRecord> = s.details.get(id).map(|d| d.members.iter().filter(|m| !m.is_bot()).collect()).unwrap_or_default();
    let group = is_group(members.len()) && !s.read_only;
    let mut panel = col()
        .key("members")
        .w(320.0)
        .padding(12.0)
        .gap(8.0)
        .radius(10.0)
        .fill(tone(p.surface))
        .border(1.0, tone(p.border_strong))
        .child(
            row()
                .w_full()
                .center_y()
                .child(text(format!("Members ({})", members.len())).font(ts(14.0, 600.0)).color(tone(p.foreground)).grow(1.0))
                .children((!s.read_only).then(|| {
                    row()
                        .key("members-add")
                        .h(28.0)
                        .px(8.0)
                        .gap(4.0)
                        .center_y()
                        .radius(6.0)
                        .hover(tone(p.popup_hover))
                        .on_press(|s: &mut UiState<A>| s.chatlist.manage.adding = !s.chatlist.manage.adding)
                        .child(sym("person_add", 16.0).color(tone(p.accent_text)))
                        .child(text("Add people").font(ts(13.0, 400.0)).color(tone(p.accent_text)))
                })),
        );
    if s.chatlist.manage.adding {
        let skip: Vec<String> = members.iter().filter_map(|m| m.user_id.clone()).collect();
        let draft = &s.chatlist.manage.add;
        panel = panel.child(crate::new_chat::picker(s, draft, "members-new", |s| &mut s.chatlist.manage.add, &skip));
        if !draft.chips.is_empty() {
            let id = id.to_owned();
            panel = panel.child(crate::screens::button("Add", "members-add-confirm", move |s: &mut UiState<A>, _| {
                let people = s.chatlist.manage.add.chips.iter()
                    .map(|chip| store::MemberRecord { user_id: Some(chip.user_id.clone()), display_name: chip.name.clone() })
                    .collect();
                s.actions.add_members(&id, people);
                s.chatlist.manage.adding = false;
                s.chatlist.manage.add = Default::default();
            }));
        }
    }
    let mut list = col().w_full().gap(2.0).max_h(360.0).scroll(ScrollSpec { stick_bottom: false });
    for (index, member) in members.iter().enumerate() {
        let user = member.user_id.clone();
        let chat = id.to_owned();
        let mut line = row()
            .key(format!("member-{index}"))
            .w_full()
            .h(ROW_H)
            .px(6.0)
            .gap(8.0)
            .center_y()
            .radius(6.0)
            .hover(tone(p.row_hover))
            .child(crate::avatar::person(s, &member.display_name, member.user_id.as_deref(), 28.0, None))
            .child(text(member.display_name.clone()).font(ts(13.0, 400.0)).color(tone(p.foreground)).truncate().grow(1.0).min_w(0.0));
        if let (true, Some(user)) = (group, user) {
            line = line.child(
                row()
                    .key(format!("member-remove-{index}"))
                    .size(24.0, 24.0)
                    .center()
                    .radius(6.0)
                    .hover(tone(p.popup_hover))
                    .on_press(move |s: &mut UiState<A>| s.actions.remove_member(&chat, &user))
                    .child(sym("close", 14.0).color(tone(p.text_muted))),
            );
        }
        list = list.child(line);
    }
    panel.child(list)
}
