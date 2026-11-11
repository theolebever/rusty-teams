//! Backend bridge for the Rune view. Starts the Teams backend on the shared tokio runtime and
//! turns its events into `Update`s that the view drains once per frame.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Local, TimeZone, Utc};
use scraper::{Html, Node};
use app_backend::backend::{BackendEvent, ConnectionState, Engine};
use store::{ChatRecord, MessageRecord, SidebarTeam, Store};
use teams_core::CoreEvent;
use tokio::sync::{mpsc as tokio_mpsc, watch};

use crate::types::{Actions, ChannelRow, ChatRow, MessageRow, SendTarget, TeamRow, Update};

const DATABASE_FILE: &str = "cache.sqlite3";
const WEBVIEW_FOLDER: &str = "WebView2";
const WINDOW_TITLE: &str = "Rusty Teams - Sign in";
const STORE_OPEN_ATTEMPTS: u32 = 20;
const STORE_OPEN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
// teams-core keeps this key private; same value as its META_USER_ID.
const META_USER_ID: &str = "me_user_id";
const DELETED_TEXT: &str = "This message was deleted";
const UNKNOWN_SENDER: &str = "Unknown";

pub struct Bridge {
    shared: Arc<Shared>,
    updates: Receiver<Update>,
    // Kept alive so the backend's activity watch stays open. The view always counts as active.
    _activity: watch::Sender<bool>,
}

/// State shared by the view-facing calls and the event forwarder.
pub(crate) struct Shared {
    open_threads: Mutex<std::collections::HashSet<String>>,
    /// Ids whose photo was looked up and fetched this session. `AvatarsChanged` publishes later ones.
    avatars_requested: Arc<Mutex<std::collections::HashSet<String>>>,
    /// Chats whose latest page was fetched ahead of opening them, once per session.
    warmed: Mutex<std::collections::HashSet<String>>,
    pub(crate) read_only: bool,
    calls: Mutex<Option<Arc<app_backend::launcher::CallLauncher>>>,
    call_control: crate::calls_backend::Calls,
    message_limit: Mutex<usize>,
    foreground: Mutex<(Option<String>, bool)>,
    engine: Mutex<Option<Arc<Engine>>>,
    open: Mutex<Option<String>>,
    updates: Sender<Update>,
    wake: Box<dyn Fn() + Send + Sync>,
    agent_stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl Shared {
    pub(crate) fn publish(&self, update: Update) {
        if let Update::Status(status) = &update { app_backend::diagnostics::record(format!("Rune: {status}")); }
        let update = match update {
            Update::Status(status) if self.read_only => Update::Status(format!("{status} · Read-only")),
            update => update,
        };
        let _ = self.updates.send(update);
        (self.wake)();
    }

    pub(crate) fn engine(&self) -> Option<Arc<Engine>> {
        self.engine
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn is_open(&self, conversation_id: &str) -> bool {
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_deref()
            == Some(conversation_id)
    }

    /// Publishes the chat list. False when the sidebar could not be read (the error is the status).
    fn publish_chats(&self, engine: &Engine) -> bool {
        match engine.sidebar() {
            Ok(sidebar) => {
                let me = engine.store().meta(META_USER_ID).ok().flatten().unwrap_or_default();
                let pictures = sidebar.chats.iter().filter(|chat| chat.kind != "oneOnOne").map(|chat| teams_core::chat_picture_key(&chat.id));
                self.publish_avatars(engine, sidebar.chats.iter().flat_map(|chat| &chat.members).filter_map(|member| member.user_id.clone()).chain(pictures).collect());
                self.warm_recent(&sidebar.chats);
                let details = sidebar.chats.iter().map(|chat| {
                    let one_on_one = chat.kind == "oneOnOne";
                    let presence = one_on_one.then(|| chat.members.iter()
                        .filter_map(|member| member.user_id.as_deref()).find(|id| *id != me)
                        .and_then(|id| engine.presence(id))).flatten();
                    let avatar_id = if one_on_one {
                        chat.members.iter().filter_map(|m| m.user_id.as_ref()).find(|id| **id != me).cloned()
                    } else {
                        Some(teams_core::chat_picture_key(&chat.id))
                    };
                    let group = if one_on_one { vec![] } else {
                        chat.members.iter().filter(|m| !m.is_bot() && m.user_id.as_deref() != Some(&me)).take(2).cloned().collect()
                    };
                    (chat.id.clone(), crate::types::ChatDetails { avatar_id, group, members: chat.members.clone(), muted: chat.muted, presence })
                }).collect();
                self.publish(Update::Details(details));
                self.publish(Update::Chats(chat_rows(sidebar.chats, Local::now())));
                self.publish(Update::Teams(team_rows(sidebar.teams)));
                true
            }
            Err(error) => {
                self.publish(Update::Status(format!("Chats unavailable: {error}")));
                false
            }
        }
    }

    fn publish_messages(&self, engine: &Engine, conversation_id: &str) {
        let limit = *self.message_limit.lock().unwrap_or_else(PoisonError::into_inner);
        match engine.store().messages(conversation_id, None, limit) {
            Ok(mut records) => {
                for root in self.open_threads.lock().unwrap_or_else(PoisonError::into_inner).iter() {
                    if let Ok(thread) = engine.store().thread_messages(conversation_id, root) { records.extend(thread); }
                }
                records.sort_by(|a, b| (a.created_at, &a.message_id).cmp(&(b.created_at, &b.message_id)));
                records.dedup_by(|a, b| a.message_id == b.message_id);
                let channel = engine.store().channel(conversation_id).ok().flatten().is_some();
                let records: Vec<_> = records.into_iter().filter(|record| !channel || channel_post(record)).collect();
                let me = engine
                    .store()
                    .meta(META_USER_ID)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let rows = message_rows(&records, &me, Local::now());
                let edited = records.iter().filter(|record| record.edited_at.is_some() && !record.deleted).map(|record| record.message_id.clone()).collect();
                self.publish_avatars(engine, records.iter().filter_map(|record| record.sender_id.clone()).collect());
                self.publish(Update::Messages {
                    conversation_id: conversation_id.to_owned(),
                    rows,
                });
                self.publish(Update::Edited { conversation_id: conversation_id.to_owned(), ids: edited });
            }
            Err(error) => self.publish(Update::Status(format!("Messages unavailable: {error}"))),
        }
    }

    /// Fetches the latest page of the newest chats with nothing cached, one after another, so
    /// they open with their messages instead of an empty list.
    fn warm_recent(&self, chats: &[ChatRecord]) {
        let ids = {
            let mut warmed = self.warmed.lock().unwrap_or_else(PoisonError::into_inner);
            recent_chat_ids(chats, WARM_CHATS).into_iter().filter(|id| warmed.insert(id.clone())).collect::<Vec<_>>()
        };
        let Some(engine) = self.engine().filter(|_| !ids.is_empty()) else { return };
        app_backend::runtime::handle().spawn(async move {
            for id in ids {
                if engine.store().messages(&id, None, 1).is_ok_and(|cached| cached.is_empty()) {
                    let _ = engine.fetch_newer(&id).await;
                }
            }
        });
    }

    /// Each id once per session: the sidebar reloads on every presence change, and reading and
    /// fetching every member's photo again took up to a quarter of a second each time.
    /// ponytail: a photo missing at startup is fetched again only on the next launch.
    fn publish_avatars(&self, engine: &Engine, mut ids: Vec<String>) {
        {
            let mut requested = self.avatars_requested.lock().unwrap_or_else(PoisonError::into_inner);
            ids.retain(|id| requested.insert(id.clone()));
        }
        if ids.is_empty() { return; }
        for id in &ids {
            if let Some(photo) = engine.avatar(id) {
                self.publish(Update::Image { key: format!("avatar:{id}"), bytes: photo.bytes });
            }
        }
        let requested = self.avatars_requested.clone();
        if let Some(engine) = self.engine() {
            app_backend::runtime::handle().spawn(async move {
                let mut pending = tokio::task::JoinSet::new();
                for chunk in ids.chunks(20) {
                    let engine = engine.clone();
                    let chunk = chunk.to_vec();
                    let requested = requested.clone();
                    pending.spawn(async move {
                        if let Err(error) = engine.fetch_avatars(&chunk).await {
                            app_backend::diagnostics::record(format!("Rune: Profile photos unavailable: {error}"));
                            // Asked again at the next reload.
                            let mut requested = requested.lock().unwrap_or_else(PoisonError::into_inner);
                            for id in &chunk { requested.remove(id); }
                        }
                    });
                    if pending.len() == 3 && let Some(Err(error)) = pending.join_next().await {
                        app_backend::diagnostics::record(format!("Rune: Profile photo task failed: {error}"));
                    }
                }
                while let Some(result) = pending.join_next().await {
                    if let Err(error) = result { app_backend::diagnostics::record(format!("Rune: Profile photo task failed: {error}")); }
                }
            });
        }
    }
}

impl Bridge {
    pub fn shutdown(&self) { self.stop_agent(); self.shared.call_control.hangup(); }
    /// Starts the backend. `wake` is called (from any thread) after every new `Update` so the UI redraws.
    pub fn start(
        database: Option<PathBuf>,
        no_sso: bool,
        read_only: bool,
        endpoint: Option<String>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Bridge {
        let (updates, receiver) = mpsc::channel();
        let shared = Arc::new_cyclic(|weak: &std::sync::Weak<Shared>| {
            let weak = weak.clone();
            let video_weak = weak.clone();
            Shared {
            open_threads: Mutex::new(std::collections::HashSet::new()),
            avatars_requested: Arc::default(),
            warmed: Mutex::default(),
            read_only,
            agent_stop: Mutex::new(None),
            calls: Mutex::new(None),
            call_control: crate::calls_backend::Calls::new(read_only, move |update| {
                if let Some(shared) = weak.upgrade() { shared.publish(Update::Call(update)); }
            }, move || { if let Some(shared) = video_weak.upgrade() { shared.publish(Update::VideoReady); } }),
            message_limit: Mutex::new(50),
            foreground: Mutex::new((None, false)),
            engine: Mutex::new(None),
            open: Mutex::new(None),
            updates,
            wake: Box::new(wake),
        }});
        let database = database.unwrap_or_else(|| data_path(DATABASE_FILE));
        let store = Arc::new(open_store(&database));
        let transport = endpoint.map_or_else(|| transport(&database, no_sso), |endpoint| Arc::new(session::CdpTransport::new(&endpoint)) as Arc<dyn session::Transport>);
        let (activity_sender, activity) = watch::channel(false);
        let events = app_backend::backend::start(store, transport, !read_only, activity);
        app_backend::runtime::handle().spawn(forward(shared.clone(), events));
        Bridge {
            shared,
            updates: receiver,
            _activity: activity_sender,
        }
    }

    /// Drains pending updates without blocking. Called by the UI each frame.
    pub fn poll(&mut self) -> Vec<Update> {
        self.updates.try_iter().collect()
    }

    /// Runs one engine call for a chat, then republishes the list; a failure shows in the status line.
    fn chat_task<F, Fut>(&self, id: &str, what: &'static str, call: F)
    where
        F: FnOnce(Arc<Engine>, String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = teams_core::Result<()>> + Send + 'static,
    {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let (shared, id) = (self.shared.clone(), id.to_owned());
        app_backend::runtime::handle().spawn(async move {
            match call(engine.clone(), id).await {
                Ok(()) => { shared.publish_chats(&engine); }
                Err(error) => shared.publish(Update::Status(format!("Could not {what}: {error}"))),
            }
        });
    }

    pub fn set_activity(&self, active: bool) {
        self._activity.send_if_modified(|value| { if *value == active { false } else { *value = active; true } });
    }

    pub fn set_foreground(&self, chat: Option<&str>, in_call: bool) {
        *self.shared.foreground.lock().unwrap_or_else(PoisonError::into_inner) = (chat.map(str::to_owned), in_call);
    }
}

impl Actions for Bridge {
    fn load_audio(&self, url: &str) {
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        let url = url.to_owned();
        app_backend::runtime::handle().spawn(async move {
            use teams_core::Remote;
            let result = engine.remote().hosted_content(&url).await
                .map_err(|error| error.to_string()).and_then(|media| {
                    if media.bytes.len() > 32 * 1024 * 1024 { Err("Audio message exceeds 32 MiB".into()) }
                    else { Ok(media.bytes) }
                });
            shared.publish(Update::Audio { url, result });
        });
    }
    fn call_back(&self, entry: &chatsvc::CallLogEntry) {
        if self.shared.read_only || entry.is_meeting { return; }
        let (Some(engine), Some(peer)) = (self.shared.engine(), entry.peer_id.clone()) else { return };
        let shared = self.shared.clone();
        let name = entry.peer_name.clone().unwrap_or_else(|| peer.clone());
        let thread = entry.thread_id.clone();
        app_backend::runtime::handle().spawn(async move {
            let user = peer.strip_prefix("8:orgid:").unwrap_or(&peer);
            let thread = match thread.filter(|id| !id.is_empty()) {
                Some(id) => id,
                None => match engine.create_one_on_one(user).await {
                    Ok(id) => id,
                    Err(error) => { shared.publish(Update::CallHistory(Err(error.to_string()))); return; }
                },
            };
            shared.call_control.start(calling::CallSpec::People {
                callees: vec![calling::Callee { mri: format!("8:orgid:{user}"), display_name: name }], thread_id: thread,
            });
        });
    }
    fn load_call_history(&self) {
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::CallHistory(Err("Calls are not connected yet".into())));
            return;
        };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            shared.publish(Update::CallHistory(engine.list_call_logs().await.map_err(|error| error.to_string())));
        });
    }
    fn stop_agent(&self) {
        if let Some(stop) = self.shared.agent_stop.lock().unwrap_or_else(PoisonError::into_inner).take() { let _ = stop.send(()); }
    }

    fn run_agent(&self, id: &str, prompt: &str) {
        if self.shared.read_only || prompt.trim().is_empty() { return; }
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::AgentResult("Not connected yet".into()));
            return;
        };
        let settings = engine.store().meta(agents::SETTINGS_KEY).ok().flatten()
            .and_then(|json| agents::AgentSettings::from_json(&json).ok()).unwrap_or_default();
        let Some(profile) = settings.agents.iter().find(|agent| agent.id == id).cloned() else {
            self.shared.publish(Update::AgentResult("Agent is no longer configured".into()));
            return;
        };
        let action = agents::AgentAction { working_dir: "~".into(), ..Default::default() };
        let plan = match agents::build_invocation(&profile, &action, prompt, &agents::new_session_id(), None, None) {
            Ok(plan) => plan,
            Err(error) => { self.shared.publish(Update::AgentResult(error.to_string())); return; }
        };
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let mut running = self.shared.agent_stop.lock().unwrap_or_else(PoisonError::into_inner);
        if running.is_some() { return; }
        *running = Some(stop);
        drop(running);
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let (events, mut updates) = tokio_mpsc::unbounded_channel();
            let drain = tokio::spawn(async move { while updates.recv().await.is_some() {} });
            let result = agents::run(plan, profile.output, events, stopped).await;
            let _ = drain.await;
            shared.agent_stop.lock().unwrap_or_else(PoisonError::into_inner).take();
            let text = match result.status {
                agents::RunStatus::Completed { final_text } => final_text,
                agents::RunStatus::Failed { message, .. } => message,
                agents::RunStatus::Stopped => "Stopped".into(),
            };
            shared.publish(Update::AgentResult(text));
        });
    }
    fn rename_chat(&self, id: &str, title: &str) {
        if self.shared.read_only || title.trim().is_empty() { return; }
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        let (id, title) = (id.to_owned(), title.trim().to_owned());
        app_backend::runtime::handle().spawn(async move {
            match engine.rename_chat(&id, &title).await {
                Ok(()) => { shared.publish_chats(&engine); }
                Err(error) => shared.publish(Update::Status(format!("Could not rename chat: {error}"))),
            }
        });
    }

    fn set_chat_muted(&self, id: &str, muted: bool) {
        self.chat_task(id, "mute chat", move |engine, id| async move { engine.set_chat_muted(&id, muted).await });
    }
    fn set_chat_read(&self, id: &str, read: bool) {
        self.chat_task(id, "mark chat", move |engine, id| async move {
            if read { engine.mark_read(&id).await } else { engine.mark_unread(&id).await }
        });
    }
    fn hide_chat(&self, id: &str) {
        self.chat_task(id, "hide chat", |engine, id| async move { engine.hide_chat(&id).await });
    }
    fn leave_chat(&self, id: &str) {
        self.chat_task(id, "leave chat", |engine, id| async move { engine.leave_chat(&id).await });
    }
    fn add_members(&self, id: &str, people: Vec<store::MemberRecord>) {
        self.chat_task(id, "add people", move |engine, id| async move {
            engine.add_chat_members(&id, &people, chatsvc::HistoryShare::None).await
        });
    }
    fn remove_member(&self, id: &str, user_id: &str) {
        let user_id = user_id.to_owned();
        self.chat_task(id, "remove member", move |engine, id| async move { engine.remove_chat_member(&id, &user_id).await });
    }
    fn search_people(&self, query: &str) {
        let Some(engine) = self.shared.engine() else { return };
        let (shared, query) = (self.shared.clone(), query.trim().to_owned());
        if query.is_empty() { return; }
        app_backend::runtime::handle().spawn(async move {
            match engine.search_people(&query).await {
                Ok(users) => {
                    let rows = users.into_iter().filter_map(|user| {
                        let mail = user.mail.or(user.user_principal_name);
                        let name = user.display_name.or_else(|| mail.clone())?;
                        Some(crate::types::Person { user_id: user.id, name, mail })
                    }).collect();
                    shared.publish(Update::People { query, rows });
                }
                Err(error) => shared.publish(Update::Status(format!("People search failed: {error}"))),
            }
        });
    }
    fn create_chat(&self, user_ids: Vec<String>, topic: Option<String>) {
        if self.shared.read_only || user_ids.is_empty() { return; }
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let made = match user_ids.as_slice() {
                [one] => engine.create_one_on_one(one).await,
                many => engine.create_group(many, topic.as_deref()).await,
            };
            match made {
                Ok(id) => { shared.publish_chats(&engine); shared.publish(Update::ChatCreated(id)); }
                Err(error) => shared.publish(Update::Status(format!("Could not create chat: {error}"))),
            }
        });
    }

    fn meet_now(&self, id: &str) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { self.shared.publish(Update::Status("Not connected yet".into())); return };
        match engine.store().chat(id) {
            Ok(Some(chat)) => {
                let me = engine.store().meta(META_USER_ID).ok().flatten().unwrap_or_default();
                if me.is_empty() || !(chat.kind.eq_ignore_ascii_case("oneOnOne") || chat.kind.eq_ignore_ascii_case("group")) {
                    self.shared.publish(Update::Status("This chat is not available for calling".into()));
                    return;
                }
                let callees = chat.members.iter().filter(|m| !m.is_bot()).filter_map(|m| {
                    let user = m.user_id.as_deref().filter(|user| *user != me)?;
                    Some(calling::Callee { mri: format!("8:orgid:{user}"), display_name: m.display_name.clone() })
                }).collect::<Vec<_>>();
                if callees.is_empty() || teams_core::is_notes_chat(id) {
                    self.shared.publish(Update::Status("This chat has no callable participants".into()));
                } else { self.shared.call_control.start(calling::CallSpec::People { callees, thread_id: id.to_owned() }); }
            }
            Ok(None) => self.shared.publish(Update::Status("Chat is not available yet".into())),
            Err(error) => self.shared.publish(Update::Status(format!("Could not start call: {error}"))),
        }
    }
    fn open_thread(&self, conversation_id: &str, root: &str) {
        let Some(engine) = self.shared.engine() else { return };
        self.shared.open_threads.lock().unwrap_or_else(PoisonError::into_inner).insert(root.to_owned());
        self.shared.publish_messages(&engine, conversation_id);
        let shared = self.shared.clone();
        let conversation_id = conversation_id.to_owned();
        let root = root.to_owned();
        app_backend::runtime::handle().spawn(async move {
            if let Err(error) = engine.refresh_thread(&conversation_id, &root).await {
                shared.publish(Update::Status(format!("Could not load thread: {error}")));
            } else if shared.is_open(&conversation_id) { shared.publish_messages(&engine, &conversation_id); }
        });
    }
    fn delete_message(&self, conversation_id: &str, message_id: &str) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        let conversation_id = conversation_id.to_owned();
        let message_id = message_id.to_owned();
        app_backend::runtime::handle().spawn(async move {
            if let Err(error) = engine.soft_delete_message(&conversation_id, &message_id).await {
                shared.publish(Update::Status(format!("Could not delete message: {error}")));
            } else if shared.is_open(&conversation_id) { shared.publish_messages(&engine, &conversation_id); }
        });
    }
    fn edit_message(&self, conversation_id: &str, message_id: &str, text: &str) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        let (conversation_id, message_id, text) = (conversation_id.to_owned(), message_id.to_owned(), text.to_owned());
        app_backend::runtime::handle().spawn(async move {
            if let Err(error) = engine.edit_message(&conversation_id, &message_id, &text).await {
                shared.publish(Update::Status(format!("Could not edit message: {error}")));
            } else if shared.is_open(&conversation_id) { shared.publish_messages(&engine, &conversation_id); }
        });
    }
    fn pin_message(&self, conversation_id: &str, message_id: &str, pin: bool) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        let (conversation_id, message_id) = (conversation_id.to_owned(), message_id.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let result = if pin { engine.pin_message(&conversation_id, &message_id).await }
                else { engine.unpin_message(&conversation_id, &message_id, None).await };
            match result {
                Ok(()) => refresh_pins(&shared, &engine, &conversation_id).await,
                Err(error) => shared.publish(Update::Status(format!("Could not {} message: {error}", if pin { "pin" } else { "unpin" }))),
            }
        });
    }
    fn forward_message(&self, source_id: &str, target_id: &str, message_id: &str) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        let (source, target, id) = (source_id.to_owned(), target_id.to_owned(), message_id.to_owned());
        app_backend::runtime::handle().spawn(async move {
            match engine.forward_messages(&source, &target, &[id], "").await {
                Ok(_) => shared.publish(Update::Status("Message forwarded".into())),
                Err(error) => shared.publish(Update::Status(format!("Could not forward message: {error}"))),
            }
        });
    }
    fn mark_unread_from(&self, conversation_id: &str, message_id: &str) {
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        let (conversation_id, message_id) = (conversation_id.to_owned(), message_id.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let at = engine.store().messages_by_id(&conversation_id, std::slice::from_ref(&message_id)).ok()
                .and_then(|mut rows| rows.remove(&message_id)).map(|record| record.created_at);
            let Some(at) = at else { shared.publish(Update::Status("Message is not cached".into())); return };
            if let Err(error) = engine.mark_unread_from(&conversation_id, at).await {
                shared.publish(Update::Status(format!("Could not mark as unread: {error}")));
            }
        });
    }
    fn react(&self, conversation_id: &str, message_id: &str, reaction: &str, remove: bool) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        let conversation_id = conversation_id.to_owned();
        let message_id = message_id.to_owned();
        let reaction = reaction.to_owned();
        app_backend::runtime::handle().spawn(async move {
            let result = if remove { engine.unset_reaction(&conversation_id, &message_id, &reaction).await }
                else { engine.set_reaction(&conversation_id, &message_id, &reaction).await };
            if let Err(error) = result { shared.publish(Update::Status(format!("Could not update reaction: {error}"))); }
            else if shared.is_open(&conversation_id) { shared.publish_messages(&engine, &conversation_id); }
        });
    }
    fn respond_event(&self, id: &str, ical_uid: Option<&str>, reply: chatsvc::Reply, send_response: bool) {
        if self.shared.read_only { return; }
        let calls = self.shared.calls.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(calls) = calls else { self.shared.publish(Update::Status("Calendar is not connected yet".into())); return };
        let shared = self.shared.clone();
        let id = id.to_owned();
        let ical_uid = ical_uid.map(str::to_owned);
        app_backend::runtime::handle().spawn(async move {
            match calls.respond_to_event(&id, ical_uid.as_deref(), reply, send_response).await {
                Ok(()) => shared.publish(Update::CalendarRefresh),
                Err(error) => shared.publish(Update::Status(format!("Could not respond to event: {error}"))),
            }
        });
    }
    fn search(&self, conversation_id: &str, query: &str) {
        let Some(engine) = self.shared.engine() else { return };
        let result = engine.search_messages_in(conversation_id, query, 100).and_then(|hits| {
            let ids: Vec<_> = hits.into_iter().map(|hit| hit.message_id).collect();
            let records = engine.store().messages_by_id(conversation_id, &ids)?;
            let ordered: Vec<_> = ids.iter().rev().filter_map(|id| records.get(id).cloned()).collect();
            let me = engine.store().meta(META_USER_ID)?.unwrap_or_default();
            Ok(message_rows(&ordered, &me, Local::now()))
        });
        match result {
            Ok(rows) => self.shared.publish(Update::Search { conversation_id: conversation_id.to_owned(), query: query.to_owned(), rows }),
            Err(error) => self.shared.publish(Update::Status(format!("Search failed: {error}"))),
        }
    }
    fn search_all(&self, query: &str) {
        let Some(engine) = self.shared.engine() else { return };
        match crate::search::run(&engine, query) {
            Ok(results) => self.shared.publish(Update::GlobalSearch(results)),
            Err(error) => self.shared.publish(Update::Status(format!("Search failed: {error}"))),
        }
    }
    fn upload_file(&self, target: SendTarget, path: PathBuf) { crate::media::upload_file(self.shared.clone(), target, path); }
    fn discard_attachment(&self, uploaded: teams_core::UploadedFile) { crate::media::discard_attachment(self.shared.clone(), uploaded); }
    fn set_appearance(&self, settings: app_backend::appearance::AppearanceSettings) {
        if let Some(engine) = self.shared.engine() { app_backend::appearance::save(engine.store(), &settings); }
    }
    fn accept_call(&self, ring_id: u64, video: bool) { self.shared.call_control.accept(ring_id, video); }
    fn decline_call(&self, ring_id: u64) { self.shared.call_control.decline(ring_id); }
    fn load_image(&self, image: teams_core::ImageRef) { crate::media::load_image(self.shared.clone(), image); }
    fn download_file(&self, url: String, path: PathBuf) { crate::media::download_file(self.shared.clone(), url, path); }
    fn join_call(&self, url: &str, video: bool) { self.shared.call_control.join(url, video); }
    fn call_command(&self, command: calling::CallCommand) { self.shared.call_control.command(command); }
    fn take_video(&self) -> Vec<(calling::VideoKey, Arc<calling::VideoPicture>)> { self.shared.call_control.take_video() }
    fn load_older(&self, conversation_id: &str) {
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        let conversation_id = conversation_id.to_owned();
        app_backend::runtime::handle().spawn(async move {
            match engine.load_older(&conversation_id).await {
                Ok(_) if shared.is_open(&conversation_id) => {
                    *shared.message_limit.lock().unwrap_or_else(PoisonError::into_inner) += 50;
                    shared.publish_messages(&engine, &conversation_id);
                }
                Ok(_) => {},
                Err(error) => shared.publish(Update::Status(format!("Could not load older messages: {error}"))),
            }
        });
    }
    fn read_only(&self) -> bool { self.shared.read_only }

    fn set_availability(&self, forced: Option<teams_core::ForcedAvailability>) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            if let Err(error) = engine.set_own_availability(forced).await {
                shared.publish(Update::Status(format!("Could not set status: {error}")));
            }
        });
    }
    fn set_status_note(&self, note: Option<teams_core::StatusNote>) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            if let Err(error) = engine.set_own_status_note(note).await {
                shared.publish(Update::Status(format!("Could not set status message: {error}")));
            }
        });
    }
    fn load_profile(&self, user_id: &str) {
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::Profile { user_id: user_id.to_owned(), result: Err("Not connected yet".into()) });
            return;
        };
        let (shared, user_id) = (self.shared.clone(), user_id.to_owned());
        shared.publish_avatars(&engine, vec![user_id.clone()]);
        app_backend::runtime::handle().spawn(async move {
            let result = engine.person_profile(&user_id).await.map_err(|error| error.to_string());
            shared.publish(Update::Profile { user_id, result });
        });
    }
    fn open_direct(&self, user_id: &str) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let (shared, user_id) = (self.shared.clone(), user_id.to_owned());
        app_backend::runtime::handle().spawn(async move {
            match one_on_one(&engine, &user_id).await {
                Ok(id) => shared.publish(Update::OpenChat(id)),
                Err(error) => shared.publish(Update::Status(format!("Could not open chat: {error}"))),
            }
        });
    }
    fn call_person(&self, user_id: &str, name: &str) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let (shared, user_id, name) = (self.shared.clone(), user_id.to_owned(), name.to_owned());
        app_backend::runtime::handle().spawn(async move {
            match one_on_one(&engine, &user_id).await {
                Ok(thread_id) => shared.call_control.start(calling::CallSpec::People {
                    callees: vec![calling::Callee { mri: format!("8:orgid:{user_id}"), display_name: name }], thread_id,
                }),
                Err(error) => shared.publish(Update::Status(format!("Could not start call: {error}"))),
            }
        });
    }

    fn load_calendar(&self, week: chrono::NaiveDate) {
        let calls = self.shared.calls.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(calls) = calls else {
            self.shared.publish(Update::Status("Calendar is not connected yet".into()));
            return;
        };
        let Some(start) = week.and_hms_opt(0, 0, 0).and_then(|time| Local.from_local_datetime(&time).earliest()) else { return };
        let Some(end) = (week + chrono::Duration::days(7)).and_hms_opt(0, 0, 0).and_then(|time| Local.from_local_datetime(&time).earliest()) else { return };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            match calls.calendar_view(start.with_timezone(&Utc), end.with_timezone(&Utc)).await {
                Ok(events) => shared.publish(Update::Calendar { week, rows: events.into_iter().map(|event| crate::screens::CalendarRow {
                    id: event.object_id, subject: event.subject, start: event.start, end: event.end,
                    cancelled: event.cancelled, all_day: event.all_day, organizer: event.organizer_name,
                    ical_uid: event.ical_uid, join_url: event.join_url, thread_id: event.thread_id, response: event.response,
                }).collect() }),
                Err(error) => shared.publish(Update::Status(format!("Calendar unavailable: {error}"))),
            }
        });
    }

    fn load_agenda(&self) {
        let calls = self.shared.calls.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(calls) = calls else {
            self.shared.publish(Update::Status("Calendar is not connected yet".into()));
            return;
        };
        let (start, end) = crate::agenda::window(Utc::now());
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            match calls.calendar_view(start, end).await {
                Ok(events) => shared.publish(Update::Agenda(events)),
                Err(error) => shared.publish(Update::Status(format!("Agenda unavailable: {error}"))),
            }
        });
    }

    fn mark_activity_read(&self, id: &str) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        let id = id.to_owned();
        app_backend::runtime::handle().spawn(async move {
            if let Err(error) = engine.mark_activity_read(&id).await { shared.publish(Update::Status(format!("Activity update failed: {error}"))); }
        });
    }

    fn mark_all_activity_read(&self) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            if let Err(error) = engine.mark_all_activity_read().await { shared.publish(Update::Status(format!("Activity update failed: {error}"))); }
        });
    }

    fn open(&self, conversation_id: &str) {
        self.shared.open_threads.lock().unwrap_or_else(PoisonError::into_inner).clear();
        *self.shared.message_limit.lock().unwrap_or_else(PoisonError::into_inner) = 50;
        *self
            .shared
            .open
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(conversation_id.to_owned());
        let Some(engine) = self.shared.engine() else {
            return;
        };
        self.shared.publish_messages(&engine, conversation_id);
        {
            let shared = self.shared.clone();
            let conversation_id = conversation_id.to_owned();
            let engine = engine.clone();
            app_backend::runtime::handle().spawn(async move {
                if !shared.read_only && let Err(error) = engine.mark_read(&conversation_id).await {
                    shared.publish(Update::Status(format!("Could not mark conversation {conversation_id} read: {error}")));
                }
                refresh_pins(&shared, &engine, &conversation_id).await;
                if let Ok(Some(chat)) = engine.store().chat(&conversation_id) {
                    let ids: Vec<_> = chat.members.iter().filter(|member| !member.is_bot()).filter_map(|member| member.user_id.clone()).collect();
                    if let Err(error) = engine.watch_presence(&ids).await { shared.publish(Update::Status(format!("Presence subscription unavailable: {error}"))); }
                    if let Err(error) = engine.refresh_presence(&ids).await { shared.publish(Update::Status(format!("Presence unavailable: {error}"))); }
                    shared.publish_chats(&engine);
                }
            });
        }
        let shared = self.shared.clone();
        let conversation_id = conversation_id.to_owned();
        let engine = engine.clone();
        app_backend::runtime::handle().spawn(async move {
            match engine.fetch_newer(&conversation_id).await {
                Ok(_) => {
                    if shared.is_open(&conversation_id) {
                        shared.publish_messages(&engine, &conversation_id);
                    }
                }
                Err(error) => shared.publish(Update::Status(format!(
                    "Could not refresh messages: {error}"
                ))),
            }
        });
    }

    fn send(&self, target: SendTarget, text: String, files: Vec<teams_core::FileReference>) {
        self.send_to(target, crate::types::Composed { text, ..Default::default() }, files);
    }

    fn send_composed(&self, target: SendTarget, composed: crate::types::Composed, files: Vec<teams_core::FileReference>) {
        self.send_to(target, composed, files);
    }

    fn search_gifs(&self, query: &str) {
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::Gifs { query: query.to_owned(), result: Err("Not connected yet".into()) });
            return;
        };
        let (shared, query) = (self.shared.clone(), query.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let result = engine.search_gifs(&query).await.map_err(|error| error.to_string());
            shared.publish(Update::Gifs { query, result });
        });
    }

    fn list_cloud_files(&self, place: crate::cloud_picker::Place) {
        use crate::cloud_picker::{Place, entry};
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::CloudFiles { place, result: Err("Not connected yet".into()) });
            return;
        };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let result = match &place {
                Place::Recent => engine.picker_recent().await
                    .map(|files| files.into_iter().map(|(file, owner)| entry(file, owner)).collect()),
                Place::MyFiles => async { engine.picker_folder(&engine.picker_root(None).await?).await }.await,
                Place::Shared => engine.picker_shared().await,
                Place::Folder(folder) => engine.picker_folder(folder).await,
                Place::Search(query) => engine.search_cloud_files(&teams_core::substrate_search::SearchSession::new(), query).await
                    .map(|files| files.into_iter().map(|file| entry(file, None)).collect()),
            };
            shared.publish(Update::CloudFiles { place, result: result.map_err(|error| error.to_string()) });
        });
    }

    fn attach_cloud_file(&self, target: SendTarget, file: teams_core::CloudFile) {
        let name = file.name.clone();
        let engine = self.shared.engine().filter(|_| !self.shared.read_only);
        let Some(engine) = engine else {
            self.shared.publish(Update::CloudAttached { target, name, result: Err("Not connected yet".into()) });
            return;
        };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let result = engine.attach_cloud_file(&target.conversation_id, &file).await.map_err(|error| error.to_string());
            shared.publish(Update::CloudAttached { target, name, result });
        });
    }

    fn save_draft(&self, conversation_id: &str, text: &str) {
        let Some(engine) = self.shared.engine() else { return };
        let _ = match crate::composed::draft_record(conversation_id, text) {
            Some(record) => engine.store().save_draft_text(&record),
            None => engine.store().delete_draft(conversation_id),
        };
    }

    fn load_draft(&self, conversation_id: &str) -> Option<String> {
        let record = self.shared.engine()?.store().draft(conversation_id).ok()??;
        crate::composed::draft_text(&record.payload)
    }
}

impl Bridge {
    fn send_to(&self, target: SendTarget, composed: crate::types::Composed, files: Vec<teams_core::FileReference>) {
        let crate::types::Composed { text, mentions, gifs } = composed;
        if text.trim().is_empty() && files.is_empty() && gifs.is_empty() {
            return;
        }
        if self.shared.read_only {
            self.shared.publish(Update::SendFailed { target, text, files, error: "Read-only mode".into() });
            return;
        }
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::SendFailed {
                target,
                text,
                files,
                error: "Not connected yet".into(),
            });
            return;
        };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let conversation_id = &target.conversation_id;
            let extras = teams_core::MessageExtras { files: files.clone(), ..Default::default() };
            let html = crate::composed::html(&text, &gifs);
            let result = if let Some(root) = &target.thread_root_id {
                engine.send_message_with_extras(conversation_id, &html, Some(root), &mentions, &extras).await
            } else if target.channel {
                engine.post_to_channel_with_extras(conversation_id, &html, None, &mentions, &extras).await
            } else {
                engine.send_message_with_extras(conversation_id, &html, None, &mentions, &extras).await
            };
            match result {
                Ok(_) => shared.publish_messages(&engine, conversation_id),
                Err(error) => shared.publish(Update::SendFailed {
                    target,
                    text,
                    files,
                    error: error.to_string(),
                }),
            }
        });
    }
}

/// Publishes the signed-in user and photo, and refreshes their own presence (answered by a
/// `PresenceStatusChanged` event).
fn refresh_me(shared: &Arc<Shared>, engine: &Arc<Engine>) {
    let (shared, engine) = (shared.clone(), engine.clone());
    app_backend::runtime::handle().spawn(async move {
        if let Err(error) = engine.refresh_own_status().await {
            app_backend::diagnostics::record(format!("Rune: Own status unavailable: {error}"));
        }
        if let Some(me) = engine.me() {
            let email = engine.store().meta("me_email").ok().flatten().unwrap_or_default();
            shared.publish_avatars(&engine, vec![me.user_id.clone()]);
            shared.publish(Update::Me { user_id: me.user_id, name: me.display_name, email });
        }
    });
}

/// The existing 1:1 chat with `user_id`, else a new one.
const WARM_CHATS: usize = 15;

fn recent_chat_ids(chats: &[ChatRecord], count: usize) -> Vec<String> {
    let mut recent: Vec<_> = chats.iter().collect();
    recent.sort_by_key(|chat| std::cmp::Reverse(chat.last_message_at));
    recent.into_iter().take(count).map(|chat| chat.id.clone()).collect()
}

async fn one_on_one(engine: &Engine, user_id: &str) -> Result<String, String> {
    if let Ok(sidebar) = engine.sidebar()
        && let Some(chat) = sidebar.chats.iter().find(|chat| chat.kind == "oneOnOne" && chat.members.iter().any(|m| m.user_id.as_deref() == Some(user_id)))
    {
        return Ok(chat.id.clone());
    }
    engine.create_one_on_one(user_id).await.map_err(|error| error.to_string())
}

async fn forward(shared: Arc<Shared>, mut events: tokio_mpsc::UnboundedReceiver<BackendEvent>) {
    let mut notifications = crate::notifications::Notifications::new();
    while let Some(event) = events.recv().await {
        match event {
            BackendEvent::Typing(event) => shared.publish(Update::Typing(event)),
            BackendEvent::Connection(state) => {
                if state == ConnectionState::Online && let Some(engine) = shared.engine() {
                    shared.publish_chats(&engine);
                    refresh_me(&shared, &engine);
                }
                shared.publish(Update::Status(status_text(&state)));
            }
            BackendEvent::Engine(engine) => {
                *shared.engine.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(engine.clone());
                // The engine exists only after a session, and its cached chats are shown now.
                // The backend's own Online comes after its first sidebar refresh (and folders),
                // so set it here; later Connection events, sent after this one, still override it.
                shared.publish_chats(&engine);
                refresh_me(&shared, &engine);
                if let Some(activity) = teams_core::stored_activity_feed(engine.store()) { shared.publish(Update::Activity(activity)); }
                shared.publish(Update::Appearance(app_backend::appearance::load(engine.store())));
                let settings = engine.store().meta(agents::SETTINGS_KEY).ok().flatten()
                    .and_then(|json| agents::AgentSettings::from_json(&json).ok()).unwrap_or_default();
                shared.publish(Update::AgentSettings(settings));
                let shared = shared.clone();
                app_backend::runtime::handle().spawn(async move {
                    if let Ok(sidebar) = engine.sidebar() {
                        let ids: Vec<_> = sidebar.chats.iter().flat_map(|c| &c.members).filter(|m| !m.is_bot()).filter_map(|m| m.user_id.clone()).collect();
                        if let Err(error) = engine.watch_presence(&ids).await { shared.publish(Update::Status(format!("Presence subscription unavailable: {error}"))); }
                        if let Err(error) = engine.refresh_presence(&ids).await { shared.publish(Update::Status(format!("Presence unavailable: {error}"))); }
                    }
                });
            }
            BackendEvent::Calls(calls) => {
                shared.call_control.set_launcher(calls.clone());
                *shared.calls.lock().unwrap_or_else(PoisonError::into_inner) = Some(calls);
                shared.publish(Update::CalendarRefresh);
            }
            BackendEvent::Ring(event) => shared.publish(Update::Ring(event)),
            BackendEvent::Core(event) => {
                let Some(engine) = shared.engine() else {
                    continue;
                };
                match event {
                    CoreEvent::AvatarsChanged { user_ids } => {
                        for id in user_ids {
                            if let Some(photo) = engine.avatar(&id) { shared.publish(Update::Image { key: format!("avatar:{id}"), bytes: photo.bytes }); }
                        }
                    }
                    CoreEvent::ImagesChanged { keys } => {
                        for key in keys {
                            if let Some(image) = engine.image(&key) { shared.publish(Update::Image { key, bytes: image.bytes }); }
                        }
                    }
                    CoreEvent::Error { message } => shared.publish(Update::Status(message)),
                    CoreEvent::ActivityChanged => {
                        if let Some(activity) = teams_core::stored_activity_feed(engine.store()) { shared.publish(Update::Activity(activity)); }
                    }
                    CoreEvent::SidebarChanged => {
                        shared.publish_chats(&engine);
                    }
                    CoreEvent::PresenceChanged => { shared.publish_chats(&engine); }
                    CoreEvent::PresenceStatusChanged(status) => shared.publish(Update::OwnStatus(status)),
                    CoreEvent::PinsChanged { conversation_id } => {
                        if shared.is_open(&conversation_id) { refresh_pins(&shared, &engine, &conversation_id).await; }
                    }
                    CoreEvent::MessagesStoredLate { conversation_id, message_ids } => {
                        notifications.mark_late(&conversation_id, message_ids);
                        if shared.is_open(&conversation_id) {
                            shared.publish_messages(&engine, &conversation_id);
                        }
                    }
                    CoreEvent::MessagesChanged { conversation_id } => {
                        let (foreground, in_call) = shared.foreground.lock().unwrap_or_else(PoisonError::into_inner).clone();
                        let environment = crate::notifications::environment(&engine, foreground.as_deref() == Some(&conversation_id), in_call);
                        notifications.messages(&engine, &conversation_id, environment);
                        if shared.is_open(&conversation_id) { shared.publish_messages(&engine, &conversation_id); }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

async fn refresh_pins(shared: &Shared, engine: &Engine, id: &str) {
    match engine.chat_pins(id).await {
        Ok(pins) => {
            let pin = if let Some(pin) = pins.first() {
                let cached = engine.store().messages_by_id(id, std::slice::from_ref(&pin.message_id)).ok().and_then(|mut rows| rows.remove(&pin.message_id));
                let record = if cached.is_some() { cached } else {
                    match engine.refresh_message(id, &pin.message_id).await {
                        Ok(record) => record,
                        Err(error) => { shared.publish(Update::Status(format!("Pinned message unavailable: {error}"))); None }
                    }
                };
                match record {
                    Some(record) => Some(crate::types::PinnedRow {
                        conversation_id: id.to_owned(), sender: record.sender_name.unwrap_or_default(),
                        text: if record.deleted { DELETED_TEXT.to_owned() } else { html_to_text(&record.body_html) },
                    }),
                    _ => Some(crate::types::PinnedRow { conversation_id: id.to_owned(), sender: String::new(), text: format!("Message {}", pin.message_id) }),
                }
            } else { None };
            shared.publish(Update::Pinned { conversation_id: id.to_owned(), pin });
        }
        Err(error) => shared.publish(Update::Status(format!("Pinned messages unavailable: {error}"))),
    }
}

fn status_text(state: &ConnectionState) -> String {
    match state {
        ConnectionState::Connecting => "Connecting…".into(),
        ConnectionState::Online => "Online".into(),
        ConnectionState::NoBrowser => "No browser found".into(),
        ConnectionState::NoAppTab => "Open Teams in a browser tab".into(),
        ConnectionState::LoginRequired => "Sign in in the browser window".into(),
        ConnectionState::Failed(error) => format!("Failed: {error}"),
    }
}

/// Newest activity first; chats without activity last.
pub fn chat_rows(mut chats: Vec<ChatRecord>, now: DateTime<Local>) -> Vec<ChatRow> {
    chats.sort_by_key(|chat| std::cmp::Reverse(chat.last_message_at));
    chats
        .iter()
        .map(|chat| ChatRow {
            id: chat.id.clone(),
            title: chat.display_title(),
            preview: chat_preview(chat),
            unread: chat.unread,
            when: chat
                .last_message_at
                .map(|at| format_when(at, now, false))
                .unwrap_or_default(),
        })
        .collect()
}

fn team_rows(teams: Vec<SidebarTeam>) -> Vec<TeamRow> {
    teams
        .into_iter()
        .filter(|team| !team.hidden)
        .map(|team| TeamRow {
            id: team.team.id,
            name: team.team.name,
            channels: team
                .channels
                .into_iter()
                .map(|channel| ChannelRow {
                    hidden: team.hidden_channel_ids.contains(&channel.id),
                    id: channel.id,
                    name: channel.name,
                    unread: channel.unread,
                })
                .collect(),
        })
        .collect()
}

fn chat_preview(chat: &ChatRecord) -> String {
    let text = if chat.last_message_deleted {
        DELETED_TEXT.to_owned()
    } else {
        single_line(chat.last_message_preview.as_deref().unwrap_or_default())
    };
    match chat.last_message_sender_name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() && text.is_empty() => name.to_owned(),
        Some(name) if !name.is_empty() => format!("{name}: {text}"),
        _ => text,
    }
}

/// Oldest first, as the records arrive from the store.
pub fn message_rows(
    records: &[MessageRecord],
    my_id: &str,
    now: DateTime<Local>,
) -> Vec<MessageRow> {
    records
        .iter()
        .map(|record| MessageRow {
            sender_id: record.sender_id.clone(),
            system: teams_core::call_event(record).is_some() || teams_core::roster_event::roster_event(record).is_some(),
            id: record.message_id.clone(),
            reply_to_id: record.reply_to_id.clone(),
            deleted: record.deleted,
            can_delete: teams_core::can_delete(record, my_id),
            reactions: teams_core::reaction_summaries(record, (!my_id.is_empty()).then_some(my_id)),
            sender: record
                .sender_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(UNKNOWN_SENDER)
                .to_owned(),
            when: format_when(record.created_at, now, true),
            text: if record.deleted {
                DELETED_TEXT.to_owned()
            } else {
                message_text(record)
            },
            mine: !my_id.is_empty() && record.sender_id.as_deref() == Some(my_id),
            media: crate::media::Content::from_record(record),
        })
        .collect()
}

fn channel_post(record: &MessageRecord) -> bool {
    teams_core::roster_event::roster_event(record).is_none()
        && !(record.sender_id.is_none() && record.call_event_json.is_none()
            && record.body_html.trim().is_empty()
            && record.subject.as_deref().is_none_or(|subject| subject.trim().is_empty())
            && record.attachments_json.trim() == "[]")
}

fn message_text(record: &MessageRecord) -> String {
    if let Some(event) = teams_core::roster_event::roster_event(record) {
        return event.text(|_, name| name.filter(|name| !name.trim().is_empty()).unwrap_or("Someone").to_owned());
    }
    let body = teams_core::readable_text(record);
    record.subject.iter().cloned().chain(std::iter::once(body)).chain(teams_core::card_texts(record))
        .filter(|part| !part.trim().is_empty()).collect::<Vec<_>>().join("\n")
}

/// "14:05" when `at` is on the same local day as `now`, else "08/11" ("08/11 14:05" when
/// `time_on_other_days`).
pub fn format_when(at: DateTime<Utc>, now: DateTime<Local>, time_on_other_days: bool) -> String {
    let local = at.with_timezone(&Local);
    let time = local.format("%H:%M");
    if local.date_naive() == now.date_naive() {
        time.to_string()
    } else if time_on_other_days {
        format!("{} {time}", local.format("%d/%m"))
    } else {
        local.format("%d/%m").to_string()
    }
}

/// HTML body to plain text: tags dropped, entities decoded, block breaks as newlines,
/// whitespace collapsed per line, empty lines dropped.
pub fn html_to_text(html: &str) -> String {
    let mut raw = String::new();
    for node in Html::parse_fragment(html).root_element().descendants() {
        match node.value() {
            Node::Text(text) => raw.push_str(&text.replace(['\n', '\r', '\t'], " ")),
            Node::Element(element) if matches!(element.name(), "br" | "p" | "div" | "li") => {
                raw.push('\n')
            }
            _ => {}
        }
    }
    raw.lines()
        .map(single_line)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn data_path(name: &str) -> PathBuf {
    directories::ProjectDirs::from("", "", store::DATA_DIR_NAME)
        .map(|directories| directories.data_local_dir().join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// The cache database can be locked by the previous instance while it exits, so retry briefly.
fn open_store(path: &Path) -> Store {
    for _ in 1..STORE_OPEN_ATTEMPTS {
        match Store::open(path) {
            Ok(store) => return store,
            Err(_) => std::thread::sleep(STORE_OPEN_INTERVAL),
        }
    }
    Store::open(path).expect("cache database")
}

#[cfg(windows)]
fn transport(database: &Path, no_sso: bool) -> Arc<dyn session::Transport> {
    if no_sso {
        webview::disable_single_sign_on();
    }
    let user_data_folder = match database.parent() {
        Some(directory) => directory.join(WEBVIEW_FOLDER),
        None => data_path(WEBVIEW_FOLDER),
    };
    webview::start(webview::HostConfig {
        user_data_folder,
        window_title: WINDOW_TITLE.into(),
        log: |message| app_backend::diagnostics::record(message.to_owned()),
    })
}

#[cfg(not(windows))]
fn transport(_database: &Path, _no_sso: bool) -> Arc<dyn session::Transport> {
    Arc::new(session::CdpTransport::new(session::DEFAULT_ENDPOINT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn chat(id: &str, at: Option<DateTime<Utc>>) -> ChatRecord {
        ChatRecord {
            id: id.into(),
            title: format!("Chat {id}"),
            last_message_at: at,
            ..ChatRecord::default()
        }
    }

    #[test]
    fn the_newest_chats_are_warmed_first() {
        let at = |hour| Some(Utc.with_ymd_and_hms(2026, 11, 11, hour, 0, 0).unwrap());
        let chats = [chat("old", at(8)), chat("never", None), chat("new", at(12)), chat("mid", at(10))];
        assert_eq!(recent_chat_ids(&chats, 2), ["new", "mid"]);
    }

    #[test]
    fn meeting_records_keep_the_system_kind_instead_of_a_chat_author() {
        let event = teams_core::CallEvent {
            phase: teams_core::CallPhase::Started, call_type: teams_core::CallType::Meeting,
            call_id: "meeting".into(), duration_seconds: None, initiator_id: None,
            initiator_name: None, participant_ids: vec![], participant_names: vec![], recording: None,
        };
        let record = MessageRecord { call_event_json: Some(event.to_json()), ..Default::default() };
        let rows = message_rows(&[record], "me", Local::now());
        assert!(rows[0].system);
        assert_eq!(rows[0].text, "Meeting started");
    }

    #[test]
    fn channel_feed_excludes_membership_events_but_preserves_real_posts() {
        let mut record = MessageRecord { attachments_json: "[]".into(), ..Default::default() };
        assert!(!channel_post(&record));
        record.call_event_json = Some(r#"{"roster":{"change":"Added","initiator_id":"a","initiator_name":null,"member_ids":["b"],"member_names":[null],"history":"All","topic":null}}"#.into());
        assert!(!channel_post(&record));
        record.call_event_json = None;
        record.sender_id = Some("ada".into());
        record.subject = Some("Release notes".into());
        record.body_html = "<p>Ready to ship</p>".into();
        assert!(channel_post(&record));
        assert_eq!(message_text(&record), "Release notes\nReady to ship");
    }

    #[test]
    fn html_to_text_keeps_paragraphs_and_decodes_entities() {
        assert_eq!(
            html_to_text("<p>Hello <b>big</b></p><p>world &amp; co&nbsp;&lt;3 &quot;&#39;</p>"),
            "Hello big\nworld & co <3 \"'"
        );
        assert_eq!(html_to_text("a<br>b"), "a\nb");
        assert_eq!(html_to_text("line one\n   two"), "line one two");
        assert_eq!(html_to_text(""), "");
    }

    #[test]
    fn chats_sort_newest_first_and_unknown_last() {
        let early = Utc.with_ymd_and_hms(2026, 11, 8, 9, 0, 0).unwrap();
        let late = Utc.with_ymd_and_hms(2026, 11, 9, 9, 0, 0).unwrap();
        let now = late.with_timezone(&Local);
        let rows = chat_rows(
            vec![
                chat("old", Some(early)),
                chat("none", None),
                chat("new", Some(late)),
            ],
            now,
        );
        let ids: Vec<_> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["new", "old", "none"]);
        assert_eq!(rows[2].when, "");
        assert_eq!(rows[0].title, "Chat new");
    }

    #[test]
    fn chat_preview_prefixes_sender_and_hides_deleted_bodies() {
        let mut record = chat("a", None);
        record.last_message_sender_name = Some("Ada".into());
        record.last_message_preview = Some("see\nyou".into());
        assert_eq!(chat_preview(&record), "Ada: see you");

        record.last_message_deleted = true;
        assert_eq!(chat_preview(&record), "Ada: This message was deleted");

        record.last_message_sender_name = None;
        record.last_message_deleted = false;
        record.last_message_preview = None;
        assert_eq!(chat_preview(&record), "");
    }

    #[test]
    fn message_rows_map_sender_mine_and_deleted() {
        let at = Utc.with_ymd_and_hms(2026, 11, 8, 12, 5, 0).unwrap();
        let now = at.with_timezone(&Local);
        let mine = MessageRecord {
            message_id: "1".into(),
            sender_id: Some("me".into()),
            sender_name: Some("Me".into()),
            created_at: at,
            body_html: "<p>hi</p>".into(),
            ..MessageRecord::default()
        };
        let gone = MessageRecord {
            message_id: "2".into(),
            created_at: at,
            deleted: true,
            body_html: "<p>secret</p>".into(),
            ..MessageRecord::default()
        };
        let rows = message_rows(&[mine, gone], "me", now);
        assert!(rows[0].mine);
        assert_eq!(rows[0].sender, "Me");
        assert_eq!(rows[0].text, "hi");
        assert!(!rows[1].mine);
        assert_eq!(rows[1].sender, "Unknown");
        assert_eq!(rows[1].text, DELETED_TEXT);
        assert_eq!(
            rows[0].when,
            at.with_timezone(&Local).format("%H:%M").to_string()
        );
    }

    #[test]
    fn format_when_uses_time_today_and_date_otherwise() {
        let at = Utc.with_ymd_and_hms(2026, 11, 8, 12, 5, 0).unwrap();
        let local = at.with_timezone(&Local);
        let date = local.format("%d/%m").to_string();
        let time = local.format("%H:%M").to_string();
        let later = (at + Duration::days(2)).with_timezone(&Local);

        assert_eq!(format_when(at, local, false), time);
        assert_eq!(format_when(at, later, false), date);
        assert_eq!(format_when(at, later, true), format!("{date} {time}"));
    }
}
