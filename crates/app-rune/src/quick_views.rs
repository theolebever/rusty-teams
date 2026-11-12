//! Quick views share the sidebar's conversations and the account's saved-message list.

use app_backend::schemes::active_palette;
use rune::*;

use crate::types::Actions;
use crate::ui::{self, text, ts, UiState};
use crate::view_chatlist::tone;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum QuickView {
    Mentions,
    Drafts,
    Saved,
}

pub(crate) fn is_mention(kind: &str) -> bool {
    matches!(kind, "mention" | "mentionInChat" | "mentionInCommunity" | "mentionInSubCommunity" | "communityMention")
}

fn entry<A: Actions + 'static>(key: String, title: String, preview: String, conversation: String, root: Option<String>) -> El {
    let p = active_palette();
    col().key(key).w_full().padding(12.0).gap(4.0).radius(6.0).hover(tone(p.row_hover))
        .label(title.clone())
        .on_press_tx(move |s: &mut UiState<A>, tx| {
            let channel = s.teams.iter().flat_map(|team| &team.channels).any(|channel| channel.id == conversation);
            crate::screens::navigate(s, if channel { crate::screens::Screen::Teams } else { crate::screens::Screen::Chat });
            ui::open_chat(s, &conversation, tx);
            if channel && root.is_some() { ui::select_thread(s, root.clone()); }
        })
        .child(text(title).font(ts(14.0, 600.0)).color(tone(p.foreground)).truncate())
        .child(text(preview).font(ts(13.0, 400.0)).color(tone(p.text_muted)).truncate())
}

pub fn view<A: Actions + 'static>(s: &UiState<A>, view: QuickView) -> El {
    let p = active_palette();
    let title = match view { QuickView::Mentions => "Mentions", QuickView::Drafts => "Drafts", QuickView::Saved => "Saved" };
    let mut list = col().key("quick-view-items").w_full().gap(4.0);
    let mut count = 0;
    match view {
        QuickView::Mentions => {
            for item in s.screens.activity.items.iter().filter(|item| is_mention(&item.activity_type) && !item.source_thread_id.is_empty()) {
                count += 1;
                list = list.child(entry::<A>(format!("quick-mention-{}", item.message_id), item.source_user_name.clone(),
                    teams_core::preview_text(&item.preview).unwrap_or_default(), item.source_thread_id.clone(), item.source_reply_chain_id.clone()));
            }
        }
        QuickView::Drafts => {
            for (id, title) in s.chats.iter().map(|chat| (&chat.id, &chat.title))
                .chain(s.teams.iter().flat_map(|team| &team.channels).map(|channel| (&channel.id, &channel.name))) {
                if let Some(draft) = s.chat_draft(id) {
                    count += 1;
                    list = list.child(entry::<A>(format!("quick-draft-{id}"), title.clone(), draft, id.clone(), None));
                }
            }
        }
        QuickView::Saved => match &s.chatlist.saved {
            None => { count += 1; list = list.child(text("Loading saved messages…").font(ts(13.0, 400.0)).color(tone(p.text_muted))); }
            Some(Err(error)) => {
                count += 1;
                list = list.child(text(error.clone()).font(ts(13.0, 400.0)).color(tone(p.red_soft)).wrap())
                    .child(crate::screens::button("Retry", "saved-retry", |s: &mut UiState<A>, _| { s.chatlist.saved = None; s.actions.load_saved(); }));
            }
            Some(Ok(items)) => for (i, item) in items.iter().enumerate() {
                count += 1;
                let saved = item.clone();
                let mut line = row().w_full().center_y().child(entry::<A>(format!("quick-saved-{i}"),
                    item.topic.clone().or_else(|| item.author_name.clone()).unwrap_or_else(|| "Saved message".into()),
                    teams_core::preview_text(&item.preview).unwrap_or_default(), item.conversation_id.clone(), Some(item.root_id.clone())).grow(1.0).min_w(0.0));
                if !s.read_only {
                    line = line.child(crate::screens::button("Remove from saved", &format!("saved-remove-{i}"), move |s: &mut UiState<A>, _| s.actions.unsave(&saved)));
                }
                list = list.child(line);
            },
        },
    }
    if count == 0 {
        list = list.child(text(format!("No {}", title.to_lowercase())).font(ts(13.0, 400.0)).color(tone(p.text_muted)));
    }
    col().key("quick-view").grow(1.0).min_w(0.0).h_full().min_h(0.0).padding(20.0).gap(16.0)
        .child(text(title).font(ts(20.0, 600.0)).color(tone(p.foreground)))
        .child(col().grow(1.0).min_h(0.0).w_full().scroll(ScrollSpec { stick_bottom: false }).child(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake(std::cell::RefCell<Vec<String>>);
    impl Actions for Fake {
        fn open(&self, id: &str) { self.0.borrow_mut().push(id.into()); }
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
    }

    #[test]
    fn drafts_open_the_original_conversation() {
        let mut s = UiState::new(Fake::default());
        s.chats.push(crate::types::ChatRow { id: "chat".into(), title: "Chat".into(), preview: String::new(), unread: false, when: String::new() });
        s.selected = Some("chat".into());
        s.composer = TextEdit::new("Unsent text");
        s.chatlist.quick_view = Some(QuickView::Drafts);
        let mut rt = Runtime::new(s, crate::ui::view::<Fake>, (1280.0, 800.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(1.0);
        let (x, y) = rt.find("quick-draft-chat").expect("draft row");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert!(rt.state.chatlist.quick_view.is_none());
        assert_eq!(rt.state.composer.text, "Unsent text");
        assert_eq!(*rt.state.actions.0.borrow(), ["chat"]);
    }

    #[test]
    fn all_mention_kinds_are_recognized() {
        assert!(is_mention("mentionInChat"));
        assert!(is_mention("communityMention"));
        assert!(!is_mention("reply"));
    }
}
