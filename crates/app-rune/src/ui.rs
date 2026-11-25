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
    pub(crate) window_focused: bool,
    pub(crate) starting: bool,
    /// Set once the connection reached Online this run; before that an expired session opens the picker.
    pub(crate) online_once: bool,
    pub(crate) started_at: std::time::Instant,
    pub(crate) splash_stage: u8,
    /// Launched behind the splash, so the content fades up when it goes.
    pub(crate) from_splash: bool,
    pub(crate) demo_ready_at: Option<std::time::Instant>,
    pub screens: crate::screens::ScreenState,
    pub calls: crate::calls::CallView,
    pub(crate) call_window: Option<crate::call_window::CallWindow>,
    pub(crate) detached_chat: bool,
    pub(crate) chat_windows: std::collections::HashMap<String, rune::AppHandle<UiState<crate::bridge::Bridge>>>,
    pub media: crate::media::MediaState,
    pub(crate) cards: crate::card_state::State,
    pub(crate) translation: crate::translation::State,
    pub(crate) library: crate::library::State,
    pub(crate) meeting: crate::meeting_form::State,
    pub appearance: app_backend::appearance::AppearanceSettings,
    pub status: String,
    /// Own presence, status menu and status dialog (`status_menu.rs`).
    pub own: crate::status_menu::State,
    pub profile: crate::profile::State,
    pub toast: crate::toast::State,
    /// The account picker replaces the window while open (`account_picker.rs`).
    pub account_picker: Option<crate::account_picker::Picker>,
    pub chats: Vec<ChatRow>,
    pub selected: Option<String>,
    pub teams: Vec<TeamRow>,
    pub expanded_teams: Vec<String>,
    pub chatlist: crate::view_chatlist::ChatListState,
    /// Messages of `selected`, oldest first.
    pub messages: Vec<MessageRow>,
    pub(crate) outbox: Vec<crate::outbox::Row>,
    pub(crate) schedule: crate::schedule::State,
    pub(crate) receipts: HashMap<String, teams_core::ReceiptState>,
    pub(crate) first_unread: Option<String>,
    pub(crate) deleted_here: std::collections::HashSet<(String, String)>,
    pub send_error: Option<String>,
    pub composer: TextEdit,
    /// The in-conversation search field, shown while `search_open`.
    pub(crate) search: TextEdit,
    pub(crate) search_open: bool,
    pub(crate) search_results: Option<Vec<MessageRow>>,
    /// Pinned message of the open conversation, if any (`view_header.rs`).
    pub pinned: Vec<crate::types::PinnedRow>,
    pub pin_index: usize,
    /// Member counts by conversation id; the header shows the count when one is known.
    pub member_counts: HashMap<String, usize>,
    pub details: HashMap<String, crate::types::ChatDetails>,
    pub typing: HashMap<(String, String), chatsvc::TypingEvent>,
    pub shared_open: bool,
    pub details_open: bool,
    pub channel_descriptions: HashMap<String, Result<String, String>>,
    pub rename_open: bool,
    pub rename_title: TextEdit,
    pub format_open: bool,
    pub emoji_open: bool,
    /// Mentions, pickers and GIFs of the composer (`view_composer.rs`).
    pub(crate) compose: crate::view_composer::Compose,
    pub agent: crate::agent_panel::State,
    pub settings: crate::settings::State,
    pub reminders: state::call::reminder::MeetingToasts,
    /// Running meetings by thread id, looked up near a reminded meeting (`reminders::poll`).
    pub(crate) live_meetings: HashMap<String, calling::meeting::LiveMeeting>,
    pub tray: crate::tray::State,
    /// The launch option of the first process, run at the first poll (`arguments::Command`).
    pub(crate) launch_command: Option<crate::arguments::Command>,
    pub(crate) launch_reply_last: Option<String>,
    /// The incoming call ringtone, stopped when dropped (`notifications::ring`).
    pub(crate) ringtone: Option<crate::sound::Handle>,
    /// Whether a call rang at the last frame, so a silent ring flashes once (`notifications::ring`).
    pub(crate) was_ringing: bool,
    /// The looping ringback or reconnecting tone of the current call (`notifications::ring`).
    /// Chat list section settings (`teams_core::stored_section_settings`), for the badge.
    pub(crate) section_settings: teams_core::ChatSectionSettings,
    pub(crate) call_tone: Option<(app_backend::notify::sounds::CallTone, crate::sound::Handle)>,
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
    compose_drafts: HashMap<SendTarget, crate::view_composer::Compose>,
    reply_drafts: HashMap<SendTarget, crate::types::ReplyReference>,
    send_errors: HashMap<SendTarget, String>,
}

impl<A: Actions> UiState<A> {
    pub fn new(actions: A) -> Self {
        let read_only = actions.read_only();
        UiState {
            actions,
            agent: Default::default(),
            settings: Default::default(),
            reminders: Default::default(),
            live_meetings: HashMap::new(),
            tray: Default::default(),
            launch_command: None,
            launch_reply_last: None,
            ringtone: None,
            was_ringing: false,
            call_tone: None,
            section_settings: Default::default(),
            nav: Default::default(),
            details: HashMap::new(),
            typing: HashMap::new(),
            shared_open: false,
            details_open: false,
            channel_descriptions: HashMap::new(),
            rename_open: false,
            rename_title: TextEdit::default(),
            format_open: false,
            emoji_open: false,
            compose: Default::default(),
            read_only,
            window_focused: false,
            starting: false,
            online_once: false,
            started_at: std::time::Instant::now(),
            splash_stage: 0,
            from_splash: false,
            demo_ready_at: None,
            screens: crate::screens::ScreenState::default(),
            calls: crate::calls::CallView::default(),
            call_window: None,
            detached_chat: false,
            chat_windows: Default::default(),
            media: crate::media::MediaState::default(),
            cards: Default::default(),
            translation: Default::default(),
            library: Default::default(),
            meeting: Default::default(),
            appearance: Default::default(),
            status: String::new(),
            own: Default::default(),
            profile: Default::default(),
            toast: Default::default(),
            account_picker: None,
            chats: Vec::new(),
            selected: None,
            teams: Vec::new(),
            expanded_teams: Vec::new(),
            chatlist: Default::default(),
            messages: Vec::new(),
            outbox: Vec::new(),
            schedule: Default::default(),
            receipts: HashMap::new(),
            first_unread: None,
            deleted_here: Default::default(),
            send_error: None,
            composer: TextEdit::new(""),
            search: TextEdit::new(""),
            search_open: false,
            search_results: None,
            pinned: Vec::new(),
            pin_index: 0,
            member_counts: HashMap::new(),
            global_search: Default::default(),
            thread_root_id: None,
            expanded_posts: Default::default(),
            picker: Default::default(),
            confirm_delete: None,
            more_open: None,
            menu: Default::default(),
            drafts: HashMap::new(),
            compose_drafts: HashMap::new(),
            reply_drafts: HashMap::new(),
            send_errors: HashMap::new(),
            appearing: Vec::new(),
        }
    }

    /// Applies one bridge update. Messages for a conversation other than `selected` are dropped.
    pub(crate) fn foreground_chat(&self) -> Option<&str> {
        if self.window_focused && matches!(self.screens.screen, crate::screens::Screen::Chat | crate::screens::Screen::Teams) {
            self.selected.as_deref()
        } else {
            None
        }
    }

    /// Channels the user has not hidden, in teams they have not hidden.
    pub(crate) fn visible_channels(&self) -> impl Iterator<Item = (&crate::types::TeamRow, &crate::types::ChannelRow)> {
        self.teams.iter()
            .filter(|team| !self.chatlist.team_details.get(&team.id).is_some_and(|details| details.hidden))
            .flat_map(|team| team.channels.iter().filter(|channel| !channel.hidden).map(move |channel| (team, channel)))
    }

    pub(crate) fn channel(&self, id: &str) -> Option<&crate::types::ChannelRow> {
        self.teams.iter().flat_map(|team| &team.channels).find(|channel| channel.id == id)
    }

    pub fn apply(&mut self, update: Update) {
        match update {
            Update::PopOutChat(_) => {},
            Update::MeetingCreated { request, result } => {
                if request == self.meeting.request {
                    self.meeting.sending = false;
                    match &result { Ok(()) => self.meeting.open = false, Err(error) => self.meeting.error = Some(error.clone()) }
                }
                if result.is_ok() { self.toast.show("Meeting created"); crate::screens::reload_calendar(self); }
            }
            Update::InlineFile { target, marker, name, result } => {
                if active_target(self).as_ref() == Some(&target) {
                    self.compose.inline_loading.remove(&marker);
                    if self.composer.text.contains(&marker) {
                        let result = result.and_then(|bytes| crate::inline_compose::insert(self, marker.clone(), bytes, name, false));
                        if let Err(error) = result { self.composer = TextEdit::new(self.composer.text.replace(&marker, "")); self.toast.show(error); }
                    }
                } else if let Some(compose) = self.compose_drafts.get_mut(&target) {
                    compose.inline_loading.remove(&marker);
                    if self.drafts.get(&target).is_some_and(|text| text.contains(&marker)) {
                        let result = result.and_then(|bytes| crate::inline_compose::image(bytes, name));
                        match result {
                            Ok(image) if compose.inline.iter().map(|(_, image)| image.image.bytes.len()).sum::<usize>().saturating_add(image.image.bytes.len()) <= crate::inline_compose::MAX_DRAFT_BYTES => compose.inline.push((marker, image)),
                            Ok(_) => {
                                if let Some(text) = self.drafts.get_mut(&target) { *text = text.replace(&marker, ""); }
                                self.toast.show("A draft can contain up to 64 MiB of inline images");
                            }
                            Err(error) => {
                                if let Some(text) = self.drafts.get_mut(&target) { *text = text.replace(&marker, ""); }
                                self.toast.show(error);
                            }
                        }
                    }
                }
            }
            Update::ImageCopied(result) => match result { Ok(bytes) => self.media.clipboard_image = Some(bytes), Err(error) => self.toast.show(error) },
            Update::Video { url, result } => self.media.video.loaded(url, result),
            Update::LibraryRoot { channel, result } => crate::library::root(self, channel, result),
            Update::LibraryRows { channel, folder, result } => crate::library::loaded(self, channel, folder, result),
            Update::Translated { conversation, message, source, language, result } => crate::translation::apply(self, conversation, message, source, language, result),
            Update::LanguageStamps { conversation, result } => {
                if self.selected.as_deref() == Some(&conversation) {
                    match result {
                        Ok(rows) => self.translation.stamps = rows.into_iter().map(|row| (row.message_id, row.stamp)).collect(),
                        Err(error) => self.toast.show(format!("Translation languages unavailable: {error}")),
                    }
                }
            }
            Update::PostFollowed { conversation, root, followed } => {
                if self.selected.as_deref() == Some(&conversation) { self.menu.followed.insert(root, followed); }
            }
            Update::ReactionPeople { conversation, message, result } => {
                if self.selected.as_deref() == Some(&conversation) && self.picker.people.as_ref().is_some_and(|(id, _)| id == &message) { self.picker.people = Some((message, result)); }
            }
            Update::CardAction { scope, result } => crate::card_state::apply(self, scope, result),
            Update::ScheduledList(result) => {
                self.schedule.loading = false;
                match result { Ok(rows) => self.schedule.rows = rows, Err(error) => self.schedule.error = Some(error) }
            }
            Update::FollowedPresence(people) => self.profile.followed = people,
            Update::OpenDirect(id) => self.actions.open_direct(&id),
            Update::NotificationReply(id) => { self.profile.chat_request = Some(id); self.profile.reply_focus = true; }
            Update::NotificationRead(id) => {
                if !self.profile.read_requests.contains(&id) { self.profile.read_requests.push(id); }
            }
            Update::ChannelDescription { id, result } => {
                self.channel_descriptions.insert(id.clone(), result.clone());
                crate::team_menu::description(self, &id, result);
            }
            Update::ChannelChanged { id, deleted, result } => crate::team_menu::changed(self, &id, deleted, result),
            Update::ScheduledChanged { id, result } => crate::schedule::changed(self, &id, result),
            Update::Scheduled { target, text, result } => {
                if self.schedule.pending.as_ref() == Some(&(target.clone(), text.clone())) {
                    self.schedule.pending = None;
                    match result {
                        Ok(()) => {
                            let mut cleared = false;
                            if active_target(self).as_ref() == Some(&target) && self.composer.text == text
                                && self.compose.gifs.is_empty() && self.compose.reply.is_none()
                                && self.compose.inline.is_empty() && self.compose.inline_loading.is_empty()
                                && self.compose.subject.text.is_empty() && self.compose.headline.text.is_empty()
                                && self.media.files(&target).is_empty() && !self.media.is_uploading(&target) {
                                self.composer.clear();
                                self.compose = Default::default();
                                cleared = true;
                            } else if self.drafts.get(&target) == Some(&text) {
                                self.drafts.remove(&target);
                                self.compose_drafts.remove(&target);
                                cleared = true;
                            }
                            if cleared && target.thread_root_id.is_none() { self.actions.save_draft(&target.conversation_id, ""); }
                            self.toast.show("Message scheduled");
                            self.schedule.open = false;
                        }
                        Err(error) => self.schedule.error = Some(error),
                    }
                }
            }
            Update::ComposeFailed { target, composed, files, error } => {
                self.apply(Update::SendFailed { target: target.clone(), text: composed.text.clone(), reply: composed.reply.clone(), files, error });
                let compose = if active_target(self).as_ref() == Some(&target) { &mut self.compose }
                    else { self.compose_drafts.entry(target).or_default() };
                compose.mentions.chosen.extend(composed.mentions);
                compose.gifs.splice(0..0, composed.gifs);
                compose.inline.extend(composed.inline);
                if compose.importance == store::MessageImportance::Normal { compose.importance = composed.importance; }
                if let Some(subject) = composed.subject { compose.subject = TextEdit::new(join_draft(&subject, &compose.subject.text)); }
                if let Some(banner) = composed.banner { compose.headline = TextEdit::new(join_draft(&banner.title, &compose.headline.text)); }
            }
            Update::Outbox(rows) => self.outbox = rows,
            Update::EditFinished { conversation_id, message_id, result } => {
                if self.selected.as_deref() == Some(&conversation_id) && self.menu.edit_id.as_deref() == Some(&message_id) {
                    self.menu.edit_saving = false;
                    match result {
                        Ok(()) => self.menu.edit_id = None,
                        Err(error) => self.menu.edit_error = Some(error),
                    }
                } else if let Err(error) = result { self.toast.show(format!("Could not edit message: {error}")); }
            }
            Update::Live(state) => self.toast.live(state),
            Update::LiveRetryReady => self.toast.retry_ready = true,
            Update::Synced(at) => self.toast.last_sync = Some(at),
            Update::Deleted { conversation_id, message_id } => { self.deleted_here.insert((conversation_id, message_id)); }
            Update::Restored { conversation_id, message_id } => { self.deleted_here.remove(&(conversation_id, message_id)); }
            Update::Forwarded { conversation_id, message_id, result } => {
                let active = self.selected.as_deref() == Some(&conversation_id) && self.menu.forward.as_deref() == Some(&message_id);
                if active { self.menu.forward_sending = false; }
                match result {
                    Ok(()) => {
                        if active { self.menu.selected.clear(); crate::message_menu::close(self); }
                        self.toast.show("Message forwarded");
                    }
                    Err(error) => {
                        if active { self.menu.forward_error = Some(error); }
                        else { self.toast.show(format!("Could not forward message: {error}")); }
                    }
                }
            }
            Update::FirstUnread { conversation_id, message_id } => {
                if self.selected.as_deref() == Some(&conversation_id) { self.first_unread = message_id; }
            }
            Update::Receipts { conversation_id, states } => {
                if self.selected.as_deref() == Some(&conversation_id) { self.receipts = states; }
            }
            Update::NotificationPause(until) => self.own.paused_until = until,
            Update::SwitcherSearch { session, query, result } => crate::switcher::apply(self, &session, &query, result),
            Update::Audio { url, result } => self.media.voice.loaded(url, result),
            Update::CallHistory(result) => {
                self.calls.history_loading = false;
                match result {
                    Ok(history) => { self.calls.history = history; self.calls.history_error = None; }
                    Err(error) => self.calls.history_error = Some(error),
                }
            }
            Update::AgentSettings(settings) => self.agent.settings = settings,
            Update::AgentResult(result) => self.agent.finish(result),
            Update::AgentProgress(step) => { self.agent.running = true; self.agent.step(step); }
            Update::AgentRuns(runs) => self.agent.runs.extend(runs),
            Update::Details(details) => {
                for (id, chat) in &details { self.member_counts.insert(id.clone(), chat.members.len()); }
                self.details = details;
            }
            Update::Pinned { conversation_id, pins } => {
                if self.selected.as_deref() == Some(&conversation_id) { self.pinned = pins; self.pin_index = 0; }
            }
            Update::Edited { conversation_id, ids } => {
                if self.selected.as_deref() == Some(&conversation_id) { self.menu.edited = ids.into_iter().collect(); }
            }
            Update::Typing(event) => {
                let key = (event.conversation_id.clone(), event.user_id.clone());
                if event.active { self.typing.insert(key, event); } else { self.typing.remove(&key); }
            }
            Update::People { query, rows } => crate::new_chat::apply_people(self, &query, rows),
            Update::ChatCreated(id) => {
                self.chatlist.manage.new_chat.creating = false;
                self.chatlist.manage.new_chat_open = false;
                self.chatlist.manage.pending_open = Some(id);
            }
            Update::ChatCreateFailed(error) => {
                self.chatlist.manage.new_chat.creating = false;
                self.chatlist.manage.new_chat.error = Some(error);
            }
            // Prefix match as in `toast::State::observe`: read-only sessions append " · Read-only".
            Update::Status(s) => {
                self.online_once |= s.starts_with("Online");
                self.toast.observe(&s);
                self.status = s;
            }
            Update::Me { user_id, name, email } => self.own.me = Some(crate::status_menu::Me { user_id, name, email }),
            Update::OwnStatus(status) => self.own.status = status,
            Update::Profile { user_id, result } => self.profile.loaded(user_id, result),
            Update::OpenChat(id) => self.profile.chat_request = Some(id),
            Update::Chats(rows) => {
                self.chats = rows;
                self.starting = false;
                // Chats after a sign-in the picker asked for mean the session is back.
                if self.account_picker.as_ref().is_some_and(|picker| picker.signing_in) { self.account_picker = None; }
            }
            Update::SessionExpired => {
                if self.online_once { self.toast.session_expired = true } else { crate::account_picker::open_picker(self, true) }
            }
            Update::SignInFailed(error) => {
                if let Some(picker) = &mut self.account_picker { picker.signing_in = false; }
                self.toast.show(error);
            }
            Update::Folders(folders) => self.chatlist.folders = folders,
            Update::Favorites(ids) => self.chatlist.favorites = ids,
            Update::ChatPaging { more, loading } => { self.chatlist.has_older = more; self.chatlist.loading_older = loading; }
            Update::Saved(result) => self.chatlist.saved = Some(result),
            Update::Teams(rows) => self.teams = rows,
            Update::TeamMetadata { teams, channels, pinned } => {
                self.chatlist.team_details = teams;
                self.chatlist.channel_details = channels;
                self.chatlist.pinned_channels = pinned;
            }
            Update::CopyLink(result) => match result {
                Ok(link) => self.chatlist.copy_link = Some(link),
                Err(error) => self.toast.show(error),
            },
            Update::Appearance(settings) => self.appearance = settings,
            Update::Mentioned(id) => { self.tray.mentions.insert(id); }
            Update::Preferences(prefs) => {
                if self.settings.prefs.translation.as_ref().map(|settings| &settings.target_locale) != prefs.translation.as_ref().map(|settings| &settings.target_locale) {
                    let target = prefs.translation.as_ref().map(|settings| teams_core::default_target(settings.target_locale.as_deref(), settings.display_locale.as_deref(), &prefs.languages)).unwrap_or_else(|| "en".into());
                    self.translation.target_changed(&target);
                }
                self.calls.selected_camera = prefs.camera.clone();
                self.calls.selected_input = prefs.microphone.clone();
                self.calls.selected_output = prefs.speaker.clone();
                self.settings.prefs = *prefs;
            }
            Update::Activity(snapshot) => self.screens.activity = snapshot,
            Update::Call(update) => {
                if self.call_window.is_some() {
                    match &update {
                        calling::CallUpdate::Notice(message)
                        | calling::CallUpdate::State(calling::CallState::Ended {
                            reason: calling::EndReason::Failed(message),
                        }) => self.toast.show(message.clone()),
                        _ => {}
                    }
                }
                if self.call_window.is_none() && matches!(&update, calling::CallUpdate::State(calling::CallState::Connecting)) { self.screens.screen = crate::screens::Screen::Calls; }
                if let calling::CallUpdate::Roster(roster) = &update {
                    let ids = roster.iter()
                        .map(|person| person.mri.strip_prefix("8:orgid:").unwrap_or(&person.mri).to_owned())
                        .collect();
                    self.actions.warm_avatars(ids);
                }
                self.calls.apply(update);
            }
            Update::CallIntro(intro) => self.calls.start(intro),
            Update::RecordingRights { policy, own_tenant } => self.calls.recording_rights(policy, own_tenant),
            Update::Ring(event) => {
                if self.call_window.is_none() && matches!(&event, calling::EngineEvent::Incoming(_)) { self.screens.screen = crate::screens::Screen::Calls; }
                self.calls.apply_ring(event);
            }
            Update::VideoReady => self.calls.apply_video(self.actions.take_video()),
            Update::Image { key, bytes } => self.media.loaded(key, bytes),
            Update::ImagePreview { key, bytes, preview } => self.media.loaded_preview(key, bytes, preview),
            Update::ImageFailed { key, error } => { self.media.failed(key, error); }
            Update::Attachment { target, path, result } => {
                if let Some(orphan) = self.media.uploaded(target, path, result) { self.actions.discard_attachment(orphan); }
            }
            Update::Calendar { week, rows } => {
                if self.screens.week == week { self.screens.calendar = rows; }
            }
            Update::CalendarRange { start, end, rows } => {
                if self.screens.week == start && start + chrono::Duration::days(self.screens.calendar_view.days() as i64) == end { self.screens.calendar = rows; }
            }
            Update::MeetingReady(result) => match result {
                Ok(url) => {
                    self.calls.link = TextEdit::new(url);
                    self.calls.prejoin_open = true;
                    self.screens.screen = crate::screens::Screen::Calls;
                }
                Err(error) => self.toast.show(format!("Could not create meeting: {error}")),
            },
            Update::CalendarRefresh if self.screens.screen == crate::screens::Screen::Calendar => crate::screens::reload_calendar(self),
            Update::CalendarRefresh => {}
            Update::Agenda(events) => self.screens.agenda = events,
            Update::SectionSettings(settings) => self.section_settings = settings,
            Update::OutlookReminders { start, end, result } => self.reminders.outlook_mut().apply(start, end, result),
            Update::LiveMeeting { thread_id, meeting: Some(meeting) } => { self.live_meetings.insert(thread_id, meeting); }
            Update::LiveMeeting { thread_id, meeting: None } => { self.live_meetings.remove(&thread_id); }
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
                    // Images load as their message scrolls into view (`media::watch`), not all at once.
                    let rows = crate::teams::group_replies(rows);
                    self.appearing = arrived(&self.messages, &rows);
                    self.messages = rows;
                    if self.launch_reply_last.as_deref() == Some(&conversation_id) { self.launch_reply_last = None; crate::arguments::reply_last(self); }
                }
            }
            Update::Gifs { query, result } => self.compose.gif.loaded(query, result),
            Update::CloudFiles { place, result } => self.compose.cloud.loaded(place, result),
            Update::CloudAttached { target, name, result } => self.media.cloud_attached(&target, &name, result),
            Update::SendFailed {
                target,
                text,
                reply,
                files,
                error,
            } => {
                self.media.restore_files(target.clone(), files);
                let error = format!("Send could not be confirmed: {error}");
                if active_target(self).as_ref() == Some(&target) {
                    if self.compose.reply.is_none() { self.compose.reply = reply; }
                    let current = &self.composer.text;
                    self.composer = TextEdit::new(join_draft(&text, current));
                    self.send_error = Some(error);
                } else {
                    if let Some(reply) = reply { self.reply_drafts.entry(target.clone()).or_insert(reply); }
                    let current = self.drafts.remove(&target).unwrap_or_default();
                    self.drafts
                        .insert(target.clone(), join_draft(&text, &current));
                    self.send_errors.insert(target, error);
                }
            }
        }
    }

    pub(crate) fn chat_draft(&self, id: &str) -> Option<String> {
        let text = if self.selected.as_deref() == Some(id) && self.thread_root_id.is_none() {
            self.composer.text.clone()
        } else {
            let target = SendTarget { conversation_id: id.to_owned(), thread_root_id: None, channel: self.channel(id).is_some() };
            self.drafts.get(&target).cloned().or_else(|| self.actions.load_draft(id)).unwrap_or_default()
        };
        (!text.trim().is_empty()).then_some(text)
    }

    pub(crate) fn chat_send_failed(&self, id: &str) -> bool {
        (self.selected.as_deref() == Some(id) && self.thread_root_id.is_none() && self.send_error.is_some())
            || self.send_errors.keys().any(|target| target.conversation_id == id && target.thread_root_id.is_none())
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
    s.launch_reply_last = None;
    s.chatlist.quick_view = None;
    save_draft(s);
    s.selected = Some(id.to_owned());
    s.shared_open = false;
    s.details_open = false;
    s.library = Default::default();
    s.rename_open = false;
    s.pinned.clear();
    s.pin_index = 0;
    s.thread_root_id = None;
    s.expanded_posts.clear();
    s.picker.close();
    s.confirm_delete = None;
    s.more_open = None;
    s.menu = Default::default();
    s.translation.dialog = None;
    s.messages.clear();
    s.receipts.clear();
    s.first_unread = None;
    s.media.voice.pause();
    s.media.video.pause();
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
    let channel = s.channel(&conversation_id).is_some();
    Some(SendTarget { conversation_id, thread_root_id: s.thread_root_id.clone(), channel })
}

pub(crate) fn save_draft<A: Actions>(s: &mut UiState<A>) {
    crate::view_composer::persist_draft(s);
    crate::view_composer::stop_typing(s);
    if let Some(target) = active_target(s) {
        if let Some(reply) = &s.compose.reply { s.reply_drafts.insert(target.clone(), reply.clone()); }
        else { s.reply_drafts.remove(&target); }
        s.drafts.insert(target.clone(), s.composer.text.clone());
        s.compose_drafts.insert(target.clone(), std::mem::take(&mut s.compose));
        if let Some(error) = s.send_error.take() { s.send_errors.insert(target, error); }
    }
}

pub(crate) fn forget_cached_draft<A: Actions>(s: &mut UiState<A>) {
    if let Some(target) = active_target(s) {
        s.drafts.remove(&target);
        s.compose_drafts.remove(&target);
        s.reply_drafts.remove(&target);
        s.media.clear(&target);
    }
}

pub(crate) fn restore_draft<A: Actions>(s: &mut UiState<A>) {
    if let Some(target) = active_target(s) {
        if !s.drafts.contains_key(&target) && target.thread_root_id.is_none()
            && let Some((stored, files)) = s.actions.load_composition(&target.conversation_id) {
                let mut compose = crate::view_composer::Compose::default();
                compose.mentions.chosen = stored.mentions;
                compose.reply = stored.reply.clone();
                compose.gifs = stored.gifs;
                compose.inline = stored.inline;
                compose.importance = stored.importance;
                compose.subject = TextEdit::new(stored.subject.unwrap_or_default());
                compose.headline = TextEdit::new(stored.banner.map(|banner| banner.title).unwrap_or_default());
                for (marker, image) in &compose.inline { s.media.loaded(marker.clone(), image.image.bytes.to_vec()); }
                if let Some(reply) = stored.reply { s.reply_drafts.insert(target.clone(), reply); }
                s.drafts.insert(target.clone(), stored.text);
                s.compose_drafts.insert(target.clone(), compose);
                s.media.restore_files(target.clone(), files);
            }
        let stored = if target.thread_root_id.is_none() { s.actions.load_draft(&target.conversation_id) } else { None };
        s.composer = TextEdit::new(s.drafts.remove(&target).or(stored).unwrap_or_default());
        s.compose = s.compose_drafts.remove(&target).unwrap_or_default();
        s.compose.reply = s.reply_drafts.remove(&target);
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
    if s.schedule.pending.as_ref().is_some_and(|(pending, _)| pending == &target) { return; }
    if s.compose.inline_loading.iter().any(|marker| s.composer.text.contains(marker)) { return; }
    let text = s.composer.text.trim().to_owned();
    let files = s.media.files(&target);
    if s.media.is_uploading(&target) || (text.is_empty() && files.is_empty() && s.compose.gifs.is_empty()) {
        return;
    }
    s.send_error = None;
    crate::view_composer::stop_typing(s);
    s.media.clear(&target);
    let composed = s.compose.take(text);
    s.actions.send_composed(target, composed, files);
    s.composer.clear();
}

/// Lays `root` out at 1/zoom of the window and scales it back up, so everything grows by `zoom`.
/// Rune has no percentage heights, so the window size comes from last frame's `zoom-host`.
fn zoomed<A: Actions + 'static>(cx: &mut Cx<UiState<A>>, zoom: f32, root: El) -> El {
    if (zoom - 1.0).abs() < f32::EPSILON {
        return root.w_full().h_full();
    }
    let Some([w, h]) = cx.layout_size("zoom-host") else {
        return col().key("zoom-host").w_full().h_full().child(root.w_full().h_full());
    };
    col().key("zoom-host").w_full().h_full()
        .child(root.w(w / zoom).h(h / zoom).origin(0.0, 0.0).scale(zoom))
}

pub fn view<A: Actions + 'static>(s: &UiState<A>, cx: &mut Cx<UiState<A>>) -> El {
    s.media.begin_render();
    crate::theme::apply(cx, &s.appearance);
    let zoom = s.appearance.zoom;
    cx.titlebar_height(crate::view_chrome::TITLE_BAR_HEIGHT * zoom);
    let width = cx.layout_size("root").map_or(1100.0, |size| size[0]);
    let arriving = !s.appearing.is_empty() && !cx.is_reduced_motion();
    if s.account_picker.is_some() {
        return zoomed(cx, zoom, col().key("account-picker-host").w_full().h_full().fill(c(WHITE))
            .child(crate::view_chrome::title_bar(s, cx.chrome_insets().trailing / zoom))
            .child(crate::account_picker::view(s))
            .children(crate::toast::layer(s)));
    }
    if s.starting {
        return zoomed(cx, zoom, col().key("startup-splash").w_full().h_full().fill(c(WHITE))
            .child(crate::view_chrome::title_bar(s, cx.chrome_insets().trailing / zoom))
            .child(crate::splash::view(s)));
    }
    if s.toast.reconnecting {
        return zoomed(cx, zoom, col().key("root").fill(c(WHITE))
            .child(crate::view_chrome::title_bar(s, cx.chrome_insets().trailing / zoom))
            .child(crate::toast::reconnect(s)));
    }
    // The pre-join screen owns the whole window: a leftover reminder or banner from the main
    // window would otherwise be painted over it at the top edge.
    let prejoin = s.calls.prejoin_open && s.screens.screen == crate::screens::Screen::Calls;
    let root = col().key("root");
    // The window's content fades up once, as it replaces the splash.
    let root = if s.from_splash { root.sequence(Sequence::new(crate::splash::REVEAL)) } else { root };
    let root = crate::view_messages::snappy(root, arriving)
        // Snappy UI: no mount/unmount fades, layout tweens or colour springs anywhere below,
        // except the commit where messages arrive live (`view_messages::snappy`).
        .motion(Spring::NONE)
        .fill(c(WHITE))
        .child(crate::view_chrome::title_bar(s, cx.chrome_insets().trailing / zoom))
        .children(if prejoin { None } else { crate::toast::banner(s) })
        .children(if prejoin { None } else { crate::toast::session_expired(s) })
        .child(
            row()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .children((!s.detached_chat).then(|| crate::view_chrome::rail(s)))
                .child(if matches!(s.screens.screen, crate::screens::Screen::Chat | crate::screens::Screen::Teams | crate::screens::Screen::Activity) {
                    let sidebar = if s.detached_chat { el().size(0.0, 0.0) } else if matches!(s.screens.screen, crate::screens::Screen::Teams | crate::screens::Screen::Activity) {
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
        .children(crate::view_chrome::more_menu(s))
        .children(crate::view_chatlist::more_menu(s))
        .children(crate::team_menu::layer(s))
        .children(if prejoin { Vec::new() } else { crate::screens::overlays(s) })
        .children(crate::view_header::meet_options(s))
        .children(crate::image_viewer::overlay(s))
        .child(crate::chat_menu::overlay(s))
        .children(crate::message_menu::overlay(s))
        .children(crate::reactions::people(s))
        .children(crate::card_state::dialog(s))
        .children(crate::translation::dialog(s))
        .children(crate::meeting_form::dialog(s))
        .children(crate::shortcuts::overlays(s));
    zoomed(cx, zoom, root)
}

fn conversation<A: Actions + 'static>(s: &UiState<A>, compact: bool, arriving: bool) -> El {
    if s.screens.screen == crate::screens::Screen::Chat && let Some(view) = s.chatlist.quick_view {
        return crate::quick_views::view(s, view);
    }
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
        .child(row().w_full().grow(1.0).min_h(0.0)
            .child(if s.shared_open { crate::view_messages::shared(s) } else { crate::view_messages::message_list(s, arriving) })
            .children(s.details_open.then(|| crate::view_header::details(s, id))))
        .child(crate::find::jump_bar(s))
        .children((!s.menu.selected.is_empty()).then(|| row().w_full().gap(8.0).padding(8.0)
            .child(text(format!("{} messages selected", s.menu.selected.len())).font(ts(13.0, 600.0)))
            .child(crate::screens::button("Forward selected", "forward-selected", |s: &mut UiState<A>, tx| {
                if let Some(id) = s.messages.iter().find(|message| s.menu.selected.contains(&message.id)).map(|message| message.id.clone()) {
                    s.menu.forward = Some(id); tx.focus("forward-search", false);
                }
            }))
            .child(crate::screens::button("Clear", "forward-selection-clear", |s: &mut UiState<A>, _| s.menu.selected.clear()))))
        .child(crate::outbox::view(s))
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
        calling_offline: std::cell::Cell<bool>,
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
        fn undo_delete_message(&self, conversation_id: &str, message_id: &str) {
            self.calls.borrow_mut().push(format!("undo {conversation_id} {message_id}"));
        }
        fn retry_live(&self) { self.calls.borrow_mut().push("retry live".into()); }
        fn calling_ready(&self) -> bool { !self.calling_offline.get() }
        fn join_call(&self, url: &str, _: bool) { self.calls.borrow_mut().push(format!("join {url}")); }
        fn load_calendar(&self, _: chrono::NaiveDate) { self.calls.borrow_mut().push("calendar".into()); }
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
            forwarded: false, importance: Default::default(),
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
    fn a_join_command_waits_until_calling_connects() {
        let mut rt = rt();
        rt.state.actions.calling_offline.set(true);
        let join = crate::arguments::Command::Join("https://teams.microsoft.com/l/meetup-join/x".into());
        rt.update(|s, tx| join.run(s, tx));
        assert!(calls(&rt).is_empty());
        rt.state.actions.calling_offline.set(false);
        rt.update(|s, tx| { if let Some(command) = s.launch_command.take() { command.run(s, tx); } });
        assert_eq!(calls(&rt), ["join https://teams.microsoft.com/l/meetup-join/x"]);
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
    fn live_failure_offers_retry_without_hiding_chat_and_calendar_updates_apply() {
        let mut rt = rt();
        apply(&mut rt, Update::Status("Online".into()));
        apply(&mut rt, Update::LiveRetryReady);
        let synced = chrono::Utc::now();
        apply(&mut rt, Update::Synced(synced));
        apply(&mut rt, Update::Live(app_backend::backend::LiveState::Failed));
        rt.update(|s, _| { s.toast.tick(std::time::Instant::now() + std::time::Duration::from_secs(5)); });
        rt.frame(1.0 / 60.0);
        assert!(rt.find("connection-banner").is_some());
        assert!(rt.find("reconnecting").is_none());
        assert_eq!(rt.state.toast.last_sync, Some(synced));
        click(&mut rt, "live-retry");
        assert_eq!(calls(&rt), ["retry live"]);
        apply(&mut rt, Update::Status("Could not load a photo".into()));
        assert!(rt.find("live-retry").is_some());
        apply(&mut rt, Update::Live(app_backend::backend::LiveState::Live));
        assert!(rt.find("connection-banner").is_none());
        apply(&mut rt, Update::CalendarRefresh);
        assert_eq!(calls(&rt), ["retry live"], "a closed calendar does not reload");
        rt.update(|s, _| s.screens.screen = crate::screens::Screen::Calendar);
        apply(&mut rt, Update::CalendarRefresh);
        apply(&mut rt, Update::Agenda(vec![]));
        assert_eq!(calls(&rt), ["retry live", "calendar"]);
        assert!(rt.state.screens.agenda.is_empty());
    }

    #[test]
    fn undo_only_appears_after_successful_deletion_in_this_session() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("a", "Ada"), chat("b", "Bo")]));
        click(&mut rt, "chat-a");
        let mut own = msg("own", "Me", "deleted", true);
        own.deleted = true;
        let mut other = msg("other", "Ada", "deleted", false);
        other.deleted = true;
        apply(&mut rt, Update::Messages { conversation_id: "a".into(), rows: vec![own.clone(), other] });
        assert!(rt.find("undo-delete-own").is_none());
        apply(&mut rt, Update::Deleted { conversation_id: "a".into(), message_id: "own".into() });
        assert!(rt.find("undo-delete-own").is_some());
        apply(&mut rt, Update::Deleted { conversation_id: "a".into(), message_id: "other".into() });
        assert!(rt.find("undo-delete-other").is_none());
        click(&mut rt, "undo-delete-own");
        assert_eq!(calls(&rt).last().unwrap(), "undo a own");
        click(&mut rt, "chat-b");
        apply(&mut rt, Update::Messages { conversation_id: "b".into(), rows: vec![own] });
        assert!(rt.find("undo-delete-own").is_none());
        click(&mut rt, "chat-a");
        apply(&mut rt, Update::Messages { conversation_id: "a".into(), rows: vec![{
            let mut m = msg("own", "Me", "deleted", true); m.deleted = true; m
        }] });
        assert!(rt.find("undo-delete-own").is_some());
        apply(&mut rt, Update::Restored { conversation_id: "a".into(), message_id: "own".into() });
        assert!(rt.find("undo-delete-own").is_none());
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
        assert_eq!(rt.state.selected.as_deref(), Some("a"));
        assert_eq!(rt.state.composer.text, "Keep this draft");
        click(&mut rt, "chat-a");
        assert_eq!(rt.state.composer.text, "Keep this draft");
    }

    #[test]
    fn conversation_metadata_ignores_late_pins_and_tracks_typing_stop() {
        let mut state = UiState::new(Fake::default());
        state.selected = Some("b".into());
        state.apply(Update::Pinned { conversation_id: "a".into(), pins: vec![crate::types::PinnedRow {
            message_id: "pin".into(),
            conversation_id: "a".into(), sender: "Ada".into(), text: "Private".into(),
        }] });
        assert!(state.pinned.is_empty());
        state.apply(Update::Details(HashMap::from([("b".into(), crate::types::ChatDetails {
            avatar_id: None, group: vec![],
            members: vec![store::MemberRecord { user_id: Some("ada".into()), display_name: "Ada".into() }],
            muted: true, presence: None, importance: Default::default(), desktop_notifications: Default::default(),
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
            Update::SendFailed { reply: None,
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
            Update::SendFailed { reply: None,
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
            Update::SendFailed { reply: None,
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
    fn a_storage_failure_restores_the_full_composition_in_its_own_chat() {
        let mut rt = rt();
        apply(&mut rt, Update::Chats(vec![chat("a", "Ada"), chat("b", "Bo")]));
        click(&mut rt, "chat-b");
        apply(&mut rt, Update::ComposeFailed {
            target: SendTarget { conversation_id: "a".into(), thread_root_id: None, channel: false },
            composed: crate::types::Composed {
                inline: vec![],
                text: "hello @Ada".into(),
                importance: store::MessageImportance::Urgent,
                subject: Some("Subject".into()),
                banner: Some(teams_core::AnnouncementBanner { title: "Headline".into(), background: teams_core::BannerBackground::Color("indigo".into()) }),
                mentions: vec![teams_core::MentionInput::user("ada", "Ada")],
                gifs: vec![chatsvc::Gif { url: "https://x.test/a.gif".into(), title: "wave".into(), preview_url: String::new(), width: 20, height: 20, preview_width: 20, preview_height: 20 }],
                reply: None,
            },
            files: vec![], error: "disk full".into(),
        });
        assert!(rt.state.compose.gifs.is_empty());
        click(&mut rt, "chat-a");
        assert_eq!(rt.state.compose.gifs.len(), 1);
        assert_eq!(rt.state.compose.mentions.chosen.len(), 1);
        assert_eq!(rt.state.compose.importance, store::MessageImportance::Urgent);
        assert_eq!(rt.state.compose.subject.text, "Subject");
        assert_eq!(rt.state.compose.headline.text, "Headline");
        assert_eq!(rt.state.composer.text, "hello @Ada");
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
        apply(&mut rt, Update::SendFailed { reply: None, target: SendTarget { conversation_id: "channel".into(), thread_root_id: Some("root".into()), channel: true }, text: "reply".into(), files: vec![], error: "offline".into() });
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
