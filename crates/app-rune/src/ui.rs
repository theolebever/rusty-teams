//! The Rune view of the Teams client: chat list on the left, the open conversation on the
//! right, the composer under it. Bridge updates come in through `UiState::apply`; user intent
//! goes out through the `Actions` the state owns.

use crate::types::{Actions, ChatRow, MessageRow, SendTarget, TeamRow, Update};
use rune::tok::Family;
use rune::*;
use std::collections::HashMap;

pub(crate) const HOVER: &str = "#EBEBEB";
pub(crate) const LINE: &str = "#E0E0E0";
pub(crate) const INK: &str = "#242424";
pub(crate) const MUTED: &str = "#616161";
pub(crate) const WHITE: &str = "#FFFFFF";


pub(crate) fn c(s: &str) -> Color {
    crate::settings::color(s)
}

pub(crate) fn ts(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Segoe UI"))
}

/// `rune::text` with colour emoji. Rune only routes a grapheme to Segoe UI Emoji when it carries
/// `U+FE0F`; Teams sends `👍` `😆` `❤` bare, which Segoe UI Symbol then draws in monochrome ink.
/// Every view module imports this one explicitly, which shadows the `rune::*` glob.
pub(crate) fn text(content: impl Into<String>) -> El {
    let content = content.into();
    if content.is_ascii() { rune::text(content) } else { rune::text(teams_core::emoji_presentation(&content)) }
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
    /// Own presence, status menu and status dialog (`status_menu.rs`).
    pub own: crate::status_menu::State,
    pub profile: crate::profile::State,
    pub toast: crate::toast::State,
    pub chats: Vec<ChatRow>,
    pub selected: Option<String>,
    pub teams: Vec<TeamRow>,
    pub expanded_teams: Vec<String>,
    pub chatlist: crate::view_chatlist::ChatListState,
    /// Messages of `selected`, oldest first.
    pub messages: Vec<MessageRow>,
    pub send_error: Option<String>,
    pub composer: TextEdit,
    /// The in-conversation search field, shown while `search_open`.
    pub(crate) search: TextEdit,
    pub(crate) search_open: bool,
    pub(crate) search_results: Option<Vec<MessageRow>>,
    /// Pinned message of the open conversation, if any (`view_header.rs`).
    pub pinned: Option<crate::types::PinnedRow>,
    /// Member counts by conversation id; the header shows the count when one is known.
    pub member_counts: HashMap<String, usize>,
    pub details: HashMap<String, crate::types::ChatDetails>,
    pub typing: HashMap<(String, String), chatsvc::TypingEvent>,
    pub shared_open: bool,
    pub rename_open: bool,
    pub rename_title: TextEdit,
    pub format_open: bool,
    pub emoji_open: bool,
    /// Mentions, pickers and GIFs of the composer (`view_composer.rs`).
    pub(crate) compose: crate::view_composer::Compose,
    pub agent: crate::agent_panel::State,
    /// Switcher, find bar, shortcuts dialog and jump bar (`shortcuts.rs`).
    pub nav: crate::shortcuts::NavState,
    pub global_search: crate::search::SearchState,
    pub thread_root_id: Option<String>,
    pub expanded_posts: std::collections::HashSet<String>,
    pub picker: crate::reactions::Picker,
    pub(crate) confirm_delete: Option<String>,
    /// The message whose "more" actions are showing, if any.
    pub(crate) more_open: Option<String>,
    pub(crate) menu: crate::message_menu::MenuState,
    /// Ids of messages that just arrived live in the open chat; they slide in once (`view_messages.rs`).
    /// `poll` empties it, so only the commit that adds them animates.
    pub(crate) appearing: Vec<String>,
    drafts: HashMap<SendTarget, String>,
    send_errors: HashMap<SendTarget, String>,
}

impl<A: Actions> UiState<A> {
    pub fn new(actions: A) -> Self {
        let read_only = actions.read_only();
        UiState {
            actions,
            agent: Default::default(),
            nav: Default::default(),
            details: HashMap::new(),
            typing: HashMap::new(),
            shared_open: false,
            rename_open: false,
            rename_title: TextEdit::default(),
            format_open: false,
            emoji_open: false,
            compose: Default::default(),
            read_only,
            screens: crate::screens::ScreenState::default(),
            calls: crate::calls::CallView::default(),
            media: crate::media::MediaState::default(),
            appearance: Default::default(),
            status: String::new(),
            own: Default::default(),
            profile: Default::default(),
            toast: Default::default(),
            chats: Vec::new(),
            selected: None,
            teams: Vec::new(),
            expanded_teams: Vec::new(),
            chatlist: Default::default(),
            messages: Vec::new(),
            send_error: None,
            composer: TextEdit::new(""),
            search: TextEdit::new(""),
            search_open: false,
            search_results: None,
            pinned: None,
            member_counts: HashMap::new(),
            global_search: Default::default(),
            thread_root_id: None,
            expanded_posts: Default::default(),
            picker: Default::default(),
            confirm_delete: None,
            more_open: None,
            menu: Default::default(),
            drafts: HashMap::new(),
            send_errors: HashMap::new(),
            appearing: Vec::new(),
        }
    }

    /// Applies one bridge update. Messages for a conversation other than `selected` are dropped.
    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Audio { url, result } => self.media.voice.loaded(url, result),
            Update::CallHistory(result) => {
                self.calls.history_loading = false;
                match result {
                    Ok(history) => { self.calls.history = history; self.calls.history_error = None; }
                    Err(error) => self.calls.history_error = Some(error),
                }
            }
            Update::AgentSettings(settings) => self.agent.settings = settings,
            Update::AgentResult(result) => { self.agent.output = result; self.agent.running = false; }
            Update::Details(details) => {
                for (id, chat) in &details { self.member_counts.insert(id.clone(), chat.members.len()); }
                self.details = details;
            }
            Update::Pinned { conversation_id, pin } => {
                if self.selected.as_deref() == Some(&conversation_id) { self.pinned = pin; }
            }
            Update::Edited { conversation_id, ids } => {
                if self.selected.as_deref() == Some(&conversation_id) { self.menu.edited = ids.into_iter().collect(); }
            }
            Update::Typing(event) => {
                let key = (event.conversation_id.clone(), event.user_id.clone());
                if event.active { self.typing.insert(key, event); } else { self.typing.remove(&key); }
            }
            Update::People { query, rows } => crate::new_chat::apply_people(self, &query, rows),
            Update::ChatCreated(id) => self.chatlist.manage.pending_open = Some(id),
            Update::Status(s) => { self.toast.observe(&s); self.status = s; }
            Update::Me { user_id, name, email } => self.own.me = Some(crate::status_menu::Me { user_id, name, email }),
            Update::OwnStatus(status) => self.own.status = status,
            Update::Profile { user_id, result } => self.profile.loaded(user_id, result),
            Update::OpenChat(id) => self.profile.chat_request = Some(id),
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
            Update::ImagePreview { key, bytes, preview } => self.media.loaded_preview(key, bytes, preview),
            Update::ImageFailed { key, error } => { self.media.errors.insert(key, error); }
            Update::Attachment { target, path, result } => {
                if let Some(orphan) = self.media.uploaded(target, path, result) { self.actions.discard_attachment(orphan); }
            }
            Update::Calendar { week, rows } => {
                if self.screens.week == week { self.screens.calendar = rows; }
            }
            Update::CalendarRefresh => self.actions.load_calendar(self.screens.week),
            Update::Agenda(events) => self.screens.agenda = events,
            Update::Search { conversation_id, query, rows } => {
                if self.selected.as_deref() == Some(&conversation_id) && self.search.text.trim() == query {
                    self.search_results = Some(rows);
                }
            }
            Update::GlobalSearch(results) => crate::search::apply(&mut self.global_search, results),
            Update::Messages {
                conversation_id,
                rows,
            } => {
                if self.selected.as_deref() == Some(conversation_id.as_str()) {
                    for image in rows.iter().flat_map(|row| row.media.images.iter().cloned().chain(row.media.rich.images())) {
                        if !self.media.images.contains_key(image.key()) && !self.media.errors.contains_key(image.key()) {
                            self.actions.load_image(image.clone());
                        }
                    }
                    let rows = crate::teams::group_replies(rows);
                    self.appearing = arrived(&self.messages, &rows);
                    self.messages = rows;
                }
            }
            Update::Gifs { query, result } => self.compose.gif.loaded(query, result),
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
                    self.composer = TextEdit::new(join_draft(&text, current));
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

/// Messages of `new` that arrived live since `old`: never the first load, a chat switch, an
/// older page (it goes in above the first known message) or a catch-up of many.
fn arrived(old: &[MessageRow], new: &[MessageRow]) -> Vec<String> {
    if old.is_empty() || old.first().map(|m| &m.id) != new.first().map(|m| &m.id) { return Vec::new() }
    let known: std::collections::HashSet<&str> = old.iter().map(|m| m.id.as_str()).collect();
    let fresh: Vec<String> = new.iter().filter(|m| !known.contains(m.id.as_str())).map(|m| m.id.clone()).collect();
    // ponytail: more than three at once is a reconnect catch-up; they land without motion.
    if fresh.len() > 3 { Vec::new() } else { fresh }
}

pub(crate) fn open_chat<A: Actions>(s: &mut UiState<A>, id: &str, tx: &mut Tx) {
    save_draft(s);
    s.selected = Some(id.to_owned());
    s.shared_open = false;
    s.rename_open = false;
    s.pinned = None;
    s.thread_root_id = None;
    s.expanded_posts.clear();
    s.picker.close();
    s.confirm_delete = None;
    s.more_open = None;
    s.menu = Default::default();
    s.messages.clear();
    s.media.voice.pause();
    s.search.clear();
    s.search_open = false;
    s.search_results = None;
    s.nav.touch(id);
    s.nav.find = Default::default();
    s.nav.scrolled_up = false;
    restore_draft(s);
    s.actions.open(id);
    tx.scroll_to_bottom_instant("messages");
}

pub(crate) fn active_target<A: Actions>(s: &UiState<A>) -> Option<SendTarget> {
    let conversation_id = s.selected.clone()?;
    let channel = s.teams.iter().flat_map(|team| &team.channels).any(|channel| channel.id == conversation_id);
    Some(SendTarget { conversation_id, thread_root_id: s.thread_root_id.clone(), channel })
}

pub(crate) fn save_draft<A: Actions>(s: &mut UiState<A>) {
    if let Some(target) = active_target(s) {
        s.drafts.insert(target.clone(), s.composer.text.clone());
        if let Some(error) = s.send_error.take() { s.send_errors.insert(target, error); }
    }
}

fn restore_draft<A: Actions>(s: &mut UiState<A>) {
    if let Some(target) = active_target(s) {
        let stored = if target.thread_root_id.is_none() { s.actions.load_draft(&target.conversation_id) } else { None };
        s.composer = TextEdit::new(s.drafts.remove(&target).or(stored).unwrap_or_default());
        s.send_error = s.send_errors.remove(&target);
    }
}

pub(crate) fn select_thread<A: Actions>(s: &mut UiState<A>, root: Option<String>) {
    save_draft(s);
    s.thread_root_id = root;
    if let (Some(conversation), Some(root)) = (&s.selected, &s.thread_root_id) {
        s.expanded_posts.insert(root.clone());
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

pub(crate) fn send<A: Actions>(s: &mut UiState<A>) {
    let Some(target) = active_target(s) else { return };
    let text = s.composer.text.trim().to_owned();
    let files = s.media.files(&target);
    if s.media.is_uploading(&target) || (text.is_empty() && files.is_empty() && s.compose.gifs.is_empty()) {
        return;
    }
    s.send_error = None;
    s.media.clear(&target);
    let composed = s.compose.take(text);
    s.actions.send_composed(target, composed, files);
    s.composer.clear();
}

pub fn view<A: Actions + 'static>(s: &UiState<A>, cx: &mut Cx<UiState<A>>) -> El {
    s.media.begin_render();
    crate::theme::apply(cx, &s.appearance);
    let width = cx.layout_size("root").map_or(1100.0, |size| size[0]);
    let arriving = !s.appearing.is_empty() && !cx.is_reduced_motion();
    if s.toast.reconnecting {
        return col().key("root").w_full().h_full().fill(c(WHITE))
            .child(crate::view_chrome::title_bar(s, cx.chrome_insets().trailing))
            .child(crate::toast::reconnect(s, cx.is_reduced_motion()));
    }
    let root = col().key("root").w_full().h_full();
    crate::view_messages::snappy(root, arriving)
        // Snappy UI: no mount/unmount fades, layout tweens or colour springs anywhere below,
        // except the commit where messages arrive live (`view_messages::snappy`).
        .motion(Spring::NONE)
        .fill(c(WHITE))
        .child(crate::view_chrome::title_bar(s, cx.chrome_insets().trailing))
        .children(crate::toast::banner(s))
        .child(
            row()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .child(crate::view_chrome::rail(s))
                .child(if matches!(s.screens.screen, crate::screens::Screen::Chat | crate::screens::Screen::Teams | crate::screens::Screen::Activity) {
                    let sidebar = if matches!(s.screens.screen, crate::screens::Screen::Teams | crate::screens::Screen::Activity) {
                        crate::screens::view(s)
                    } else {
                        crate::view_chatlist::pane(s, (width * 0.28).clamp(220.0, 320.0))
                    };
                    row().grow(1.0).min_w(0.0).h_full().child(sidebar).child(conversation(s, width < 1200.0, arriving))
                } else { crate::screens::view(s) }),
        )
        .children(crate::toast::layer(s))
        .children(crate::profile::layer(s))
        .children(crate::status_menu::layer(s))
        .children(crate::image_viewer::overlay(s))
        .child(crate::chat_menu::overlay(s))
        .children(crate::message_menu::overlay(s))
        .children(crate::shortcuts::overlays(s))
}

fn conversation<A: Actions + 'static>(s: &UiState<A>, compact: bool, arriving: bool) -> El {
    if s.screens.screen == crate::screens::Screen::Activity && s.screens.selected_activity.is_none() {
        return col().key("activity-preview").grow(1.0).min_w(0.0).h_full().center().gap(12.0)
            .child(crate::icons::sym("notifications", 40.0).color(c(MUTED)))
            .child(text("Select a notification").font(ts(18.0, 600.0)).color(c(INK)))
            .child(text("See the conversation and its context here.").font(ts(14.0, 400.0)).color(c(MUTED)));
    }
    if s.selected.is_none() && s.screens.screen == crate::screens::Screen::Teams {
        return crate::teams::overview(s);
    }
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
    let conversation = col().key("conversation").grow(1.0).min_w(0.0).h_full().min_h(0.0).fill(c(WHITE));
    crate::view_messages::snappy(conversation, arriving)
        .child(crate::view_header::header(s, id, compact))
        .child(crate::find::bar(s))
        .child(if s.shared_open { crate::view_messages::shared(s) } else { crate::view_messages::message_list(s, arriving) })
        .child(crate::find::jump_bar(s))
        .child(if s.read_only { row().w_full().padding(16.0).child(text("Read-only").font(ts(13.0, 400.0)).color(c(MUTED))) } else { crate::view_composer::composer(s, compact) })
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
    fn switching_screens_shows_the_new_pane_at_once_without_a_fade() {
        let mut rt = rt();
        click(&mut rt, "nav-Teams");
        let pane = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("teams-overview")).unwrap();
        assert_eq!(pane.opacity, 1.0, "pane must not fade in");
        assert!(rt.elements().iter().all(|e| !e.exiting), "old pane must not fade out");
    }

    #[test]
    fn teams_navigation_keeps_a_chat_draft_and_team_cards_open_channels() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("a", "Ada")]));
        apply(&mut rt, Update::Teams(vec![TeamRow { id: "team".into(), name: "Team".into(), channels: vec![ChannelRow { hidden: false, id: "general".into(), name: "General".into(), unread: false }] }]));
        click(&mut rt, "chat-a");
        rt.update(|s, _| s.composer = TextEdit::new("Keep this draft"));
        rt.frame(1.0);
        click(&mut rt, "nav-Teams");
        rt.frame(2.0);
        assert!(rt.find("teams-overview").is_some());
        assert!(rt.state.selected.is_none());
        click(&mut rt, "team-card-team");
        assert_eq!(rt.state.selected.as_deref(), Some("general"));
        click(&mut rt, "nav-Chat");
        rt.frame(3.0);
        click(&mut rt, "chat-a");
        assert_eq!(rt.state.composer.text, "Keep this draft");
    }

    #[test]
    fn conversation_metadata_ignores_late_pins_and_tracks_typing_stop() {
        let mut state = UiState::new(Fake::default());
        state.selected = Some("b".into());
        state.apply(Update::Pinned { conversation_id: "a".into(), pin: Some(crate::types::PinnedRow {
            conversation_id: "a".into(), sender: "Ada".into(), text: "Private".into(),
        }) });
        assert!(state.pinned.is_none());
        state.apply(Update::Details(HashMap::from([("b".into(), crate::types::ChatDetails {
            avatar_id: None, group: vec![],
            members: vec![store::MemberRecord { user_id: Some("ada".into()), display_name: "Ada".into() }],
            muted: true, presence: None,
        })])));
        assert_eq!(state.member_counts["b"], 1);
        assert!(state.details["b"].muted);
        let mut event = chatsvc::TypingEvent { conversation_id: "b".into(), user_id: "ada".into(), display_name: "Ada".into(), active: true, received_at: chrono::Utc::now() };
        state.apply(Update::Typing(event.clone()));
        assert_eq!(state.typing.len(), 1);
        event.active = false;
        state.apply(Update::Typing(event));
        assert!(state.typing.is_empty());
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
                    hidden: false,
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
            h1 > crate::view_messages::MESSAGE_ROW_H,
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
        apply(&mut rt, Update::Teams(vec![TeamRow { id: "team".into(), name: "Team".into(), channels: vec![ChannelRow { hidden: false, id: "channel".into(), name: "General".into(), unread: false }] }]));
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
        click(&mut rt, "more-m");
        click(&mut rt, "delete-m");
        assert_eq!(calls(&rt), ["open a"]);
        click(&mut rt, "delete-m");
        assert_eq!(calls(&rt), ["open a", "delete a m"]);
    }
}
