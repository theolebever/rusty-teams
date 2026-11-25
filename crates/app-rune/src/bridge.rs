//! Backend bridge for the Rune view. Starts the Teams backend on the shared tokio runtime and
//! turns its events into `Update`s that the view drains once per frame.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use chrono::{DateTime, Local, TimeZone, Utc};
use scraper::{Html, Node};
use app_backend::backend::{BackendEvent, ConnectionState, Engine};
use store::{ChatRecord, MessageRecord, SidebarTeam, Store};
use teams_core::CoreEvent;
use tokio::sync::{mpsc as tokio_mpsc, watch};

use crate::accounts;
use crate::types::{Actions, ChannelRow, ChatRow, MessageRow, SendTarget, TeamRow, Update};

pub(crate) const DATABASE_FILE: &str = "cache.sqlite3";
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
    /// The call window's bridge: `open` follows the call chat and leaves the main chat alone.
    call_chat: bool,
    detached_chat: Option<String>,
}

/// State shared by the view-facing calls and the event forwarder.
pub(crate) struct Shared {
    pub(crate) outbox_lock: tokio::sync::Mutex<()>,
    pub(crate) fresh_outbox: Mutex<std::collections::HashSet<String>>,
    switcher_query: Mutex<(String, String)>,
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
    /// The chat shown beside the call in the call window, followed like the open one.
    call_chat: Mutex<Option<String>>,
    detached_chats: Mutex<std::collections::HashSet<String>>,
    /// Chats whose detached window closed; the main view drops their retained handles next frame.
    closed_chat_windows: Mutex<Vec<String>>,
    open_generation: AtomicU64,
    live_retry: Mutex<Option<Arc<tokio::sync::Notify>>>,
    calendar_nudge: Mutex<Option<tokio_mpsc::UnboundedSender<()>>>,
    updates: Sender<Update>,
    /// The last sidebar snapshot of each kind sent, so an identical republish (presence
    /// changes reload the whole sidebar) does not rebuild the view.
    last_snapshots: Mutex<std::collections::HashMap<std::mem::Discriminant<Update>, Update>>,
    wake: Box<dyn Fn() + Send + Sync>,
    pub(crate) agent_stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    pub(crate) store: Arc<Store>,
    /// The account folder of this run, recorded in the registry once it is `Online`.
    folder: PathBuf,
    identity_saved: AtomicBool,
    /// Opens the sign-in window of an account (`Actions::sign_in`).
    transport: Arc<dyn session::Transport>,
}

impl Shared {
    pub(crate) fn publish(&self, update: Update) {
        if let Update::Status(status) = &update { app_backend::diagnostics::record(format!("Rune: {status}")); }
        let update = match update {
            Update::Status(status) if self.read_only => Update::Status(format!("{status} · Read-only")),
            update => update,
        };
        if is_snapshot(&update) {
            let mut last = self.last_snapshots.lock().unwrap_or_else(PoisonError::into_inner);
            let kind = std::mem::discriminant(&update);
            if last.get(&kind) == Some(&update) { return; }
            last.insert(kind, update.clone());
        }
        let _ = self.updates.send(update);
        (self.wake)();
    }

    pub(crate) fn launcher(&self) -> Option<Arc<app_backend::launcher::CallLauncher>> {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub(crate) fn engine(&self) -> Option<Arc<Engine>> {
        self.engine
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// GPUI's `refresh_recording_policy` and `apply_recording_rights`: the org recording policy
    /// and, for a meeting, our own tenant (recording is closed to people from another org).
    /// ponytail: read again for each call, cache like GPUI if it shows in the call setup time.
    fn load_recording_rights(self: &Arc<Self>, meeting_chat: Option<String>) {
        let Some(engine) = self.engine() else { return };
        let shared = self.clone();
        app_backend::runtime::handle().spawn(async move {
            let policy = engine.recording_policy().await.ok();
            let own_tenant = match meeting_chat {
                Some(chat) => engine.own_tenant_id(&chat).await.ok(),
                None => None,
            };
            shared.publish(Update::RecordingRights { policy, own_tenant });
        });
    }

    fn is_open(&self, conversation_id: &str) -> bool {
        if self.detached_chats.lock().unwrap_or_else(PoisonError::into_inner).contains(conversation_id) { return true; }
        [&self.open, &self.call_chat]
            .iter()
            .any(|open| open.lock().unwrap_or_else(PoisonError::into_inner).as_deref() == Some(conversation_id))
    }

    /// `open` for the call window's chat panel: shows the cached messages, marks them read and
    /// fetches newer ones, without touching the main window's open chat.
    fn open_call_chat(self: &Arc<Self>, conversation_id: &str) {
        *self.call_chat.lock().unwrap_or_else(PoisonError::into_inner) = Some(conversation_id.to_owned());
        self.refresh_separate_chat(conversation_id);
    }

    fn refresh_separate_chat(self: &Arc<Self>, conversation_id: &str) {
        let Some(engine) = self.engine() else { return };
        self.publish_messages(&engine, conversation_id);
        let (shared, conversation_id) = (self.clone(), conversation_id.to_owned());
        app_backend::runtime::handle().spawn(async move {
            if !shared.read_only && let Err(error) = engine.mark_read(&conversation_id).await {
                app_backend::diagnostics::record(format!("Rune: call chat not marked read: {error}"));
            }
            match engine.fetch_newer(&conversation_id).await {
                Ok(_) if shared.is_open(&conversation_id) => shared.publish_messages(&engine, &conversation_id),
                Ok(_) => {}
                Err(error) => shared.publish(Update::Status(format!("Could not refresh messages: {error}"))),
            }
            shared.publish_chats(&engine);
        });
    }

    /// Publishes the chat list. False when the sidebar could not be read (the error is the status).
    fn publish_chats(&self, engine: &Engine) -> bool {
        match engine.sidebar() {
            Ok(sidebar) => {
                let teams = sidebar.teams.iter().map(|team| (team.team.id.clone(), crate::types::TeamDetails { hidden: team.hidden, rights: team.rights })).collect();
                let channels = sidebar.teams.iter().flat_map(|team| team.channels.iter().map(|channel| (channel.id.clone(), crate::types::ChannelDetails {
                    private: channel.membership_type.as_deref().is_some_and(|kind| kind.eq_ignore_ascii_case("private")),
                    mentions: team.channel_mentions(&channel.id), notifications: team.channel_notifications(&channel.id),
                }))).collect();
                match engine.pinned_channels() {
                    Ok(pinned) => self.publish(Update::TeamMetadata { teams, channels, pinned }),
                    Err(error) => app_backend::diagnostics::record(format!("Rune: pinned channels unavailable: {error}")),
                }
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
                    (chat.id.clone(), crate::types::ChatDetails { importance: engine.unread_importance(&chat.id), desktop_notifications: engine.chat_desktop_notifications(&chat.id), avatar_id, group, members: chat.members.clone(), group_chat: chat.kind == "group", muted: chat.muted, presence })
                }).collect();
                self.publish(Update::Details(details));
                self.publish(Update::Chats(chat_rows(sidebar.chats, Local::now())));
                self.publish(Update::Teams(team_rows(sidebar.teams)));
                self.publish(Update::SectionSettings(teams_core::stored_section_settings(engine.store())));
                match engine.chat_folders() {
                    Ok(folders) => self.publish(Update::Folders(folders)),
                    Err(error) => self.publish(Update::Status(format!("Sections unavailable: {error}"))),
                }
                self.publish(Update::ChatPaging { more: engine.has_older_chats(), loading: engine.is_loading_older_chats() });
                true
            }
            Err(error) => {
                self.publish(Update::Status(format!("Chats unavailable: {error}")));
                false
            }
        }
    }

    pub(crate) fn publish_messages(&self, engine: &Engine, conversation_id: &str) {
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
                let show_receipts = teams_core::stored_chat_settings(engine.store()).settings().read_receipts;
                let states = records.iter().filter(|_| show_receipts).map(|record| (record.message_id.clone(), engine.receipt_state_for(record))).collect();
                self.publish(Update::Receipts { conversation_id: conversation_id.to_owned(), states });
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
        // Converted first, then sent together, so they land in one `poll` and one rebuild.
        let updates: Vec<Update> = ids.iter()
            .filter_map(|id| engine.avatar(id).map(|photo| avatar_update(format!("avatar:{id}"), photo.bytes)))
            .collect();
        for update in updates { self.publish(update); }
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
        let database = database.unwrap_or_else(|| data_path(DATABASE_FILE));
        let folder = accounts::account_folder(&database);
        let store = Arc::new(open_store(&database));
        let transport = endpoint.map_or_else(|| transport(&database, no_sso), |endpoint| Arc::new(session::CdpTransport::new(&endpoint)) as Arc<dyn session::Transport>);
        let shared = Arc::new_cyclic(|weak: &std::sync::Weak<Shared>| {
            let weak = weak.clone();
            let video_weak = weak.clone();
            let intro_weak = weak.clone();
            Shared {
              outbox_lock: tokio::sync::Mutex::new(()),
              fresh_outbox: Mutex::default(),
            switcher_query: Mutex::default(),
            open_threads: Mutex::new(std::collections::HashSet::new()),
            avatars_requested: Arc::default(),
            warmed: Mutex::default(),
            read_only,
            agent_stop: Mutex::new(None),
            calls: Mutex::new(None),
            call_control: crate::calls_backend::Calls::new(read_only, move |update| {
                if let Some(shared) = weak.upgrade() { shared.publish(Update::Call(update)); }
            }, move || { if let Some(shared) = video_weak.upgrade() { shared.publish(Update::VideoReady); } },
            move |intro| {
                let Some(shared) = intro_weak.upgrade() else { return };
                shared.load_recording_rights(intro.conversation.clone().filter(|_| intro.kind == app_call_model::model::CallKind::Meeting));
                shared.publish(Update::CallIntro(intro));
            }),
            message_limit: Mutex::new(50),
            foreground: Mutex::new((None, false)),
            engine: Mutex::new(None),
            open: Mutex::new(None),
            call_chat: Mutex::new(None),
            detached_chats: Mutex::default(),
            closed_chat_windows: Mutex::default(),
            open_generation: AtomicU64::new(0),
            live_retry: Mutex::new(None),
            calendar_nudge: Mutex::new(None),
            updates,
            last_snapshots: Mutex::default(),
            wake: Box::new(wake),
            store: store.clone(),
            folder,
            identity_saved: AtomicBool::new(false),
            transport: transport.clone(),
        }});
        shared.publish(Update::Preferences(Box::new(crate::settings_backend::load(&store))));
        let mut runs = crate::agent_runs::history(&store).unwrap_or_else(|error| { shared.publish(Update::Status(error)); Vec::new() });
        runs.extend(crate::agent_panel::stored_runs(&store));
        shared.publish(Update::AgentRuns(runs));
        let notification_shared = Arc::downgrade(&shared);
        rune_notify::on_click(move |click| {
            if let Some(shared) = notification_shared.upgrade() && let Some(update) = crate::notifications::clicked(&click) {
                shared.publish(update);
            }
        });
        let (activity_sender, activity) = watch::channel(false);
        let events = app_backend::backend::start(store, transport, !read_only, activity, None);
        app_backend::runtime::handle().spawn(forward(shared.clone(), events));
        Bridge {
            shared,
            updates: receiver,
            _activity: activity_sender,
            call_chat: false,
            detached_chat: None,
        }
    }

    pub(crate) fn call_window_actions(&self) -> Self {
        // The main window receives events; the call window only shares its backend actions.
        let (_, updates) = mpsc::channel();
        Self {
            shared: self.shared.clone(),
            updates,
            _activity: self._activity.clone(),
            call_chat: true,
            detached_chat: None,
        }
    }

    pub(crate) fn retry_call(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        let (spec, video) = self.shared.call_control.retry()?;
        let shared = self.shared.clone();
        Some(Arc::new(move || shared.call_control.start_with_video(spec.clone(), video)))
    }

    pub(crate) fn chat_window_actions(&self, id: &str) -> Self {
        let (_, updates) = mpsc::channel();
        Self { shared: self.shared.clone(), updates, _activity: self._activity.clone(), call_chat: false, detached_chat: Some(id.into()) }
    }

    pub(crate) fn close_chat_window(&self) {
        if let Some(id) = &self.detached_chat {
            self.shared.detached_chats.lock().unwrap_or_else(PoisonError::into_inner).remove(id);
            self.shared.closed_chat_windows.lock().unwrap_or_else(PoisonError::into_inner).push(id.clone());
        }
    }

    pub(crate) fn take_closed_chat_windows(&self) -> Vec<String> {
        std::mem::take(&mut *self.shared.closed_chat_windows.lock().unwrap_or_else(PoisonError::into_inner))
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
    fn accounts(&self) -> Vec<accounts::Account> { accounts::Accounts::load(&accounts::default_root()).list }
    fn current_folder(&self) -> Option<PathBuf> { Some(self.shared.folder.clone()) }
    fn sign_in(&self) {
        self.shared.last_snapshots.lock().unwrap_or_else(PoisonError::into_inner).clear();
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            if let Err(error) = shared.transport.sign_in(session::App::Teams).await {
                let message = format!("Could not open the sign-in window: {error}");
                app_backend::diagnostics::record(format!("Rune: {message}"));
                shared.publish(Update::SignInFailed(message));
            }
        });
    }
    fn switch_account(&self, folder: &Path) -> Result<(), String> { switch_to(folder) }
    fn add_account(&self) -> Result<(), String> {
        let folder = accounts::new_folder(&accounts::default_root()).map_err(|error| error.to_string())?;
        switch_to(&folder)
    }
    fn pop_out_chat(&self, id: &str) { self.shared.publish(Update::PopOutChat(id.into())); }
    fn create_meeting(&self, request: u64, mut draft: app_backend::schedule::MeetingDraft) {
        let Some(launcher) = self.shared.launcher().filter(|_| !self.shared.read_only) else {
            self.shared.publish(Update::MeetingCreated { request, result: Err("Not connected or read-only".into()) }); return;
        };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            if draft.time_zone_name.is_none()
                && let Some(mail) = shared.store.meta("me_email").ok().flatten() { draft.time_zone_name = launcher.time_zone_name(&mail).await; }
            let result = launcher.create_meeting(draft).await.map(|_| ());
            shared.publish(Update::MeetingCreated { request, result });
        });
    }
    fn load_inline_file(&self, target: SendTarget, marker: String, path: PathBuf) {
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let result = tokio::task::spawn_blocking(move || {
                use std::io::Read;
                let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
                let mut bytes = Vec::new();
                file.take(crate::inline_compose::MAX_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
                if bytes.len() > crate::inline_compose::MAX_BYTES { return Err("Inline images are limited to 32 MiB".into()); }
                Ok(bytes)
            }).await.map_err(|error| error.to_string()).and_then(|result| result);
            shared.publish(Update::InlineFile { target, marker, name, result });
        });
    }
    fn export_image(&self, bytes: Vec<u8>, rotation: u8, destination: Option<PathBuf>) {
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let saving = destination.is_some();
            let result = tokio::task::spawn_blocking(move || {
                let png = crate::image_viewer::png(&bytes, rotation)?;
                if let Some(destination) = destination {
                    use std::io::Write;
                    let directory = destination.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
                    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
                    file.write_all(&png).map_err(|error| error.to_string())?;
                    file.as_file().sync_all().map_err(|error| error.to_string())?;
                    file.persist(destination).map_err(|error| error.to_string())?;
                    Ok(Vec::new())
                } else { Ok(png) }
            }).await.map_err(|error| error.to_string()).and_then(|result| result);
            if saving {
                shared.publish(Update::Status(match result { Ok(_) => "Image saved".into(), Err(error) => format!("Could not save image: {error}") }));
            } else { shared.publish(Update::ImageCopied(result)); }
        });
    }
    fn load_video(&self, url: &str) { crate::video::load(self.shared.clone(), url.to_owned()); }
    fn library_root(&self, channel: &str) {
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::LibraryRoot { channel: channel.into(), result: Err("Not connected yet".into()) }); return;
        };
        let (shared, channel) = (self.shared.clone(), channel.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let result = engine.channel_library_root(&channel).await.map_err(|error| error.to_string());
            shared.publish(Update::LibraryRoot { channel, result });
        });
    }
    fn library_rows(&self, channel: &str, folder: teams_core::DriveFolder) {
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::LibraryRows { channel: channel.into(), folder, result: Err("Not connected yet".into()) }); return;
        };
        let (shared, channel) = (self.shared.clone(), channel.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let result = engine.library_children(&folder).await.map_err(|error| error.to_string());
            shared.publish(Update::LibraryRows { channel, folder, result });
        });
    }
    fn translate_message(&self, conversation: &str, message: &str, source: String, language: String) {
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::Translated { conversation: conversation.into(), message: message.into(), source, language, result: Err("Not connected yet".into()) }); return;
        };
        let (shared, conversation, message) = (self.shared.clone(), conversation.to_owned(), message.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let result = engine.translate_messages(&conversation, std::slice::from_ref(&message), &language, &chatsvc::TranslationTrigger::OnDemand).await
                .map_err(|error| error.to_string()).and_then(|rows| rows.into_iter().find(|row| row.message_id == message)
                    .and_then(|row| row.content_html).ok_or_else(|| "No translation was returned".into()));
            shared.publish(Update::Translated { conversation, message, source, language, result });
        });
    }
    fn load_language_stamps(&self, conversation: &str) {
        let Some(engine) = self.shared.engine() else { return };
        let (shared, conversation) = (self.shared.clone(), conversation.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let result = engine.language_stamps(&conversation).await.map_err(|error| error.to_string());
            shared.publish(Update::LanguageStamps { conversation, result });
        });
    }
    fn load_reaction_people(&self, conversation: &str, message: &str) {
        let result = self.shared.store.messages_by_id(conversation, &[message.into()])
            .map_err(|error| error.to_string()).and_then(|mut records| records.remove(message).map(|record| teams_core::reactions(&record)).ok_or_else(|| "Message is no longer cached".into()));
        self.shared.publish(Update::ReactionPeople { conversation: conversation.into(), message: message.into(), result });
    }
    fn load_post_followed(&self, conversation: &str, root: &str) {
        let Some(engine) = self.shared.engine() else { return };
        let (shared, conversation, root) = (self.shared.clone(), conversation.to_owned(), root.to_owned());
        app_backend::runtime::handle().spawn(async move {
            match engine.post_follow_states(&conversation, std::slice::from_ref(&root)).await {
                Ok(states) => shared.publish(Update::PostFollowed { conversation, followed: states.get(&root).is_some_and(|state| state.followed()), root }),
                Err(error) => shared.publish(Update::Status(format!("Could not load post follow state: {error}"))),
            }
        });
    }
    fn set_post_followed(&self, conversation: &str, root: &str, followed: bool) {
        let (root, shared) = (root.to_owned(), self.shared.clone());
        self.chat_task(conversation, "follow post", move |engine, conversation| async move {
            engine.set_post_followed(&conversation, &root, followed).await?;
            shared.publish(Update::PostFollowed { conversation, root, followed });
            Ok(())
        });
    }
    fn card_action(&self, scope: crate::card_state::Scope, action: teams_core::CardAction) {
        let Some(engine) = self.shared.engine().filter(|_| !self.shared.read_only) else {
            self.shared.publish(Update::CardAction { scope, result: Err("Not connected or read-only".into()) }); return;
        };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let result = match &action.kind {
                teams_core::CardActionKind::Submit(submit) if scope.task => engine.task_submit(&scope.conversation, &scope.message, submit.data.clone()).await,
                _ => engine.card_action(&scope.conversation, &scope.message, &action).await,
            };
            shared.publish(Update::CardAction { scope, result: result.map_err(|error| error.to_string()) });
        });
    }
    fn refresh_card(&self, scope: crate::card_state::Scope, action: teams_core::ExecuteAction, trigger: teams_core::ExecuteTrigger) {
        let Some(engine) = self.shared.engine().filter(|_| !self.shared.read_only) else {
            self.shared.publish(Update::CardAction { scope, result: Err("Not connected or read-only".into()) }); return;
        };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let result = engine.card_refresh(&scope.conversation, &scope.message, &action, trigger).await;
            shared.publish(Update::CardAction { scope, result: result.map_err(|error| error.to_string()) });
        });
    }
    fn schedule_message(&self, target: SendTarget, text: String, at: chrono::DateTime<Utc>) {
        let Some(engine) = self.shared.engine().filter(|_| !self.shared.read_only) else {
            self.shared.publish(Update::Scheduled { target, text, result: Err("Not connected or read-only".into()) });
            return;
        };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let result = app_base::schedule_time::validate(at.with_timezone(&Local), Local::now()).map_err(|error| error.message().to_owned());
            let result = match result {
                Ok(()) => engine.schedule_message(&target.conversation_id, target.thread_root_id.as_deref(), &crate::composed::html(&text, &[]), at).await.map(|_| ()).map_err(|error| error.to_string()),
                Err(error) => Err(error),
            };
            shared.publish(Update::Scheduled { target, text, result });
        });
    }
    fn load_scheduled(&self) {
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::ScheduledList(Err("Not connected".into()))); return;
        };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            shared.publish(Update::ScheduledList(engine.scheduled_messages().await.map_err(|e| e.to_string())));
        });
    }
    fn load_channel_description(&self, id: &str) {
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::ChannelDescription { id: id.into(), result: Err("Not connected".into()) }); return;
        };
        let (shared, id) = (self.shared.clone(), id.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let result = engine.channel_description(&id).await.map_err(|e| e.to_string());
            shared.publish(Update::ChannelDescription { id, result });
        });
    }
    fn change_channel(&self, id: &str, edit: Option<(String, String)>) {
        let deleted = edit.is_none();
        let Some(engine) = self.shared.engine().filter(|_| !self.shared.read_only) else {
            self.shared.publish(Update::ChannelChanged { id: id.into(), deleted, result: Err("Not connected or read-only".into()) }); return;
        };
        let (shared, id) = (self.shared.clone(), id.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let allowed = engine.sidebar().ok().is_some_and(|sidebar| sidebar.teams.iter().any(|team|
                team.channels.iter().any(|channel| channel.id == id) && (team.rights.owner || if deleted { team.rights.delete_channel } else { team.rights.edit_channel })));
            let result = if !allowed { Err("You do not have permission to change this channel".into()) }
            else if let Some((name, description)) = edit {
                if name.trim().is_empty() || name.trim().chars().count() > teams_core::MAX_CHANNEL_NAME_LENGTH || description.chars().count() > teams_core::MAX_CHANNEL_DESCRIPTION_LENGTH {
                    Err("Invalid channel name or description".into())
                } else { engine.edit_channel(&id, &name, Some(&description)).await.map_err(|e| e.to_string()) }
            } else { engine.delete_channel(&id).await.map_err(|e| e.to_string()) };
            if result.is_ok() { shared.publish_chats(&engine); }
            shared.publish(Update::ChannelChanged { id, deleted, result });
        });
    }
    fn follow_presence(&self, user_id: &str, name: &str, follow: bool) {
        let mut followed = state::presence_follow::load_followed(&self.shared.store);
        followed.retain(|person| person.user_id != user_id);
        if follow { followed.push(state::presence_follow::FollowedPerson { user_id: user_id.into(), name: name.into() }); }
        let result = serde_json::to_string(&followed).map_err(|e| e.to_string()).and_then(|json|
            self.shared.store.set_meta(state::presence_follow::FOLLOWED_META_KEY, &json).map_err(|e| e.to_string()));
        match result {
            Ok(()) => {
                self.shared.publish(Update::FollowedPresence(followed));
                if follow && let Some(engine) = self.shared.engine() {
                    let id = user_id.to_owned();
                    app_backend::runtime::handle().spawn(async move { watch_presence(&engine, &[id]).await; });
                }
            }
            Err(error) => self.shared.publish(Update::Status(format!("Could not save status notifications: {error}"))),
        }
    }
    fn change_scheduled(&self, draft: chatsvc::ScheduledDraft, text: Option<String>) {
        let Some(engine) = self.shared.engine().filter(|_| !self.shared.read_only) else {
            self.shared.publish(Update::ScheduledChanged { id: draft.id, result: Err("Not connected or read-only".into()) }); return;
        };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let result = match text {
                Some(text) if text.trim().is_empty() => Err("Message cannot be empty".into()),
                Some(html) => engine.update_scheduled(&draft, &html).await.map(Some).map_err(|e| e.to_string()),
                None => engine.cancel_scheduled(&draft.id).await.map(|_| None).map_err(|e| e.to_string()),
            };
            shared.publish(Update::ScheduledChanged { id: draft.id, result });
        });
    }
    fn retry_outbox(&self, id: &str) {
        crate::outbox::retry(self.shared.clone(), id);
    }
    fn set_notification_pause(&self, until: Option<DateTime<Utc>>) {
        match self.shared.store.set_meta(app_backend::notify::pause::META_KEY, &until.map(|time| time.to_rfc3339()).unwrap_or_default()) {
            Ok(()) => self.shared.publish(Update::NotificationPause(until)),
            Err(error) => self.shared.publish(Update::Status(format!("Could not pause notifications: {error}"))),
        }
    }
    fn set_work_location(&self, location: Option<(teams_core::WorkLocationKind, DateTime<Utc>)>) {
        self.chat_task("", "set work location", move |engine, _| async move { engine.set_own_work_location(location).await });
    }
    fn search_switcher(&self, session: teams_core::substrate_search::SearchSession, query: String) {
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        let session_id = session.correlation_id().to_owned();
        *shared.switcher_query.lock().unwrap_or_else(PoisonError::into_inner) = (session_id.clone(), query.clone());
        app_backend::runtime::handle().spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if *shared.switcher_query.lock().unwrap_or_else(PoisonError::into_inner) != (session_id.clone(), query.clone()) { return; }
            let result = engine.server_suggestions(&session, &query).await.map_err(|error| error.to_string());
            if let Ok(results) = &result { shared.publish_avatars(&engine, results.people.iter().filter_map(|person| person.user_id.clone()).collect()); }
            shared.publish(Update::SwitcherSearch { session: session_id, query, result });
        });
    }
    fn set_channel_pinned(&self, id: &str, pinned: bool) {
        self.chat_task(id, "pin channel", move |engine, id| async move { engine.set_channel_pinned(&id, pinned).await });
    }
    fn set_channel_shown(&self, id: &str, shown: bool) {
        self.chat_task(id, "change channel visibility", move |engine, id| async move { engine.set_channel_shown(&id, shown).await });
    }
    fn set_team_shown(&self, id: &str, shown: bool) {
        self.chat_task(id, "change team visibility", move |engine, id| async move { engine.set_team_shown(&id, shown).await });
    }
    fn set_channel_notifications(&self, id: &str, notifications: store::ChannelNotifications) {
        self.chat_task(id, "change channel notifications", move |engine, id| async move { engine.set_channel_notifications(&id, notifications).await });
    }
    fn create_channel(&self, team: &str, name: &str, description: &str, private: bool) {
        let (name, description) = (name.to_owned(), description.to_owned());
        self.chat_task(team, "create channel", move |engine, team| async move {
            engine.create_channel(&team, &name, &description, if private { teams_core::ChannelPrivacy::Private } else { teams_core::ChannelPrivacy::Standard }).await
        });
    }
    fn copy_team_link(&self, id: &str, channel: bool) {
        let Some(engine) = self.shared.engine() else { return };
        let (shared, id) = (self.shared.clone(), id.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let result = if channel { engine.channel_link(&id).await } else { engine.team_link(&id).await };
            shared.publish(Update::CopyLink(result.map_err(|error| error.to_string())));
        });
    }
    fn save_message(&self, conversation: &str, message: &str, root: Option<String>) {
        let message = message.to_owned();
        self.chat_task(conversation, "save message", move |engine, id| async move {
            engine.set_saved(&id, &message, root.as_deref(), true).await
        });
    }

    fn copy_message_link(&self, conversation: &str, message: &str) {
        let Some(engine) = self.shared.engine() else { return };
        let (shared, conversation, message) = (self.shared.clone(), conversation.to_owned(), message.to_owned());
        app_backend::runtime::handle().spawn(async move {
            shared.publish(Update::CopyLink(engine.message_link(&conversation, &message).await.map_err(|error| error.to_string())));
        });
    }
    fn change_folders(&self, change: teams_core::FolderChange) {
        self.chat_task("", "change section", move |engine, _| async move { engine.change_folders(change).await });
    }

    fn move_to_folder(&self, id: &str, folder: &str) {
        let folder = folder.to_owned();
        self.chat_task(id, "move chat", move |engine, id| async move { engine.move_to_folder(&id, &folder, None).await });
    }

    fn remove_from_folder(&self, id: &str, folder: &str) {
        let folder = folder.to_owned();
        self.chat_task(id, "remove chat from section", move |engine, id| async move { engine.remove_from_folder(&id, &folder).await });
    }

    fn set_folder_expanded(&self, id: &str, expanded: bool) {
        self.chat_task(id, "change section expansion", move |engine, id| async move {
            engine.set_folder_expanded(&id, expanded).await?;
            engine.store().set_folder_expanded(&id, expanded)?;
            Ok(())
        });
    }

    fn load_older_chats(&self) {
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            if let Err(error) = engine.load_older_chats().await { shared.publish(Update::Status(format!("Could not load older chats: {error}"))); }
            shared.publish_chats(&engine);
        });
    }

    fn set_chat_notifications(&self, id: &str, mode: teams_core::ChatNotificationMode, desktop: teams_core::DesktopNotifications) {
        self.chat_task(id, "change notifications", move |engine, id| async move { engine.set_chat_notifications(&id, mode, desktop).await });
    }

    fn remove_chat_history(&self, id: &str) {
        self.chat_task(id, "remove chat history", move |engine, id| async move { engine.remove_chat_history(&id).await });
    }

    fn load_saved(&self) {
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::Saved(Err("Not connected yet".into())));
            return;
        };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            shared.publish(Update::Saved(engine.list_saved().await.map_err(|error| error.to_string())));
        });
    }

    fn unsave(&self, item: &teams_core::SavedMessage) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let (shared, item) = (self.shared.clone(), item.clone());
        app_backend::runtime::handle().spawn(async move {
            match engine.set_saved(&item.conversation_id, &item.message_id, Some(&item.root_id), false).await {
                Ok(()) => shared.publish(Update::Saved(engine.list_saved().await.map_err(|error| error.to_string()))),
                Err(error) => shared.publish(Update::Status(format!("Could not remove saved message: {error}"))),
            }
        });
    }

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

    fn run_agent(&self, id: &str, prompt: &str, working_dir: &str) {
        crate::agent_runs::start(self.shared.clone(), id, prompt, working_dir, None);
    }
    fn run_agent_action(&self, id: &str, prompt: &str, working_dir: &str, session: &str) {
        crate::agent_runs::start(self.shared.clone(), id, prompt, working_dir, Some((id, session)));
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
    fn create_chat(&self, user_ids: Vec<String>, topic: Option<String>, first_message: String) {
        if self.shared.read_only || user_ids.is_empty() {
            self.shared.publish(Update::ChatCreateFailed("Read-only mode or no recipients".into())); return;
        }
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::ChatCreateFailed("Not connected yet".into())); return;
        };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let made = match user_ids.as_slice() {
                [one] => engine.create_one_on_one(one).await,
                many => engine.create_group(many, topic.as_deref()).await,
            };
            match made {
                Ok(id) => {
                    if !first_message.is_empty() { crate::outbox::enqueue(shared.clone(), SendTarget { conversation_id: id.clone(), thread_root_id: None, channel: false }, crate::types::Composed { text: first_message, ..Default::default() }, vec![]); }
                    shared.publish_chats(&engine); shared.publish(Update::ChatCreated(id));
                }
                Err(error) => shared.publish(Update::ChatCreateFailed(format!("Could not create chat: {error}"))),
            }
        });
    }

    fn start_chat_call(&self, id: &str, video: bool) {
        if self.shared.read_only { return; }
        self.call_chat(id, video);
    }

    fn meet_now(&self, id: &str) {
        if self.shared.read_only { return; }
        if !id.is_empty() { return self.call_chat(id, true); }
        let launcher = self.shared.calls.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(launcher) = launcher else { self.shared.publish(Update::MeetingReady(Err("Not connected yet".into()))); return };
        let subject = self.shared.engine().and_then(|engine| engine.store().chat(id).ok().flatten()).map(|chat| chat.title)
            .filter(|title| !title.trim().is_empty()).unwrap_or_else(|| "Meeting".into());
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            shared.publish(Update::MeetingReady(launcher.create_meet_now(&subject).await));
        });
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
            } else {
                shared.publish(Update::Deleted { conversation_id: conversation_id.clone(), message_id });
                if shared.is_open(&conversation_id) { shared.publish_messages(&engine, &conversation_id); }
            }
        });
    }
    fn undo_delete_message(&self, conversation_id: &str, message_id: &str) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        let shared = self.shared.clone();
        let (conversation_id, message_id) = (conversation_id.to_owned(), message_id.to_owned());
        app_backend::runtime::handle().spawn(async move {
            match engine.undo_soft_delete_message(&conversation_id, &message_id).await {
                Ok(()) => {
                    shared.publish(Update::Restored { conversation_id: conversation_id.clone(), message_id });
                    if shared.is_open(&conversation_id) { shared.publish_messages(&engine, &conversation_id); }
                }
                Err(error) => shared.publish(Update::Status(format!("Message restore failed. Please try again: {error}"))),
            }
        });
    }
    fn retry_live(&self) {
        if let Some(retry) = &*self.shared.live_retry.lock().unwrap_or_else(PoisonError::into_inner) {
            retry.notify_one();
        }
    }
    fn edit_message(&self, conversation_id: &str, message_id: &str, html: &str) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::EditFinished { conversation_id: conversation_id.to_owned(), message_id: message_id.to_owned(), result: Err("Not connected yet".into()) });
            return;
        };
        let shared = self.shared.clone();
        let (conversation_id, message_id, html) = (conversation_id.to_owned(), message_id.to_owned(), html.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let result: Result<(), String> = async {
                let record = engine.store().messages_by_id(&conversation_id, std::slice::from_ref(&message_id))
                    .map_err(|error| error.to_string())?.remove(&message_id)
                    .ok_or_else(|| "Original message is not cached".to_owned())?;
                let mentions = teams_core::user_mention_inputs(&record);
                engine.edit_message_html(&conversation_id, &message_id, &html, &mentions).await.map_err(|error| error.to_string())
            }.await;
            let saved = result.is_ok();
            shared.publish(Update::EditFinished { conversation_id: conversation_id.clone(), message_id, result });
            if saved && shared.is_open(&conversation_id) { shared.publish_messages(&engine, &conversation_id); }
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
    fn forward_message(&self, source_id: &str, target_id: &str, message_ids: &[String], comment: &str) {
        let id = message_ids.first().cloned().unwrap_or_default();
        if self.shared.read_only || message_ids.is_empty() || message_ids.len() > 5 {
            self.shared.publish(Update::Forwarded { conversation_id: source_id.into(), message_id: id, result: Err("Select one to five messages in a writable conversation".into()) }); return;
        }
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::Forwarded { conversation_id: source_id.into(), message_id: id, result: Err("Not connected yet".into()) }); return;
        };
        let (shared, source, target, ids, comment) = (self.shared.clone(), source_id.to_owned(), target_id.to_owned(), message_ids.to_vec(), comment.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let result = engine.forward_messages(&source, &target, &ids, &comment).await.map(|_| ()).map_err(|error| error.to_string());
            shared.publish(Update::Forwarded { conversation_id: source, message_id: id, result });
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
            match calls.respond_to_event(&id, ical_uid.as_deref(), reply, send_response, "").await {
                Ok(()) => {
                    if let Some(nudge) = &*shared.calendar_nudge.lock().unwrap_or_else(PoisonError::into_inner) { let _ = nudge.send(()); }
                    shared.publish(Update::CalendarRefresh);
                }
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
        app_backend::appearance::save(&self.shared.store, &settings);
    }
    fn save_setting(&self, change: crate::settings::Change) { crate::settings_backend::save(&self.shared, change); }
    fn write_reminder(&self, event_id: &str, snooze_until: Option<DateTime<Utc>>) {
        if self.shared.read_only { return; }
        let Some(launcher) = self.shared.launcher() else { return };
        let event_id = event_id.to_owned();
        app_backend::runtime::handle().spawn(async move {
            let written = match snooze_until {
                Some(until) => launcher.snooze_reminder(&event_id, until).await,
                None => launcher.dismiss_reminder(&event_id).await,
            };
            if let Err(error) = written { app_backend::diagnostics::record(format!("Rune: Outlook reminder not saved: {error}")); }
        });
    }
    fn save_agent_settings(&self, settings: agents::AgentSettings) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        if let Err(error) = engine.store().set_meta(agents::SETTINGS_KEY, &settings.to_json()) {
            app_backend::diagnostics::record(format!("Rune: agent settings not saved: {error}"));
        }
    }
    fn load_reminders(&self, start: DateTime<Utc>, end: DateTime<Utc>) {
        let launcher = self.shared.launcher();
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            let result = match launcher {
                Some(launcher) => launcher.reminder_view(start, end).await,
                None => Err("calling is not connected yet".to_owned()),
            };
            if let Err(error) = &result { app_backend::diagnostics::record(format!("Rune: Outlook reminders not read: {error}")); }
            shared.publish(Update::OutlookReminders { start, end, result });
        });
    }
    fn load_live_meeting(&self, thread_id: &str) {
        let Some(launcher) = self.shared.launcher() else { return };
        let (shared, thread_id) = (self.shared.clone(), thread_id.to_owned());
        app_backend::runtime::handle().spawn(async move {
            let meeting = launcher.live_meeting(&thread_id).await;
            shared.publish(Update::LiveMeeting { thread_id, meeting });
        });
    }
    fn load_devices(&self) { crate::settings_backend::list_devices(self.shared.clone()); }
    fn accept_call(&self, ring_id: u64, video: bool) {
        self.shared.call_control.accept(ring_id, video);
        self.shared.load_recording_rights(None);
    }
    fn decline_call(&self, ring_id: u64) { self.shared.call_control.decline(ring_id); }
    fn load_image(&self, image: teams_core::ImageRef) { crate::media::load_image(self.shared.clone(), image); }
    fn warm_avatars(&self, ids: Vec<String>) {
        if let Some(engine) = self.shared.engine() {
            self.shared.publish_avatars(&engine, ids);
        }
    }
    fn download_file(&self, url: String, path: PathBuf) { crate::media::download_file(self.shared.clone(), url, path); }
    fn join_call(&self, url: &str, video: bool) { self.shared.call_control.join(url, video); }
    fn calling_ready(&self) -> bool { self.shared.calls.lock().unwrap_or_else(PoisonError::into_inner).is_some() }
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
        self.load_calendar_range(week, week + chrono::Duration::days(7));
    }
    fn load_calendar_range(&self, week: chrono::NaiveDate, last: chrono::NaiveDate) {
        let calls = self.shared.calls.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(calls) = calls else {
            self.shared.publish(Update::Status("Calendar is not connected yet".into()));
            return;
        };
        let Some(start) = week.and_hms_opt(0, 0, 0).and_then(|time| Local.from_local_datetime(&time).earliest()) else { return };
        let Some(end) = last.and_hms_opt(0, 0, 0).and_then(|time| Local.from_local_datetime(&time).earliest()) else { return };
        let shared = self.shared.clone();
        app_backend::runtime::handle().spawn(async move {
            match calls.calendar_view(start.with_timezone(&Utc), end.with_timezone(&Utc)).await {
                Ok(events) => shared.publish(Update::CalendarRange { start: week, end: last, rows: events.into_iter().map(|event| crate::screens::CalendarRow {
                    id: event.object_id, subject: event.subject, start: event.start, end: event.end,
                    cancelled: event.cancelled, all_day: event.all_day, organizer: event.organizer_name,
                    ical_uid: event.ical_uid, join_url: event.join_url, dial_in: event.dial_in, thread_id: event.thread_id, response: event.response,
                }).collect() }),
                Err(error) => shared.publish(Update::Status(format!("Calendar unavailable: {error}"))),
            }
        });
    }

    fn load_agenda(&self) {
        if let Some(nudge) = &*self.shared.calendar_nudge.lock().unwrap_or_else(PoisonError::into_inner)
            && nudge.send(()).is_ok()
        { return; }
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
        if let Some(id) = &self.detached_chat {
            if id != conversation_id { self.shared.publish(Update::OpenChat(conversation_id.into())); return; }
            self.shared.detached_chats.lock().unwrap_or_else(PoisonError::into_inner).insert(id.clone());
            return self.shared.refresh_separate_chat(id);
        }
        if self.call_chat { return self.shared.open_call_chat(conversation_id); }
        if self.shared.detached_chats.lock().unwrap_or_else(PoisonError::into_inner).contains(conversation_id) { self.shared.publish(Update::PopOutChat(conversation_id.into())); return; }
        let generation = self.shared.open_generation.fetch_add(1, Ordering::Relaxed) + 1;
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
        // Capture the boundary before mark_read advances the stored horizon.
        let first_unread = engine.first_unread_message_id(conversation_id);
        let unread_chat = engine.store().chat(conversation_id).ok().flatten().filter(|chat| chat.unread);
        self.shared.publish(Update::FirstUnread { conversation_id: conversation_id.to_owned(), message_id: first_unread.clone() });
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
                if let Err(error) = engine.refresh_receipts(&conversation_id).await {
                    app_backend::diagnostics::record(format!("Rune: read receipts unavailable: {error}"));
                }
                if shared.is_open(&conversation_id) { shared.publish_messages(&engine, &conversation_id); }
                if let Ok(Some(chat)) = engine.store().chat(&conversation_id) {
                    let ids: Vec<_> = chat.members.iter().filter(|member| !member.is_bot()).filter_map(|member| member.user_id.clone()).collect();
                    watch_presence(&engine, &ids).await;

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
                        if shared.open_generation.load(Ordering::Relaxed) == generation
                            && first_unread.is_none() && let Some(chat) = unread_chat
                        {
                            let me = engine.store().meta(META_USER_ID).ok().flatten().unwrap_or_default();
                            let message_id = engine.store().first_message_after_from_others(&conversation_id, chat.last_read_at, &me).ok().flatten();
                            shared.publish(Update::FirstUnread { conversation_id: conversation_id.clone(), message_id });
                        }
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

    fn send_typing(&self, target: SendTarget, active: bool) {
        if self.shared.read_only { return; }
        let Some(engine) = self.shared.engine() else { return };
        app_backend::runtime::handle().spawn(async move {
            if let Err(error) = engine.send_typing(&target.conversation_id, target.thread_root_id.as_deref(), active).await {
                app_backend::diagnostics::record(format!("Could not send typing: {error}"));
            }
        });
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
            let result: teams_core::Result<Vec<_>> = async {
                Ok(match &place {
                    Place::Recent => engine.picker_recent().await?.into_iter().map(|(file, owner)| entry(file, owner)).collect(),
                    Place::MyFiles => engine.picker_folder(&engine.picker_root(None).await?).await?,
                    Place::Shared => engine.picker_shared().await?,
                    Place::Folder(folder) => engine.picker_folder(folder).await?,
                    Place::Search(query) => engine.search_cloud_files(&teams_core::substrate_search::SearchSession::new(), query).await?
                        .into_iter().map(|file| entry(file, None)).collect(),
                })
            }.await;
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
    fn save_composition(&self, id: &str, composed: &crate::types::Composed, files: &[teams_core::FileReference]) -> Result<(), String> {
        match crate::composed::composition_record(id, composed, files)? {
            Some(record) => self.shared.store.save_draft(&record),
            None => self.shared.store.delete_draft(id),
        }.map_err(|error| error.to_string())
    }
    fn load_composition(&self, id: &str) -> Option<(crate::types::Composed, Vec<teams_core::FileReference>)> {
        crate::composed::composition(&self.shared.store.draft(id).ok()??)
    }

    fn load_draft(&self, conversation_id: &str) -> Option<String> {
        let record = self.shared.engine()?.store().draft(conversation_id).ok()??;
        crate::composed::draft_text(&record.payload)
    }
}

/// The people a chat call rings: everyone but me and bots, for 1:1 and group chats only.
fn chat_callees(chat: &ChatRecord, me: &str) -> Result<Vec<calling::Callee>, &'static str> {
    if me.is_empty() || teams_core::is_notes_chat(&chat.id)
        || !(chat.kind.eq_ignore_ascii_case("oneOnOne") || chat.kind.eq_ignore_ascii_case("group")) {
        return Err("This chat is not available for calling");
    }
    let callees: Vec<calling::Callee> = chat.members.iter().filter(|m| !m.is_bot()).filter_map(|m| {
        let user = m.user_id.as_deref().filter(|user| *user != me)?;
        Some(calling::Callee { mri: format!("8:orgid:{user}"), display_name: m.display_name.clone() })
    }).collect();
    if callees.is_empty() { Err("This chat has no callable participants") } else { Ok(callees) }
}

impl Bridge {
    /// Rings the chat's members, with video or audio only, like the GPUI header call buttons.
    fn call_chat(&self, id: &str, video: bool) {
        let Some(engine) = self.shared.engine() else { self.shared.publish(Update::Status("Not connected yet".into())); return };
        match engine.store().chat(id) {
            Ok(Some(chat)) => {
                let me = engine.store().meta(META_USER_ID).ok().flatten().unwrap_or_default();
                match chat_callees(&chat, &me) {
                    Ok(callees) => self.shared.call_control.start_with_video(calling::CallSpec::People { callees, thread_id: id.to_owned() }, video),
                    Err(why) => self.shared.publish(Update::Status(why.into())),
                }
            }
            Ok(None) => self.shared.publish(Update::Status("Chat is not available yet".into())),
            Err(error) => self.shared.publish(Update::Status(format!("Could not start call: {error}"))),
        }
    }

    fn send_to(&self, target: SendTarget, composed: crate::types::Composed, files: Vec<teams_core::FileReference>) {
        crate::outbox::enqueue(self.shared.clone(), target, composed, files);
    }

}

/// Makes `folder` the most recent account, then starts this exe on it. The caller quits.
fn switch_to(folder: &Path) -> Result<(), String> {
    let root = accounts::default_root();
    let before = accounts::Accounts::load(&root);
    let mut registry = before.clone();
    registry.touch(folder, Utc::now().timestamp());
    registry.save(&root).map_err(|error| error.to_string())?;
    accounts::relaunch(std::env::args_os().skip(1)).map_err(|error| {
        // Keep the previous account as the one the next launch opens.
        if let Err(restore) = before.save(&root) {
            app_backend::diagnostics::record(format!("Could not restore the account list: {restore}"));
        }
        error.to_string()
    })
}

/// Writes the signed-in identity into the account registry, once per run.
fn remember_account(shared: &Shared) {
    if shared.identity_saved.swap(true, Ordering::Relaxed) {
        return;
    }
    let email = shared.store.meta("me_email").ok().flatten().unwrap_or_default();
    let name = shared.store.meta("me_display_name").ok().flatten().unwrap_or_default();
    let root = accounts::default_root();
    let mut registry = accounts::Accounts::load(&root);
    // set_identity ignores unlisted folders, so the folder is listed first.
    registry.touch(&shared.folder, Utc::now().timestamp());
    registry.set_identity(&shared.folder, &email, &name);
    if let Err(error) = registry.save(&root) {
        app_backend::diagnostics::record(format!("Could not save the account list: {error}"));
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
    let mut urgent_tick = tokio::time::interval(std::time::Duration::from_secs(10));
    urgent_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let event = tokio::select! {
            event = events.recv() => match event { Some(event) => event, None => break },
            _ = urgent_tick.tick() => {
                if let Some(engine) = shared.engine() {
                    let (foreground, in_call) = shared.foreground.lock().unwrap_or_else(PoisonError::into_inner).clone();
                    notifications.repeat_urgent(&engine, foreground.as_deref(), in_call);
                }
                continue;
            }
        };
        match event {
            BackendEvent::Live(state) => shared.publish(Update::Live(state)),
            BackendEvent::LiveRetry(retry) => {
                *shared.live_retry.lock().unwrap_or_else(PoisonError::into_inner) = Some(retry);
                shared.publish(Update::LiveRetryReady);
            }
            BackendEvent::Synced(at) => shared.publish(Update::Synced(at)),
            BackendEvent::Calendar(events) => {
                shared.publish(Update::Agenda(events));
                shared.publish(Update::CalendarRefresh);
            }
            BackendEvent::CalendarNudge(nudge) => {
                *shared.calendar_nudge.lock().unwrap_or_else(PoisonError::into_inner) = Some(nudge);
            }
            BackendEvent::Favorites(ids) => shared.publish(Update::Favorites(ids)),
            BackendEvent::Typing(event) => shared.publish(Update::Typing(event)),
            BackendEvent::Connection(state) => {
                if state == ConnectionState::Online && let Some(engine) = shared.engine() {
                    shared.publish_chats(&engine);
                    refresh_me(&shared, &engine);
                    remember_account(&shared);
                }
                let expired = state == ConnectionState::SessionExpired;
                shared.publish(Update::Status(status_text(&state)));
                if expired { shared.publish(Update::SessionExpired); }
            }
            BackendEvent::Engine(engine) => {
                *shared.engine.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(engine.clone());
                crate::outbox::publish(&shared);
                crate::outbox::resume(shared.clone());
                // The engine exists only after a session, and its cached chats are shown now.
                // The backend's own Online comes after its first sidebar refresh (and folders),
                // so set it here; later Connection events, sent after this one, still override it.
                shared.publish_chats(&engine);
                refresh_me(&shared, &engine);
                let pending = shared.open.lock().unwrap_or_else(PoisonError::into_inner).clone();
                if let Some(id) = pending { shared.publish(Update::OpenChat(id)); }
                if let Some(activity) = teams_core::stored_activity_feed(engine.store()) { shared.publish(Update::Activity(activity)); }
                shared.publish(Update::Appearance(app_backend::appearance::load(engine.store())));
                let paused_until = app_backend::notify::pause::load(engine.store());
                shared.publish(Update::NotificationPause(paused_until));
                let settings = engine.store().meta(agents::SETTINGS_KEY).ok().flatten()
                    .and_then(|json| agents::AgentSettings::from_json(&json).ok()).unwrap_or_default();
                shared.publish(Update::AgentSettings(settings));
                crate::agent_runs::resume(shared.clone());
                crate::settings_backend::refresh(shared.clone(), engine.clone());
                shared.publish(Update::FollowedPresence(state::presence_follow::load_followed(engine.store())));
                app_backend::runtime::handle().spawn(async move {
                    if let Ok(sidebar) = engine.sidebar() {
                        let mut ids: Vec<_> = sidebar.chats.iter().flat_map(|c| &c.members).filter(|m| !m.is_bot()).filter_map(|m| m.user_id.clone()).collect();
                        ids.extend(state::presence_follow::load_followed(engine.store()).into_iter().map(|person| person.user_id));
                        watch_presence(&engine, &ids).await;

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
                        let updates: Vec<Update> = user_ids.iter()
                            .filter_map(|id| engine.avatar(id).map(|photo| avatar_update(format!("avatar:{id}"), photo.bytes)))
                            .collect();
                        for update in updates { shared.publish(update); }
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
                    CoreEvent::SidebarChanged | CoreEvent::FoldersChanged => {
                        shared.publish_chats(&engine);
                    }
                    CoreEvent::PresenceChanged => { notifications.presence_changed(&engine); shared.publish_chats(&engine); }
                    CoreEvent::PresenceStatusChanged(status) => shared.publish(Update::OwnStatus(status)),
                    CoreEvent::PinsChanged { conversation_id } => {
                        if shared.is_open(&conversation_id) { refresh_pins(&shared, &engine, &conversation_id).await; }
                    }
                    CoreEvent::ReceiptsChanged { conversation_id } => {
                        if shared.is_open(&conversation_id) { shared.publish_messages(&engine, &conversation_id); }
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
                        if notifications.messages(&engine, &conversation_id, environment) { shared.publish(Update::Mentioned(conversation_id.clone())); }
                        if shared.is_open(&conversation_id) { shared.publish_messages(&engine, &conversation_id); }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Subscribes to and reads the presence of `ids`. As GPUI, a failure goes to diagnostics, not
/// to a status toast that would repeat on every chat open.
async fn watch_presence(engine: &Engine, ids: &[String]) {
    if let Err(error) = engine.watch_presence(ids).await { app_backend::diagnostics::record(format!("Rune: presence subscription unavailable: {error}")); }
    if let Err(error) = engine.refresh_presence(ids).await { app_backend::diagnostics::record(format!("Rune: presence unavailable: {error}")); }
}

async fn refresh_pins(shared: &Shared, engine: &Engine, id: &str) {
    match engine.chat_pins(id).await {
        Ok(pins) => {
            let mut rows = Vec::with_capacity(pins.len());
            for pin in pins {
                let cached = engine.store().messages_by_id(id, std::slice::from_ref(&pin.message_id)).ok().and_then(|mut rows| rows.remove(&pin.message_id));
                let record = if cached.is_some() { cached } else {
                    match engine.refresh_message(id, &pin.message_id).await {
                        Ok(record) => record,
                        Err(error) => { shared.publish(Update::Status(format!("Pinned message unavailable: {error}"))); None }
                    }
                };
                rows.push(match record {
                    Some(record) => crate::types::PinnedRow {
                        message_id: pin.message_id.clone(),
                        conversation_id: id.to_owned(), sender: record.sender_name.unwrap_or_default(),
                        text: if record.deleted { DELETED_TEXT.to_owned() } else { html_to_text(&record.body_html) },
                    },
                    _ => crate::types::PinnedRow { message_id: pin.message_id.clone(), conversation_id: id.to_owned(), sender: String::new(), text: format!("Message {}", pin.message_id) },
                });
            }
            shared.publish(Update::Pinned { conversation_id: id.to_owned(), pins: rows });
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
        ConnectionState::SessionExpired => "Session expired".into(),
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
            forwarded: teams_core::is_forwarded(record),
            importance: if record.deleted { Default::default() } else { record.importance },
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

/// Sidebar updates that replace their whole slice of the view, so an identical one changes nothing.
fn is_snapshot(update: &Update) -> bool {
    matches!(update, Update::Chats(_) | Update::Details(_) | Update::Teams(_) | Update::TeamMetadata { .. }
        | Update::SectionSettings(_) | Update::Folders(_) | Update::ChatPaging { .. })
}

/// Decoding every avatar at its 240 px source with mips cost about 0.6 MB each, a gigabyte for
/// a large tenant. 128 px keeps call tiles (up to 96 pt) acceptable at about 90 KB each.
/// Falls back to the bytes for formats this can't decode (a bot's SVG icon).
const AVATAR_PX: u32 = 128;

fn avatar_update(key: String, bytes: Vec<u8>) -> Update {
    let Ok(image) = ::image::load_from_memory(&bytes) else { return Update::Image { key, bytes } };
    // The centre square: avatars draw in a circle.
    let side = image.width().min(image.height());
    let square = image.crop_imm((image.width() - side) / 2, (image.height() - side) / 2, side, side);
    let small = if side > AVATAR_PX { square.thumbnail(AVATAR_PX, AVATAR_PX) } else { square }.into_rgba8();
    let (width, height) = small.dimensions();
    Update::ImagePreview { key, bytes: Vec::new(), preview: Some((width, height, small.into_raw())) }
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
        log: |message| app_backend::diagnostics::record(message),
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
    fn chat_calls_ring_the_other_members() {
        let member = |id: &str| store::MemberRecord { user_id: Some(id.into()), display_name: id.into() };
        let group = ChatRecord { kind: "group".into(), members: vec![member("me"), member("ann"), member("28:bot")], ..chat("19:g", None) };
        let callees = chat_callees(&group, "me").expect("group is callable");
        assert_eq!(callees.iter().map(|c| c.mri.as_str()).collect::<Vec<_>>(), ["8:orgid:ann"]);
        assert!(chat_callees(&ChatRecord { kind: "meeting".into(), ..group }, "me").is_err());
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
    fn message_rows_keep_forward_and_importance_metadata_but_hide_it_for_deletes() {
        let mut record = MessageRecord {
            body_html: "<blockquote itemtype=\"http://schema.skype.com/Forward\">Forwarded</blockquote>".into(),
            importance: store::MessageImportance::Urgent,
            ..Default::default()
        };
        let rows = message_rows(std::slice::from_ref(&record), "me", Local::now());
        assert!(rows[0].forwarded);
        assert_eq!(rows[0].importance, store::MessageImportance::Urgent);
        record.deleted = true;
        let rows = message_rows(&[record], "me", Local::now());
        assert!(!rows[0].forwarded);
        assert_eq!(rows[0].importance, store::MessageImportance::Normal);
    }

    #[test]
    fn avatars_shrink_to_a_square_of_avatar_px() {
        let mut png = Vec::new();
        ::image::DynamicImage::new_rgba8(300, 240).write_to(&mut std::io::Cursor::new(&mut png), ::image::ImageFormat::Png).unwrap();
        let Update::ImagePreview { preview: Some((w, h, rgba)), bytes, .. } = avatar_update("avatar:a".into(), png) else { panic!("not shrunk") };
        assert_eq!((w, h, rgba.len()), (AVATAR_PX, AVATAR_PX, (AVATAR_PX * AVATAR_PX * 4) as usize));
        assert!(bytes.is_empty(), "the source is not kept");
        assert!(matches!(avatar_update("avatar:b".into(), b"<svg/>".to_vec()), Update::Image { .. }));
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
