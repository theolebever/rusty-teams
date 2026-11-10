//! The message list of the open conversation, drawn as the GPUI app draws it
//! (`crates/app/src/views/message_row.rs`): other people's posts with a 28 px sender avatar and
//! a quiet bubble, own posts right-aligned in the accent tint with the time and read check under
//! them, consecutive posts from one sender grouped under one avatar and name, emoji-only posts
//! drawn big without a bubble, and react / reply / more on a toolbar that shows on hover.

use std::borrow::Cow;

use app_backend::schemes::active_palette;
use rune::*;

use crate::avatar::{avatar, rgb};
use crate::icons::sym;
use crate::types::{Actions, MessageRow};
use crate::ui::{UiState, active_target, select_thread, ts};

pub(crate) const MESSAGE_ROW_H: f32 = 64.0;
const AVATAR: f32 = 28.0;
const AVATAR_GAP: f32 = 8.0;
const START_GAP: f32 = 8.0;
const SERIES_GAP: f32 = 2.0;
const REPLY_INDENT: f32 = 32.0;
const BUBBLE_MAX_W: f32 = 560.0;
const CORNER: f32 = 14.0;
const CORNER_JOINED: f32 = 4.0;
const BODY: f32 = 13.5;
const JUMBO: f32 = 40.0;
const META: f32 = 11.0;
const CHECK: f32 = 15.0;
const ACTION: f32 = 30.0;
const ACTION_ICON: f32 = 16.0;

/// Whether row `i` continues the run of the same sender above it, and whether the next row does.
pub(crate) fn series(rows: &[MessageRow], i: usize) -> (bool, bool) {
    let same = |a: &MessageRow, b: &MessageRow| a.sender == b.sender && a.mine == b.mine;
    let has_prev = i > 0 && same(&rows[i - 1], &rows[i]);
    let has_next = rows.get(i + 1).is_some_and(|next| same(&rows[i], next));
    (has_prev, has_next)
}

/// One to three emoji and nothing else.
// ponytail: a skin tone or family sequence counts as several glyphs, so it falls back to a bubble past the cap.
pub(crate) fn emoji_only(text: &str) -> bool {
    let mut glyphs = 0;
    for ch in text.chars() {
        match ch as u32 {
            _ if ch.is_whitespace() => {}
            0x200D | 0xFE0F | 0x20E3 => {}
            0x2300..=0x23FF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0x1F000..=0x1FAFF => glyphs += 1,
            _ => return false,
        }
    }
    (1..=3).contains(&glyphs)
}

/// Corner radii `[top left, top right, bottom right, bottom left]`: the corners on the sender's
/// side of a run stay tight, so a run reads as one block.
fn corner_radii(own: bool, has_prev: bool, has_next: bool) -> [f32; 4] {
    let joined = |joined: bool| if joined { CORNER_JOINED } else { CORNER };
    if own {
        [CORNER, joined(has_prev), joined(has_next), CORNER]
    } else {
        [joined(has_prev), CORNER, CORNER, joined(has_next)]
    }
}

pub(crate) fn message_list<A: Actions + 'static>(s: &UiState<A>) -> El {
    // The version is the id sequence: a reload with the same count but other messages must re-key.
    let displayed = s.search_results.as_ref().unwrap_or(&s.messages);
    let ids: Vec<&str> = displayed.iter().map(|m| m.id.as_str()).collect();
    let rows = col()
        .key("message-rows")
        .w_full()
        .px(20.0)
        .pt(12.0)
        .growth(GrowthSpec::default())
        .instant_changes()
        .virtual_rows(
            displayed.len(),
            MESSAGE_ROW_H,
            |s: &UiState<A>, i| Cow::Owned(format!("msg-{}", s.search_results.as_ref().unwrap_or(&s.messages)[i].id)),
            |s: &UiState<A>, i| message_row(s, i),
        )
        .virtual_rows_version((ids, crate::theme::version()));
    let id = s.selected.clone().unwrap_or_default();
    col()
        .key("messages")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .scroll(ScrollSpec { stick_bottom: true })
        .child(row().key("load-older").w_full().h(36.0).center().hover(rgb(active_palette().row_hover))
            .on_press_tx(move |s: &mut UiState<A>, _| s.actions.load_older(&id))
            .child(text("Load older messages").font(ts(13.0, 600.0)).color(rgb(active_palette().foreground))))
        // Rune: a virtual/growth stack must be the last child of its scroll viewport.
        .child(rows)
}

pub(crate) fn message_row<A: Actions + 'static>(s: &UiState<A>, i: usize) -> El {
    let rows = s.search_results.as_ref().unwrap_or(&s.messages);
    let m = &rows[i];
    let (has_prev, has_next) = series(rows, i);
    let own = m.mine;
    let palette = active_palette();
    let jumbo = !m.deleted && emoji_only(&m.text);
    let body = bubble(s, m, own, has_prev, has_next, jumbo);
    let bubble_row = if own {
        row().gap(4.0).center_y().children(toolbar(s, m)).child(body)
    } else {
        row().gap(4.0).center_y().child(body).children(toolbar(s, m))
    };
    let mut stack = if own { col().gap(2.0).items_end() } else { col().gap(2.0).items_start() };
    if !own && !has_prev {
        stack = stack.child(row().key(format!("head-{}", m.id)).gap(8.0).center_y()
            .child(text(m.sender.clone()).font(ts(12.0, 600.0)).color(rgb(palette.text_soft)))
            .child(text(m.when.clone()).font(ts(META, 400.0)).color(rgb(palette.text_muted))));
    }
    stack = stack.child(bubble_row);
    if !m.deleted {
        stack = stack.child(crate::reactions::controls(s, m));
    }
    stack = stack.children(delete_row(s, m));
    let content = if own {
        let meta = row().gap(4.0).center_y()
            .child(text(m.when.clone()).font(ts(META, 400.0)).color(rgb(palette.own_meta)))
            .children((!m.deleted).then(|| sym("done_all", CHECK).color(rgb(palette.own_read))));
        row().w_full().justify_end().child(stack.child(meta))
    } else {
        // Only the first post of a run carries the avatar; the rest keep the spacer so text stays aligned.
        let lead = if has_prev { el().size(AVATAR, AVATAR) } else { avatar(&m.sender, AVATAR, None) };
        row().w_full().gap(AVATAR_GAP).items_start().child(lead).child(stack)
    };
    let indent = if m.reply_to_id.is_some() { REPLY_INDENT } else { 0.0 };
    let gap = if has_prev { SERIES_GAP } else { START_GAP };
    col().w_full().pt(gap).pl(indent).hover_group().child(content)
}

fn bubble<A: Actions + 'static>(s: &UiState<A>, m: &MessageRow, own: bool, has_prev: bool, has_next: bool, jumbo: bool) -> El {
    let palette = active_palette();
    if jumbo {
        return text(m.text.trim().to_owned()).key(format!("jumbo-{}", m.id)).font(ts(JUMBO, 400.0)).color(rgb(palette.foreground));
    }
    let mut body = col()
        .key(format!("bubble-{}", m.id))
        .max_w(BUBBLE_MAX_W)
        .corners(corner_radii(own, has_prev, has_next))
        .px(12.0)
        .py(8.0)
        .gap(6.0);
    let ink = if m.deleted {
        body = body.border(1.0, rgb(palette.border_strong));
        palette.text_muted
    } else {
        body = body.fill(rgb(if own { palette.bubble_own } else { palette.bubble_other }));
        palette.foreground
    };
    body.child(text(m.text.clone()).font(ts(BODY, 400.0)).color(rgb(ink)).wrap())
        .child(crate::media::view(s, &m.media, &m.id))
}

/// One icon button of the hover toolbar. Its press goes to `on_press`.
pub(crate) fn action_button<A: Actions + 'static>(key: String, glyph: &str, on_press: impl Fn(&mut UiState<A>, &mut Tx) + 'static) -> El {
    let palette = active_palette();
    row()
        .key(key)
        .w(ACTION)
        .h(ACTION)
        .radius(7.0)
        .center()
        .hover(rgb(palette.popup_hover))
        .on_press_tx(on_press)
        .child(sym(glyph, ACTION_ICON).color(rgb(palette.text_soft)))
}

/// React, reply and more, shown while the pointer is over the message. Read-only views have none.
fn toolbar<A: Actions + 'static>(s: &UiState<A>, m: &MessageRow) -> Option<El> {
    if s.read_only || m.deleted {
        return None;
    }
    let mut bar = row()
        .key(format!("toolbar-{}", m.id))
        .gap(1.0)
        .px(3.0)
        .h(ACTION + 4.0)
        .center_y()
        .radius(10.0)
        .fill(rgb(active_palette().popup))
        .group_opacity(0.0, 1.0)
        .child(crate::reactions::trigger::<A>(m));
    if m.reply_to_id.is_none() && active_target(s).is_some_and(|target| target.channel) {
        let root = m.id.clone();
        bar = bar.child(action_button(format!("reply-{}", m.id), "reply", move |s: &mut UiState<A>, tx: &mut Tx| {
            select_thread(s, Some(root.clone()));
            tx.focus("composer", false);
        }));
    }
    if m.can_delete {
        let id = m.id.clone();
        bar = bar.child(action_button(format!("more-{}", m.id), "more_horiz", move |s: &mut UiState<A>, _| {
            s.more_open = (s.more_open.as_deref() != Some(id.as_str())).then(|| id.clone());
        }));
    }
    Some(bar)
}

/// Under the message while its "more" is open: delete, then confirm the delete.
fn delete_row<A: Actions + 'static>(s: &UiState<A>, m: &MessageRow) -> Option<El> {
    if s.read_only || m.deleted || !m.can_delete || s.more_open.as_deref() != Some(m.id.as_str()) {
        return None;
    }
    let id = m.id.clone();
    let conversation_id = s.selected.clone().unwrap_or_default();
    let confirming = s.confirm_delete.as_deref() == Some(&id);
    Some(row().key(format!("delete-{id}")).h(28.0).px(8.0).radius(6.0).hover(rgb(active_palette().popup_hover))
        .on_press_tx(move |s: &mut UiState<A>, _| {
            if confirming {
                s.actions.delete_message(&conversation_id, &id);
                s.confirm_delete = None;
                s.more_open = None;
            } else {
                s.confirm_delete = Some(id.clone());
            }
        })
        .child(text(if confirming { "Confirm delete" } else { "Delete" }).font(ts(12.0, 400.0)).color(rgb(active_palette().red))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SendTarget, Update};
    use rune::{Env, Runtime};

    struct Fake;

    impl Actions for Fake {
        fn open(&self, _conversation_id: &str) {}
        fn send(&self, _target: SendTarget, _text: String, _files: Vec<teams_core::FileReference>) {}
    }

    fn msg(id: &str, sender: &str, mine: bool, text: &str) -> MessageRow {
        MessageRow {
            id: id.into(),
            reply_to_id: None,
            deleted: false,
            can_delete: false,
            reactions: vec![],
            sender: sender.into(),
            when: "14:06".into(),
            text: text.into(),
            mine,
            media: Default::default(),
        }
    }

    fn show(rows: Vec<MessageRow>) -> Runtime<UiState<Fake>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Fake), crate::ui::view::<Fake>, (1000.0, 700.0), env);
        rt.frame(0.0);
        rt.update(move |s, _| {
            s.selected = Some("a".into());
            s.apply(Update::Messages { conversation_id: "a".into(), rows });
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

    #[test]
    fn series_marks_runs_of_one_sender() {
        let rows = vec![msg("1", "Ada", false, ""), msg("2", "Ada", false, ""), msg("3", "Bo", false, "")];
        assert_eq!(series(&rows, 0), (false, true));
        assert_eq!(series(&rows, 1), (true, false));
        assert_eq!(series(&rows, 2), (false, false));
    }

    #[test]
    fn emoji_only_counts_up_to_three_glyphs() {
        assert!(emoji_only("\u{1F44D}"));
        assert!(emoji_only(" \u{2764}\u{FE0F} \u{1F389}\u{1F44D} "));
        assert!(!emoji_only("\u{1F44D}\u{1F44D}\u{1F44D}\u{1F44D}"));
        assert!(!emoji_only("ok \u{1F44D}"));
        assert!(!emoji_only("   "));
    }

    #[test]
    fn consecutive_posts_from_one_sender_share_one_head() {
        let rt = show(vec![msg("m1", "Ada", false, "hi"), msg("m2", "Ada", false, "there")]);
        assert!(rt.find("head-m1").is_some());
        assert!(rt.find("head-m2").is_none());
    }

    #[test]
    fn emoji_only_post_is_drawn_without_a_bubble() {
        let rt = show(vec![msg("m1", "Ada", false, "\u{1F44D}\u{1F389}"), msg("m2", "Ada", false, "ok \u{1F44D}")]);
        assert!(rt.find("jumbo-m1").is_some());
        assert!(rt.find("bubble-m1").is_none());
        assert!(rt.find("bubble-m2").is_some());
    }

    #[test]
    fn more_reveals_delete_for_a_deletable_post() {
        let mut rt = show(vec![MessageRow { can_delete: true, ..msg("m1", "Me", true, "hi") }]);
        assert!(rt.find("delete-m1").is_none());
        click(&mut rt, "more-m1");
        assert!(rt.find("delete-m1").is_some());
    }
}
