//! The message list of the open conversation, drawn as the GPUI app draws it
//! (`crates/app/src/views/message_row.rs`): other people's posts with a 28 px sender avatar and
//! a quiet bubble, own posts right-aligned in the accent tint with the time and read check under
//! them, consecutive posts from one sender grouped under one avatar and name, emoji-only posts
//! drawn big without a bubble, and react / reply / more on a toolbar that shows on hover.

use std::borrow::Cow;

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

use crate::avatar::rgb;
use crate::icons::sym;
use crate::types::{Actions, MessageRow};
use crate::ui::{UiState, active_target, select_thread, ts};

pub(crate) const MESSAGE_ROW_H: f32 = 64.0;

pub(crate) fn shared<A: Actions + 'static>(s: &UiState<A>) -> El {
    // shortcut: uses the loaded history, add server pagination when browsing all shared files.
    let mut list = col().w_full().padding(20.0).gap(12.0)
        .child(text("Files and images in loaded messages").font(ts(14.0, 600.0)).color(rgb(active_palette().foreground)));
    for message in s.messages.iter().filter(|m| !m.deleted && (!m.media.images.is_empty() || !m.media.files.is_empty())) {
        list = list.child(crate::media::view(s, &message.media, &message.id));
    }
    col().key("shared").grow(1.0).min_h(0.0).w_full().scroll(ScrollSpec { stick_bottom: false }).child(list)
}
const AVATAR: f32 = 28.0;
const AVATAR_GAP: f32 = 8.0;
const START_GAP: f32 = 8.0;
const SERIES_GAP: f32 = 2.0;
const REPLY_INDENT: f32 = 32.0;
const BUBBLE_MAX_W: f32 = 560.0;
const CORNER: f32 = 8.0;
const CORNER_JOINED: f32 = 4.0;
const BODY: f32 = 14.0;
const JUMBO: f32 = 40.0;
const META: f32 = 11.0;
const CHECK: f32 = 15.0;
const ACTION: f32 = 30.0;
const ACTION_ICON: f32 = 16.0;

/// Whether row `i` continues the run of the same sender above it, and whether the next row does.
pub(crate) fn series(rows: &[MessageRow], i: usize) -> (bool, bool) {
    let same = |a: &MessageRow, b: &MessageRow| !a.system && !b.system && a.sender == b.sender && a.mine == b.mine;
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

/// `el` with its changes landing at once, unless messages arrive live in this commit: then the
/// new rows slide in on their `transition` (an instant parent mounts its children at once).
// ponytail: other mounts in that one commit fade too; give the arrival its own commit if that shows.
pub(crate) fn snappy(el: El, arriving: bool) -> El {
    if arriving { el } else { el.instant_changes() }
}

/// How a message that arrives live comes in: own ones rise from the right, others slide from the left.
fn arrival(own: bool) -> Transition {
    let (x, y) = if own { (8.0, 16.0) } else { (-18.0, 4.0) };
    Transition::offset(x, y).combined(Transition::opacity())
}

pub(crate) fn message_list<A: Actions + 'static>(s: &UiState<A>, arriving: bool) -> El {
    // The version is the id sequence: a reload with the same count but other messages must re-key.
    let displayed = s.search_results.as_ref().unwrap_or(&s.messages);
    if active_target(s).is_some_and(|target| target.channel) && !displayed.is_empty() {
        let mut posts = col().key("channel-posts").w_full().gap(16.0).padding(24.0);
        for index in crate::teams::post_indices(displayed) {
            posts = posts.child(col().key(format!("msg-{}", displayed[index].id)).w_full().child(channel_post(s, &displayed[index])));
        }
        let id = s.selected.clone().unwrap_or_default();
        return col().key("messages").grow(1.0).min_h(0.0).w_full().scroll(ScrollSpec { stick_bottom: true })
            .child(crate::screens::button("Load older posts", "load-older", move |s: &mut UiState<A>, _| s.actions.load_older(&id)))
            .child(row().w_full().justify_center().child(posts.max_w(920.0).grow(1.0)));
    }
    if displayed.is_empty() && active_target(s).is_some_and(|target| target.channel) {
        let id = s.selected.clone().unwrap_or_default();
        return col().key("messages").grow(1.0).min_h(0.0).w_full().center().gap(12.0)
            .child(sym("chat_bubble", 36.0).color(rgb(active_palette().text_muted)))
            .child(text("No posts to show").font(ts(18.0, 600.0)).color(rgb(active_palette().foreground)))
            .child(crate::screens::button("Load older posts", "load-older", move |s: &mut UiState<A>, _| s.actions.load_older(&id)));
    }
    let ids: Vec<&str> = displayed.iter().map(|m| m.id.as_str()).collect();
    let rows = col()
        .key("message-rows")
        .w_full()
        .px(20.0)
        .pt(12.0)
        .growth(GrowthSpec::default());
    let rows = snappy(rows, arriving)
        .virtual_rows(
            displayed.len(),
            MESSAGE_ROW_H,
            |s: &UiState<A>, i| Cow::Owned(format!("msg-{}", s.search_results.as_ref().unwrap_or(&s.messages)[i].id)),
            |s: &UiState<A>, i| message_row(s, i),
        )
        .on_visible_rows(|s: &mut UiState<A>, rows| crate::find::rows_visible(s, rows))
        .virtual_rows_version((ids, crate::theme::version(), crate::find::marked(s)));
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
    if m.system {
        return col().w_full().child(row().key(format!("event-{}", m.id)).w_full().py(12.0).gap(8.0).center()
            .child(sym("videocam", 18.0).color(rgb(active_palette().text_muted)))
            .child(text(m.text.clone()).font(ts(12.0, 400.0)).color(rgb(active_palette().text_muted)).wrap())
            .child(text(m.when.clone()).font(ts(META, 400.0)).color(rgb(active_palette().text_muted))));
    }
    if active_target(s).is_some_and(|target| target.channel) { return channel_post(s, m); }
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
            .child(crate::profile::link::<A>(text(m.sender.clone()).font(ts(12.0, 600.0)).color(rgb(palette.text_soft)), format!("name-{}", m.id), m.sender_id.as_deref(), &m.sender))
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
        let lead = if has_prev { el().size(AVATAR, AVATAR) } else { crate::profile::link::<A>(crate::avatar::person(s, &m.sender, m.sender_id.as_deref(), AVATAR, None), format!("avatar-{}", m.id), m.sender_id.as_deref(), &m.sender) };
        row().w_full().gap(AVATAR_GAP).items_start().child(lead).child(stack)
    };
    let indent = if m.reply_to_id.is_some() { REPLY_INDENT } else { 0.0 };
    let gap = if has_prev { SERIES_GAP } else { START_GAP };
    let mut row = col().w_full().pt(gap).pl(indent).hover_group().child(content);
    if s.appearing.contains(&m.id) { row = row.transition(arrival(own)); }
    crate::find::mark(s, &m.id, row)
}

fn channel_post<A: Actions + 'static>(s: &UiState<A>, m: &MessageRow) -> El {
    let palette = active_palette();
    let root = m.reply_to_id.as_deref().unwrap_or(&m.id);
    let rows = s.search_results.as_ref().unwrap_or(&s.messages);
    let replies: Vec<_> = rows.iter().filter(|row| row.reply_to_id.as_deref() == Some(root) && row.id != m.id).collect();
    let expanded = s.expanded_posts.contains(root);
    let count = replies.len();
    let mut discussion = col().w_full().gap(8.0);
    for reply in replies.into_iter().skip(if expanded { 0 } else { count.saturating_sub(2) }) {
        discussion = discussion.child(compact_reply(s, reply, expanded));
    }
    let id = root.to_owned();
    let label = if expanded { "Hide replies".to_owned() } else if count > 2 { format!("View all {count} replies") }
        else if count == 1 { "1 reply".into() } else if count > 0 { format!("{count} replies") } else { "Reply".into() };
    let footer = row().key(format!("post-replies-{root}")).w_full().h(32.0).gap(8.0).center_y()
        .label(label.clone()).hover(rgb(palette.row_hover)).on_press_tx(move |s: &mut UiState<A>, tx| {
            if expanded {
                s.expanded_posts.remove(&id);
                if s.thread_root_id.as_deref() == Some(&id) { select_thread(s, None); }
            } else {
                s.expanded_posts.insert(id.clone());
                select_thread(s, Some(id.clone()));
                if !s.read_only { tx.focus("composer", false); }
            }
        }).child(sym("reply", 16.0).color(rgb(palette.accent)))
        .child(text(label).font(ts(12.0, 600.0)).color(rgb(palette.accent)));
    col().w_full().child(
        col().key(format!("post-{}", m.id)).w_full().padding(16.0).gap(10.0).radius(6.0)
            .border(1.0, rgb(palette.border)).fill(rgb(palette.surface_raised)).hover_group()
            .child(row().w_full().gap(10.0).center_y()
                .child(crate::profile::link::<A>(crate::avatar::person(s, &m.sender, m.sender_id.as_deref(), 32.0, None), format!("avatar-{}", m.id), m.sender_id.as_deref(), &m.sender))
                .child(col().grow(1.0).min_w(0.0)
                    .child(crate::profile::link::<A>(text(m.sender.clone()).font(ts(14.0, 600.0)).color(rgb(palette.foreground)), format!("name-{}", m.id), m.sender_id.as_deref(), &m.sender))
                    .child(text(m.when.clone()).font(ts(12.0, 400.0)).color(rgb(palette.text_muted))))
                .children(toolbar(s, m)))
            .children((!m.text.is_empty() && s.menu.edit_id.as_deref() != Some(m.id.as_str())).then(|| crate::rich_text::body::<A>(m, 14.0, rgb(palette.foreground))))
            .children(crate::message_menu::edit_field(s, m))
            .children(crate::message_menu::edited_mark(s, m))
            .child(crate::media::view(s, &m.media, &m.id))
            .children((!m.deleted).then(|| crate::reactions::controls(s, m)))
            .children(delete_row(s, m))
            .child(col().w_full().pt(8.0).gap(8.0).child(el().w_full().h(1.0).fill(rgb(palette.border))).child(discussion).child(footer)))
}

fn compact_reply<A: Actions + 'static>(s: &UiState<A>, m: &MessageRow, expanded: bool) -> El {
    let palette = active_palette();
    let body = text(m.text.clone()).font(ts(13.0, 400.0)).color(rgb(palette.foreground));
    let mut content = col().grow(1.0).min_w(0.0).gap(2.0)
        .child(row().gap(8.0).center_y()
            .child(crate::profile::link::<A>(text(m.sender.clone()).font(ts(12.0, 600.0)).color(rgb(palette.foreground)), format!("name-{}", m.id), m.sender_id.as_deref(), &m.sender))
            .child(text(m.when.clone()).font(ts(11.0, 400.0)).color(rgb(palette.text_muted))))
        .child(if expanded {
            crate::message_menu::edit_field(s, m)
                .unwrap_or_else(|| crate::rich_text::body::<A>(m, 13.0, rgb(palette.foreground)))
        } else { body.truncate() })
        .children(crate::message_menu::edited_mark(s, m));
    if expanded {
        content = content.child(crate::media::view(s, &m.media, &m.id))
            .child(crate::reactions::controls(s, m)).children(delete_row(s, m));
    }
    row().key(format!("post-reply-{}", m.id)).w_full().gap(8.0).py(4.0).hover_group()
        .child(crate::profile::link::<A>(crate::avatar::person(s, &m.sender, m.sender_id.as_deref(), 24.0, None), format!("avatar-{}", m.id), m.sender_id.as_deref(), &m.sender))
        .child(content).children(expanded.then(|| toolbar(s, m)).flatten())
}

fn bubble<A: Actions + 'static>(s: &UiState<A>, m: &MessageRow, own: bool, has_prev: bool, has_next: bool, jumbo: bool) -> El {
    let palette = active_palette();
    if let Some(field) = crate::message_menu::edit_field(s, m) { return field; }
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
    body.child(crate::rich_text::body::<A>(m, BODY, rgb(ink)))
        .child(crate::media::view(s, &m.media, &m.id))
        .children(crate::message_menu::edited_mark(s, m))
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
    let id = m.id.clone();
    bar = bar.child(action_button(format!("more-{}", m.id), "more_horiz", move |s: &mut UiState<A>, _| {
        s.more_open = (s.more_open.as_deref() != Some(id.as_str())).then(|| id.clone());
    }));
    Some(bar)
}

/// Under the message while its "more" is open (`message_menu.rs`).
fn delete_row<A: Actions + 'static>(s: &UiState<A>, m: &MessageRow) -> Option<El> {
    crate::message_menu::menu(s, m)
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
            sender_id: None,
            system: false,
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
    fn meeting_events_have_no_author_bubble_or_actions_and_break_sender_runs() {
        let event = MessageRow { system: true, ..msg("meeting", "Unknown", false, "Meeting started") };
        let rows = vec![msg("before", "Unknown", false, "hello"), event, msg("after", "Unknown", false, "hello")];
        assert_eq!(series(&rows, 1), (false, false));
        assert_eq!(series(&rows, 2), (false, false));
        let rt = show(rows);
        assert!(rt.find("event-meeting").is_some());
        assert!(rt.find("head-meeting").is_none());
        assert!(rt.find("bubble-meeting").is_none());
        assert!(rt.find("toolbar-meeting").is_none());
    }

    #[test]
    fn channel_replies_preview_only_two_and_expand_inside_the_post() {
        let mut rows = vec![msg("post", "Ada", false, "Release")];
        for id in ["r1", "r2", "r3"] {
            rows.push(MessageRow { reply_to_id: Some("post".into()), ..msg(id, "Bo", false, "Thanks") });
        }
        let mut rt = show(rows);
        rt.update(|s, _| {
            s.teams = vec![crate::types::TeamRow { id: "team".into(), name: "Team".into(), channels: vec![crate::types::ChannelRow { id: "a".into(), name: "General".into(), hidden: false, unread: false }] }];
        });
        rt.frame(1.0 / 60.0);
        assert!(rt.find("post-post").is_some());
        assert!(rt.find("post-r3").is_none());
        assert!(rt.find("post-reply-r1").is_none());
        assert!(rt.find("post-reply-r3").is_some());
        click(&mut rt, "post-replies-post");
        assert!(rt.find("post-reply-r1").is_some());
        assert_eq!(rt.state.thread_root_id.as_deref(), Some("post"));
        click(&mut rt, "post-replies-post");
        assert!(rt.find("post-reply-r1").is_none());
        assert!(rt.state.thread_root_id.is_none());
    }

    #[test]
    fn older_messages_go_in_above_the_message_on_screen() {
        let page = |from: usize| (from..from + 40).map(|i| msg(&format!("m{i:03}"), &format!("P{}", i % 2), false, "hello")).collect::<Vec<_>>();
        let mut rt = show(page(40));
        rt.update(|_, tx| tx.scroll_to_y("messages", 0.0));
        for t in 2..5 { rt.frame(t as f64 / 60.0); }
        let before = rt.find("msg-m040").expect("first message on screen at the top");
        rt.update(|s, _| {
            let mut rows = page(0);
            rows.extend(page(40));
            s.apply(Update::Messages { conversation_id: "a".into(), rows });
        });
        for t in 5..8 { rt.frame(t as f64 / 60.0); }
        let after = rt.find("msg-m040").expect("the message read before stays on screen");
        assert!((after.1 - before.1).abs() < 1.0, "it moved from {before:?} to {after:?}");
        assert!(rt.find("msg-m000").is_none(), "the older page stays above, off screen");
    }

    fn arrive(rt: &mut Runtime<UiState<Fake>>, rows: Vec<MessageRow>) {
        // As `main::poll`: the last commit's arrivals are done before the next batch applies.
        rt.update(move |s, _| {
            s.appearing.clear();
            s.apply(Update::Messages { conversation_id: "a".into(), rows });
        });
    }

    #[test]
    fn a_message_that_arrives_live_slides_in_once_and_loads_do_not() {
        let mut rt = show(vec![msg("m1", "Ada", false, "hi")]);
        for _ in 0..30 { rt.frame(1.0 / 60.0); }
        assert_eq!(rt.presented("msg-m1").unwrap().1, 1.0, "the first load shows at once");
        let rest = rt.presented("msg-m1").unwrap().0;

        arrive(&mut rt, vec![msg("m1", "Ada", false, "hi"), msg("m2", "Bo", false, "hey")]);
        rt.frame(1.0 / 60.0);
        let (rect, opacity) = rt.presented("msg-m2").expect("new row");
        assert!(opacity < 1.0, "it fades in, opacity {opacity}");
        // A later commit (a typing event, a hover) puts the list back to instant mid-entrance.
        rt.update(|s, _| s.appearing.clear());
        for _ in 0..60 { rt.frame(1.0 / 60.0); }
        let (settled, opacity) = rt.presented("msg-m2").unwrap();
        assert_eq!(opacity, 1.0);
        assert!(settled[0] > rect[0], "it slid in from the left: {rect:?} to {settled:?}");
        assert_eq!(rt.presented("msg-m1").unwrap().0[0], rest[0], "the message already there stays put");

        let older = vec![msg("m0", "Bo", false, "earlier"), msg("m1", "Ada", false, "hi"), msg("m2", "Bo", false, "hey")];
        arrive(&mut rt, older);
        assert!(rt.state.appearing.is_empty(), "an older page is no arrival");
        rt.update(|s, tx| crate::ui::open_chat(s, "a", tx));
        arrive(&mut rt, vec![msg("m1", "Ada", false, "hi"), msg("m3", "Bo", false, "new")]);
        assert!(rt.state.appearing.is_empty(), "reopening a chat is no arrival");
    }

    #[test]
    fn series_marks_runs_of_one_sender() {
        let rows = vec![msg("1", "Ada", false, ""), msg("2", "Ada", false, ""), msg("3", "Bo", false, "")];
        assert_eq!(series(&rows, 0), (false, true));
        assert_eq!(series(&rows, 1), (true, false));
        assert_eq!(series(&rows, 2), (false, false));
    }

    #[test]
    fn expanded_reply_renders_its_edit_field() {
        let mut s = UiState::new(Fake);
        s.menu.edit_id = Some("reply".into());
        let reply = MessageRow { reply_to_id: Some("post".into()), ..msg("reply", "Me", true, "Thanks") };
        s.messages.push(reply);
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(s, |s, _| compact_reply(s, &s.messages[0], true), (1000.0, 700.0), env);
        rt.frame(0.0);
        assert!(rt.find("edit-field-reply").is_some());
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
