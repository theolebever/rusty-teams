//! The Rune view of the Teams client: chat list on the left, the open conversation on the
//! right, the composer under it. Bridge updates come in through `UiState::apply`; user intent
//! goes out through the `Actions` the state owns.

use crate::types::{Actions, ChatRow, MessageRow, SendTarget, TeamRow, Update};
use rune::tok::Family;
use rune::*;
use std::borrow::Cow;
use std::collections::HashMap;

const PURPLE: &str = "#5B5FC7";
const LAVENDER: &str = "#E8EBFA";
const LIST: &str = "#F5F5F5";
const BUBBLE: &str = "#F5F5F5";
const HOVER: &str = "#EBEBEB";
const LINE: &str = "#E0E0E0";
const INK: &str = "#242424";
const MUTED: &str = "#616161";
const RED: &str = "#C4314B";
const WHITE: &str = "#FFFFFF";

const CHAT_ROW_H: f32 = 56.0;
const MESSAGE_ROW_H: f32 = 64.0;

fn c(s: &str) -> Color {
    crate::settings::color(s)
}

fn ts(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Segoe UI"))
}

/// Everything the window shows. `apply` folds bridge updates in; `view` reads the rest.
pub struct UiState<A: Actions> {
    pub actions: A,
    pub read_only: bool,
    pub screens: crate::screens::ScreenState,
    pub calls: crate::calls::CallView,
    pub media: crate::media::MediaState,
    pub appearance: app_backend::appearance::AppearanceSettings,
    pub status: String,
    pub chats: Vec<ChatRow>,
    pub selected: Option<String>,
    pub teams: Vec<TeamRow>,
    pub expanded_teams: Vec<String>,
    /// Messages of `selected`, oldest first.
    pub messages: Vec<MessageRow>,
    pub send_error: Option<String>,
    pub composer: TextEdit,
    search: TextEdit,
    search_results: Option<Vec<MessageRow>>,
    pub thread_root_id: Option<String>,
    confirm_delete: Option<String>,
    drafts: HashMap<SendTarget, String>,
    send_errors: HashMap<SendTarget, String>,
}

impl<A: Actions> UiState<A> {
    pub fn new(actions: A) -> Self {
        let read_only = actions.read_only();
        UiState {
            actions,
            read_only,
            screens: crate::screens::ScreenState::default(),
            calls: crate::calls::CallView::default(),
            media: crate::media::MediaState::default(),
            appearance: Default::default(),
            status: String::new(),
            chats: Vec::new(),
            selected: None,
            teams: Vec::new(),
            expanded_teams: Vec::new(),
            messages: Vec::new(),
            send_error: None,
            composer: TextEdit::new(""),
            search: TextEdit::new(""),
            search_results: None,
            thread_root_id: None,
            confirm_delete: None,
            drafts: HashMap::new(),
            send_errors: HashMap::new(),
        }
    }

    /// Applies one bridge update. Messages for a conversation other than `selected` are dropped.
    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Status(s) => self.status = s,
            Update::Chats(rows) => self.chats = rows,
            Update::Teams(rows) => self.teams = rows,
            Update::Appearance(settings) => self.appearance = settings,
            Update::Activity(snapshot) => self.screens.activity = snapshot,
            Update::Call(update) => {
                if matches!(&update, calling::CallUpdate::State(calling::CallState::Connecting)) { self.screens.screen = crate::screens::Screen::Calls; }
                self.calls.apply(update);
            }
            Update::Ring(event) => {
                if matches!(&event, calling::EngineEvent::Incoming(_)) { self.screens.screen = crate::screens::Screen::Calls; }
                self.calls.apply_ring(event);
            }
            Update::VideoReady => self.calls.apply_video(self.actions.take_video()),
            Update::Image { key, bytes } => self.media.loaded(key, bytes),
            Update::ImageFailed { key, error } => { self.media.errors.insert(key, error); }
            Update::Attachment { target, path, result } => {
                if let Some(orphan) = self.media.uploaded(target, path, result) { self.actions.discard_attachment(orphan); }
            }
            Update::Calendar { week, rows } => {
                if self.screens.week == week { self.screens.calendar = rows; }
            }
            Update::CalendarRefresh => self.actions.load_calendar(self.screens.week),
            Update::Search { conversation_id, query, rows } => {
                if self.selected.as_deref() == Some(&conversation_id) && self.search.text.trim() == query {
                    self.search_results = Some(rows);
                }
            }
            Update::Messages {
                conversation_id,
                rows,
            } => {
                if self.selected.as_deref() == Some(conversation_id.as_str()) {
                    for image in rows.iter().flat_map(|row| &row.media.images) {
                        if !self.media.images.contains_key(image.key()) && !self.media.errors.contains_key(image.key()) {
                            self.actions.load_image(image.clone());
                        }
                    }
                    self.messages = rows;
                }
            }
            Update::SendFailed {
                target,
                text,
                files,
                error,
            } => {
                self.media.restore_files(target.clone(), files);
                let error = format!("Send could not be confirmed: {error}");
                if active_target(self).as_ref() == Some(&target) {
                    let current = &self.composer.text;
                    self.composer = TextEdit::new(&join_draft(&text, current));
                    self.send_error = Some(error);
                } else {
                    let current = self.drafts.remove(&target).unwrap_or_default();
                    self.drafts
                        .insert(target.clone(), join_draft(&text, &current));
                    self.send_errors.insert(target, error);
                }
            }
        }
    }
}

pub(crate) fn open_chat<A: Actions>(s: &mut UiState<A>, id: &str, tx: &mut Tx) {
    save_draft(s);
    s.selected = Some(id.to_owned());
    s.thread_root_id = None;
    s.confirm_delete = None;
    s.messages.clear();
    s.search.clear();
    s.search_results = None;
    restore_draft(s);
    s.actions.open(id);
    tx.scroll_to_bottom_instant("messages");
}

fn active_target<A: Actions>(s: &UiState<A>) -> Option<SendTarget> {
    let conversation_id = s.selected.clone()?;
    let channel = s.teams.iter().flat_map(|team| &team.channels).any(|channel| channel.id == conversation_id);
    Some(SendTarget { conversation_id, thread_root_id: s.thread_root_id.clone(), channel })
}

fn save_draft<A: Actions>(s: &mut UiState<A>) {
    if let Some(target) = active_target(s) {
        s.drafts.insert(target.clone(), s.composer.text.clone());
        if let Some(error) = s.send_error.take() { s.send_errors.insert(target, error); }
    }
}

fn restore_draft<A: Actions>(s: &mut UiState<A>) {
    if let Some(target) = active_target(s) {
        s.composer = TextEdit::new(&s.drafts.remove(&target).unwrap_or_default());
        s.send_error = s.send_errors.remove(&target);
    }
}

fn select_thread<A: Actions>(s: &mut UiState<A>, root: Option<String>) {
    save_draft(s);
    s.thread_root_id = root;
    if let (Some(conversation), Some(root)) = (&s.selected, &s.thread_root_id) {
        s.actions.open_thread(conversation, root);
    }
    restore_draft(s);
}

fn join_draft(failed: &str, current: &str) -> String {
    if current.is_empty() {
        failed.to_owned()
    } else {
        format!("{failed}\n{current}")
    }
}

fn send<A: Actions>(s: &mut UiState<A>) {
    let Some(target) = active_target(s) else { return };
    let text = s.composer.text.trim().to_owned();
    let files = s.media.files(&target);
    if s.media.is_uploading(&target) || (text.is_empty() && files.is_empty()) {
        return;
    }
    s.send_error = None;
    s.media.clear(&target);
    s.actions.send(target, text, files);
    s.composer.clear();
}

pub fn view<A: Actions + 'static>(s: &UiState<A>, cx: &mut Cx<UiState<A>>) -> El {
    crate::theme::apply(cx, &s.appearance);
    let mut rail = col().key("rail").w(80.0).h_full().fill(c(LIST));
    for (label, screen) in [("Chat", crate::screens::Screen::Chat), ("Activity", crate::screens::Screen::Activity), ("Calendar", crate::screens::Screen::Calendar), ("Calls", crate::screens::Screen::Calls), ("Settings", crate::screens::Screen::Settings)] {
        rail = rail.child(row().key(format!("nav-{label}")).w_full().h(48.0).center().hover(c(HOVER))
            .on_press_tx(move |s: &mut UiState<A>, _| crate::screens::navigate(s, screen))
            .child(text(label).font(ts(12.0, 600.0)).color(c(INK))));
    }
    row()
        .key("root")
        .w_full()
        .h_full()
        .fill(c(WHITE))
        .child(rail)
        .child(if s.screens.screen == crate::screens::Screen::Chat { row().grow(1.0).h_full().child(chat_pane(s)).child(conversation(s)) } else { crate::screens::view(s) })
}

fn chat_pane<A: Actions + 'static>(s: &UiState<A>) -> El {
    let ids: Vec<&str> = s.chats.iter().map(|ch| ch.id.as_str()).collect();
    let rows = col()
        .key("chat-rows")
        .w_full()
        .px(8.0)
        .pb(8.0)
        .growth(GrowthSpec::default())
        .virtual_rows(
            s.chats.len(),
            CHAT_ROW_H + 2.0,
            |s: &UiState<A>, i| Cow::Owned(format!("chat-{}", s.chats[i].id)),
            |s: &UiState<A>, i| chat_row(s, i),
        )
        .virtual_rows_version((ids, crate::theme::version()));
    col()
        .key("chats")
        .w(320.0)
        .h_full()
        .min_h(0.0)
        .fill(c(LIST))
        .child(
            row().w_full().h(32.0).px(16.0).center_y().child(
                text(s.status.clone())
                    .font(ts(12.0, 400.0))
                    .color(c(MUTED))
                    .truncate()
                    .grow(1.0)
                    .min_w(0.0),
            ),
        )
        .child(
            col()
                .key("chat-list")
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                // Rune: a virtual/growth stack must be the last child of its scroll viewport.
                .child(team_list(s))
                .child(rows),
        )
}

fn team_list<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut list = col().key("teams").w_full().px(8.0).pb(8.0);
    for team in &s.teams {
        let expanded = s.expanded_teams.contains(&team.id);
        let id = team.id.clone();
        list = list.child(
            row()
                .key(format!("team-{}", team.id))
                .w_full()
                .h(36.0)
                .px(10.0)
                .center_y()
                .hover(c(HOVER))
                .on_press(move |s: &mut UiState<A>| {
                    if let Some(index) = s.expanded_teams.iter().position(|item| item == &id) {
                        s.expanded_teams.remove(index);
                    } else {
                        s.expanded_teams.push(id.clone());
                    }
                })
                .child(
                    text(format!(
                        "{}  {}",
                        if expanded { "▾" } else { "▸" },
                        team.name
                    ))
                    .font(ts(14.0, 600.0))
                    .color(c(INK))
                    .truncate(),
                ),
        );
        if expanded {
            for channel in &team.channels {
                let id = channel.id.clone();
                let selected = s.selected.as_deref() == Some(channel.id.as_str());
                list = list.child(
                    row()
                        .key(format!("channel-{}", channel.id))
                        .w_full()
                        .h(34.0)
                        .px(28.0)
                        .center_y()
                        .radius(8.0)
                        .fill(c(if selected { WHITE } else { LIST }))
                        .hover(c(HOVER))
                        .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| open_chat(s, &id, tx))
                        .child(
                            text(format!("# {}", channel.name))
                                .font(ts(14.0, if channel.unread { 700.0 } else { 400.0 }))
                                .color(c(INK))
                                .truncate(),
                        ),
                );
            }
        }
    }
    list
}

fn chat_row<A: Actions + 'static>(s: &UiState<A>, i: usize) -> El {
    let ch = &s.chats[i];
    let selected = s.selected.as_deref() == Some(ch.id.as_str());
    let id = ch.id.clone();
    let title_weight = if ch.unread { 700.0 } else { 400.0 };
    let line = row()
        .w_full()
        .gap(6.0)
        .center_y()
        .child(
            text(ch.title.clone())
                .font(ts(14.0, title_weight))
                .color(c(INK))
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(text(ch.when.clone()).font(ts(12.0, 400.0)).color(c(MUTED)));
    let card = col()
        .w_full()
        .h(CHAT_ROW_H)
        .px(10.0)
        .py(8.0)
        .gap(2.0)
        .radius(8.0)
        .fill(c(if selected { WHITE } else { LIST }))
        .hover(c(HOVER))
        .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| open_chat(s, &id, tx))
        .child(line)
        .child(
            text(ch.preview.clone())
                .font(ts(13.0, 400.0))
                .color(c(MUTED))
                .truncate(),
        );
    col().w_full().pb(2.0).child(card)
}

fn conversation<A: Actions + 'static>(s: &UiState<A>) -> El {
    let Some(id) = s.selected.as_deref() else {
        return col()
            .key("conversation")
            .grow(1.0)
            .min_w(0.0)
            .h_full()
            .center()
            .fill(c(WHITE))
            .child(
                text("Select a chat or channel")
                    .font(ts(16.0, 400.0))
                    .color(c(MUTED)),
            );
    };
    let title = s
        .chats
        .iter()
        .find(|ch| ch.id == id)
        .map(|ch| ch.title.as_str())
        .or_else(|| {
            s.teams
                .iter()
                .flat_map(|team| &team.channels)
                .find(|channel| channel.id == id)
                .map(|channel| channel.name.as_str())
        })
        .unwrap_or(id);
    col()
        .key("conversation")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .fill(c(WHITE))
        .child(
            row().w_full().h(56.0).px(20.0).center_y().child(
                text(title.to_owned())
                    .font(ts(16.0, 700.0))
                    .color(c(INK))
                    .truncate()
                    .grow(1.0)
                    .min_w(0.0),
            ),
        )
        .child(row().w_full().px(20.0).gap(8.0).center_y()
            .child(text_input(&s.search, ts(13.0, 400.0)).key("search").label("Search messages")
                .placeholder("Search cached messages", c(MUTED)).color(c(INK)).grow(1.0)
                .on_edit(|s: &mut UiState<A>| &mut s.search)
                .on_submit(|s: &mut UiState<A>, _| {
                    if let Some(id) = &s.selected { s.actions.search(id, s.search.text.trim()); }
                }))
            .child(row().key("search-close").padding(8.0).hover(c(HOVER))
                .on_press_tx(|s: &mut UiState<A>, _| { s.search.clear(); s.search_results = None; })
                .child(text("Clear").font(ts(12.0, 400.0)).color(c(INK)))))
        .child(el().h(1.0).w_full().fill(c(LINE)))
        .child(message_list(s))
        .child(if s.read_only { row().w_full().padding(16.0).child(text("Read-only").font(ts(13.0, 400.0)).color(c(MUTED))) } else { composer(s) })
}

fn message_list<A: Actions + 'static>(s: &UiState<A>) -> El {
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
        .child(row().key("load-older").w_full().h(36.0).center().hover(c(HOVER))
            .on_press_tx(move |s: &mut UiState<A>, _| s.actions.load_older(&id))
            .child(text("Load older messages").font(ts(13.0, 600.0)).color(c(INK))))
        // Rune: a virtual/growth stack must be the last child of its scroll viewport.
        .child(rows)
}

fn message_row<A: Actions + 'static>(s: &UiState<A>, i: usize) -> El {
    let m = &s.search_results.as_ref().unwrap_or(&s.messages)[i];
    let head = row()
        .gap(8.0)
        .center_y()
        .child(text(m.sender.clone()).font(ts(12.0, 700.0)).color(c(INK)))
        .child(text(m.when.clone()).font(ts(12.0, 400.0)).color(c(MUTED)));
    let bubble = col()
        .max_w(560.0)
        .radius(12.0)
        .px(12.0)
        .py(8.0)
        .fill(c(if m.mine { LAVENDER } else { BUBBLE }))
        .child(
            text(m.text.clone())
                .font(ts(14.0, 400.0))
                .color(c(INK))
                .wrap(),
        ).child(crate::media::view(s, &m.media, &m.id));
    let body = if m.mine {
        row()
            .w_full()
            .justify_end()
            .child(col().items_end().gap(4.0).child(head).child(bubble))
    } else {
        row()
            .w_full()
            .child(col().items_start().gap(4.0).child(head).child(bubble))
    };
    let mut result = col().w_full().pb(12.0).child(body);
    if !m.deleted {
        let mut reactions = row().gap(4.0);
        for reaction in &m.reactions {
            let label = format!("{} {}", reaction.emoji, reaction.count);
            let emoji = reaction.emoji.clone();
            let mine = reaction.mine;
            let message_id = m.id.clone();
            let conversation_id = s.selected.clone().unwrap_or_default();
            let mut pill = row().key(format!("reaction-{}-{emoji}", m.id)).padding(4.0).radius(6.0)
                .fill(c(if mine { LAVENDER } else { BUBBLE })).child(text(label).font(ts(12.0, 400.0)).color(c(INK)));
            if !s.read_only {
                pill = pill.on_press_tx(move |s: &mut UiState<A>, _| s.actions.react(&conversation_id, &message_id, &emoji, mine));
            }
            reactions = reactions.child(pill);
        }
        if !s.read_only && !m.reactions.iter().any(|reaction| reaction.emoji == "👍") {
            let message_id = m.id.clone();
            let conversation_id = s.selected.clone().unwrap_or_default();
            reactions = reactions.child(row().key(format!("like-{}", m.id)).padding(4.0).hover(c(HOVER))
                .on_press_tx(move |s: &mut UiState<A>, _| s.actions.react(&conversation_id, &message_id, "👍", false))
                .child(text("👍").font(ts(12.0, 400.0))));
        }
        result = result.child(reactions);
    }
    if !s.read_only && m.can_delete && !m.deleted {
        let id = m.id.clone();
        let conversation_id = s.selected.clone().unwrap_or_default();
        let confirming = s.confirm_delete.as_deref() == Some(&id);
        result = result.child(row().key(format!("delete-{id}")).h(28.0).px(8.0).hover(c(HOVER))
            .on_press_tx(move |s: &mut UiState<A>, _| {
                if confirming { s.actions.delete_message(&conversation_id, &id); s.confirm_delete = None; }
                else { s.confirm_delete = Some(id.clone()); }
            }).child(text(if confirming { "Confirm delete" } else { "Delete" }).font(ts(12.0, 400.0)).color(c(RED))));
    }
    if !s.read_only && !m.deleted && m.reply_to_id.is_none() && active_target(s).is_some_and(|target| target.channel) {
        let root = m.id.clone();
        result = result.child(row().key(format!("reply-{}", m.id)).h(28.0).px(8.0).hover(c(HOVER))
            .on_press_tx(move |s: &mut UiState<A>, tx| {
                select_thread(s, Some(root.clone()));
                tx.focus("composer", false);
            }).child(text("Reply in thread").font(ts(12.0, 600.0)).color(c(INK))));
    }
    result
}

fn composer<A: Actions + 'static>(s: &UiState<A>) -> El {
    let field = text_input(&s.composer, ts(14.0, 400.0))
        .key("composer")
        .label("Message")
        .placeholder("Type a message", c(MUTED))
        .color(c(INK))
        .w_full()
        .on_edit(|s: &mut UiState<A>| &mut s.composer)
        .on_submit(|s: &mut UiState<A>, _tx: &mut Tx| send(s));
    let send_button = row()
        .key("send")
        .h(30.0)
        .px(14.0)
        .radius(15.0)
        .fill(c(PURPLE))
        .center()
        .on_press_tx(|s: &mut UiState<A>, tx: &mut Tx| {
            send(s);
            tx.focus("composer", false);
        })
        .child(text("Send").font(ts(13.0, 600.0)).color(rune::color::hex(WHITE).expect("white")));
    col()
        .w_full()
        .px(16.0)
        .pt(8.0)
        .pb(12.0)
        .gap(6.0)
        .children(active_target(s).map(|target| crate::media::attachments(s, &target)))
        .children(s.thread_root_id.as_ref().map(|_| row().key("close-reply").h(28.0).px(8.0).hover(c(HOVER))
            .on_press_tx(|s: &mut UiState<A>, _| select_thread(s, None))
            .child(text("Replying in thread · Close").font(ts(12.0, 600.0)).color(c(INK)))))
        .children(s.send_error.as_ref().map(|e| {
            text(e.clone())
                .key("send-error")
                .font(ts(12.0, 400.0))
                .color(c(RED))
                .wrap()
        }))
        .child(
            col()
                .w_full()
                .padding(8.0)
                .gap(6.0)
                .radius(8.0)
                .fill(c(WHITE))
                .border(1.0, c(LINE))
                .child(auto_grow(field, 1, 8))
                .child(row().w_full().justify_end().child(send_button)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChannelRow;
    use rune::{Env, Runtime};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<String>>,
    }

    impl Actions for Fake {
        fn open(&self, conversation_id: &str) {
            self.calls
                .borrow_mut()
                .push(format!("open {conversation_id}"));
        }
        fn send(&self, target: SendTarget, text: String, _files: Vec<teams_core::FileReference>) {
            let verb = if target.channel { "post" } else { "send" };
            self.calls
                .borrow_mut()
                .push(format!("{verb} {} {text}", target.conversation_id));
        }
        fn delete_message(&self, conversation_id: &str, message_id: &str) {
            self.calls.borrow_mut().push(format!("delete {conversation_id} {message_id}"));
        }
    }

    fn chat(id: &str, title: &str) -> ChatRow {
        ChatRow {
            id: id.into(),
            title: title.into(),
            preview: format!("{title}: hi"),
            unread: false,
            when: "14:05".into(),
        }
    }

    fn msg(id: &str, sender: &str, text: &str, mine: bool) -> MessageRow {
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

    fn rt() -> Runtime<UiState<Fake>> {
        let env = Env {
            system_dark: false,
            scale: 1.0,
        };
        let mut rt = Runtime::new(
            UiState::new(Fake::default()),
            view::<Fake>,
            (1000.0, 700.0),
            env,
        );
        rt.frame(0.0);
        rt
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn press(rt: &mut Runtime<UiState<Fake>>, key: Key, mods: Mods) {
        rt.key(KeyEvent { key, mods });
        rt.frame(1.0 / 60.0);
    }

    fn calls(rt: &Runtime<UiState<Fake>>) -> Vec<String> {
        rt.state.actions.calls.borrow().clone()
    }

    /// Laid-out rect `[x, y, w, h]` of the built element at `key`.
    fn rect(rt: &Runtime<UiState<Fake>>, key: &str) -> [f32; 4] {
        let suffix = format!("/{key}");
        rt.elements()
            .into_iter()
            .find(|e| e.path.ends_with(&suffix))
            .unwrap_or_else(|| panic!("no built element {key}"))
            .rect
    }

    /// State changes made outside a handler must go through `update` so the view re-runs.
    fn apply(rt: &mut Runtime<UiState<Fake>>, update: Update) {
        rt.update(|s, _| s.apply(update));
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn chats_update_shows_rows() {
        let mut rt = rt();
        apply(
            &mut rt,
            Update::Chats(vec![chat("a", "Ada"), chat("b", "Bo")]),
        );
        rt.frame(1.0 / 60.0);
        assert!(rt.find("chat-a").is_some() && rt.find("chat-b").is_some());
        assert!(rt.find("chat-c").is_none());
    }

    #[test]
    fn team_expansion_opens_channel_and_posts() {
        let mut rt = rt();
        apply(
            &mut rt,
            Update::Teams(vec![TeamRow {
                id: "team".into(),
                name: "Team".into(),
                channels: vec![ChannelRow {
                    id: "channel".into(),
                    name: "General".into(),
                    unread: true,
                }],
            }]),
        );
        assert!(rt.find("channel-channel").is_none());
        click(&mut rt, "team-team");
        click(&mut rt, "channel-channel");
        assert_eq!(rt.state.selected.as_deref(), Some("channel"));
        assert_eq!(calls(&rt), ["open channel"]);
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("hello".into()), Mods::default());
        click(&mut rt, "send");
        assert_eq!(calls(&rt), ["open channel", "post channel hello"]);
        apply(
            &mut rt,
            Update::Messages {
                conversation_id: "channel".into(),
                rows: vec![msg("post", "Ada", "Hello", false)],
            },
        );
        assert!(rt.find("msg-post").is_some());
        click(&mut rt, "team-team");
        assert!(rt.find("channel-channel").is_none());
    }

    #[test]
    fn clicking_a_chat_selects_it_and_opens_it() {
        let mut rt = rt();
        apply(
            &mut rt,
            Update::Chats(vec![chat("a", "Ada"), chat("b", "Bo")]),
        );
        rt.frame(1.0 / 60.0);
        click(&mut rt, "chat-b");
        assert_eq!(rt.state.selected.as_deref(), Some("b"));
        assert_eq!(calls(&rt), ["open b"]);
    }

    #[test]
    fn messages_of_the_selected_chat_show_and_others_are_dropped() {
        let mut rt = rt();
        apply(
            &mut rt,
            Update::Chats(vec![chat("a", "Ada"), chat("b", "Bo")]),
        );
        rt.frame(1.0 / 60.0);
        click(&mut rt, "chat-b");
        apply(
            &mut rt,
            Update::Messages {
                conversation_id: "a".into(),
                rows: vec![msg("a1", "Ada", "x", false)],
            },
        );
        assert!(rt.state.messages.is_empty());
        apply(
            &mut rt,
            Update::Messages {
                conversation_id: "b".into(),
                rows: vec![
                    msg("b1", "Bo", "hi there", false),
                    msg("b2", "Me", "yo", true),
                ],
            },
        );
        assert!(rt.find("msg-b1").is_some() && rt.find("msg-b2").is_some());
        assert!(rt.find("msg-a1").is_none());
    }

    #[test]
    fn typing_then_send_calls_send_and_clears_the_composer() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("b", "Bo")]));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "chat-b");
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("  hello".into()), Mods::default());
        click(&mut rt, "send");
        assert_eq!(calls(&rt), ["open b", "send b hello"]);
        assert!(rt.state.composer.text.is_empty());
    }

    #[test]
    fn enter_sends_and_blank_text_does_not() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("b", "Bo")]));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "chat-b");
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("   ".into()), Mods::default());
        press(&mut rt, Key::Enter, Mods::default());
        assert_eq!(calls(&rt), ["open b"]);
        press(&mut rt, Key::Text("x".into()), Mods::default());
        press(&mut rt, Key::Enter, Mods::default());
        assert_eq!(calls(&rt), ["open b", "send b x"]);
        assert!(rt.state.composer.text.is_empty());
    }

    #[test]
    fn a_row_starts_below_a_taller_message_before_it() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("b", "Bo")]));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "chat-b");
        let long = (0..12)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        apply(
            &mut rt,
            Update::Messages {
                conversation_id: "b".into(),
                rows: vec![msg("b1", "Bo", &long, false), msg("b2", "Me", "yo", true)],
            },
        );
        rt.frame(0.0);
        let [_, top1, _, h1] = rect(&rt, "msg-b1");
        let [_, top2, _, _] = rect(&rt, "msg-b2");
        assert!(
            h1 > MESSAGE_ROW_H,
            "first row should be taller than the estimate, got {h1}"
        );
        assert!(
            top2 >= top1 + h1,
            "second row at {top2} overlaps first row ending at {}",
            top1 + h1
        );
    }

    #[test]
    fn a_send_failure_shows_under_the_composer() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("b", "Bo")]));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "chat-b");
        apply(
            &mut rt,
            Update::SendFailed {
                target: SendTarget { conversation_id: "b".into(), thread_root_id: None, channel: false },
                text: "hello".into(),
                files: vec![],
                error: "offline".into(),
            },
        );
        rt.frame(1.0 / 60.0);
        assert!(rt.find("send-error").is_some());
        assert_eq!(rt.state.composer.text, "hello");
    }

    #[test]
    fn drafts_and_late_send_failures_stay_with_their_chat() {
        let mut rt = rt();
        apply(
            &mut rt,
            Update::Chats(vec![chat("a", "Ada"), chat("b", "Bo")]),
        );
        click(&mut rt, "chat-a");
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("draft a".into()), Mods::default());
        click(&mut rt, "chat-b");
        assert!(rt.state.composer.text.is_empty());
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("draft b".into()), Mods::default());
        apply(
            &mut rt,
            Update::SendFailed {
                target: SendTarget { conversation_id: "a".into(), thread_root_id: None, channel: false },
                text: "failed".into(),
                files: vec![],
                error: "offline".into(),
            },
        );
        assert_eq!(rt.state.composer.text, "draft b");
        assert!(rt.state.send_error.is_none());
        click(&mut rt, "chat-a");
        assert_eq!(rt.state.composer.text, "failed\ndraft a");
        assert!(
            rt.state
                .send_error
                .as_deref()
                .unwrap()
                .contains("could not be confirmed")
        );
        click(&mut rt, "chat-b");
        assert_eq!(rt.state.composer.text, "draft b");
    }

    #[test]
    fn failed_send_preserves_new_text_in_the_same_chat() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("a", "Ada")]));
        click(&mut rt, "chat-a");
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("sent".into()), Mods::default());
        click(&mut rt, "send");
        press(&mut rt, Key::Text("new draft".into()), Mods::default());
        apply(
            &mut rt,
            Update::SendFailed {
                target: SendTarget { conversation_id: "a".into(), thread_root_id: None, channel: false },
                text: "sent".into(),
                files: vec![],
                error: "offline".into(),
            },
        );
        assert_eq!(rt.state.composer.text, "sent\nnew draft");
        assert!(rt.state.send_error.is_some());
    }

    #[test]
    fn failed_thread_reply_does_not_replace_channel_post_draft() {
        let mut rt = rt();
        apply(&mut rt, Update::Teams(vec![TeamRow { id: "team".into(), name: "Team".into(), channels: vec![ChannelRow { id: "channel".into(), name: "General".into(), unread: false }] }]));
        click(&mut rt, "team-team");
        click(&mut rt, "channel-channel");
        apply(&mut rt, Update::Messages { conversation_id: "channel".into(), rows: vec![msg("root", "Ada", "post", false)] });
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("new post".into()), Mods::default());
        click(&mut rt, "reply-root");
        assert!(rt.state.composer.text.is_empty());
        click(&mut rt, "close-reply");
        apply(&mut rt, Update::SendFailed { target: SendTarget { conversation_id: "channel".into(), thread_root_id: Some("root".into()), channel: true }, text: "reply".into(), files: vec![], error: "offline".into() });
        assert_eq!(rt.state.composer.text, "new post");
        click(&mut rt, "reply-root");
        assert_eq!(rt.state.composer.text, "reply");
    }

    #[test]
    fn deleting_requires_confirmation() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("a", "Ada")]));
        click(&mut rt, "chat-a");
        let mut row = msg("m", "Me", "hello", true);
        row.can_delete = true;
        apply(&mut rt, Update::Messages { conversation_id: "a".into(), rows: vec![row] });
        click(&mut rt, "delete-m");
        assert_eq!(calls(&rt), ["open a"]);
        click(&mut rt, "delete-m");
        assert_eq!(calls(&rt), ["open a", "delete a m"]);
    }
}
