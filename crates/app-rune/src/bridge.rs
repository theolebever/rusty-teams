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
    pub(crate) read_only: bool,
    calls: Mutex<Option<Arc<app_backend::launcher::CallLauncher>>>,
    call_control: crate::calls_backend::Calls,
    message_limit: Mutex<usize>,
    foreground: Mutex<(Option<String>, bool)>,
    engine: Mutex<Option<Arc<Engine>>>,
    open: Mutex<Option<String>>,
    updates: Sender<Update>,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    pub(crate) fn publish(&self, update: Update) {
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
            Ok(records) => {
                let me = engine
                    .store()
                    .meta(META_USER_ID)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let rows = message_rows(&records, &me, Local::now());
                self.publish(Update::Messages {
                    conversation_id: conversation_id.to_owned(),
                    rows,
                });
            }
            Err(error) => self.publish(Update::Status(format!("Messages unavailable: {error}"))),
        }
    }
}

impl Bridge {
    pub fn shutdown(&self) { self.shared.call_control.hangup(); }
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
            read_only,
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

    pub fn set_activity(&self, active: bool) {
        self._activity.send_if_modified(|value| { if *value == active { false } else { *value = active; true } });
    }

    pub fn set_foreground(&self, chat: Option<&str>, in_call: bool) {
        *self.shared.foreground.lock().unwrap_or_else(PoisonError::into_inner) = (chat.map(str::to_owned), in_call);
    }
}

impl Actions for Bridge {
    fn open_thread(&self, conversation_id: &str, root: &str) {
        let Some(engine) = self.shared.engine() else { return };
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
        let shared = self.shared.clone();
        let conversation_id = conversation_id.to_owned();
        app_backend::runtime::handle().spawn(async move {
            if !shared.read_only {
                if let Err(error) = engine.mark_read(&conversation_id).await {
                    shared.publish(Update::Status(format!("Could not mark conversation read: {error}")));
                }
            }
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
        self.send_to(target, text, files);
    }
}

impl Bridge {
    fn send_to(&self, target: SendTarget, text: String, files: Vec<teams_core::FileReference>) {
        if text.trim().is_empty() && files.is_empty() {
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
            let html = teams_core::markdown_to_html(&text);
            let result = if let Some(root) = &target.thread_root_id {
                engine.send_message_with_extras(conversation_id, &html, Some(root), &[], &extras).await
            } else if target.channel {
                engine.post_to_channel_with_extras(conversation_id, &html, None, &[], &extras).await
            } else {
                engine.send_message_with_extras(conversation_id, &html, None, &[], &extras).await
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

async fn forward(shared: Arc<Shared>, mut events: tokio_mpsc::UnboundedReceiver<BackendEvent>) {
    let mut notifications = crate::notifications::Notifications::new();
    while let Some(event) = events.recv().await {
        match event {
            BackendEvent::Connection(state) => shared.publish(Update::Status(status_text(&state))),
            BackendEvent::Engine(engine) => {
                *shared.engine.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(engine.clone());
                // The engine exists only after a session, and its cached chats are shown now.
                // The backend's own Online comes after its first sidebar refresh (and folders),
                // so set it here; later Connection events, sent after this one, still override it.
                if shared.publish_chats(&engine) {
                    shared.publish(Update::Status(status_text(&ConnectionState::Online)));
                }
                if let Some(activity) = teams_core::stored_activity_feed(engine.store()) { shared.publish(Update::Activity(activity)); }
                shared.publish(Update::Appearance(app_backend::appearance::load(engine.store())));
            }
            BackendEvent::Calls(calls) => {
                shared.call_control.set_launcher(calls.clone());
                *shared.calls.lock().unwrap_or_else(PoisonError::into_inner) = Some(calls);
            }
            BackendEvent::Ring(event) => shared.publish(Update::Ring(event)),
            BackendEvent::Core(event) => {
                let Some(engine) = shared.engine() else {
                    continue;
                };
                match event {
                    CoreEvent::ActivityChanged => {
                        if let Some(activity) = teams_core::stored_activity_feed(engine.store()) { shared.publish(Update::Activity(activity)); }
                    }
                    CoreEvent::SidebarChanged => {
                        shared.publish_chats(&engine);
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
    chats.sort_by(|a, b| b.last_message_at.cmp(&a.last_message_at));
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
                .filter(|channel| !team.hidden_channel_ids.contains(&channel.id))
                .map(|channel| ChannelRow {
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
                html_to_text(&record.body_html)
            },
            mine: !my_id.is_empty() && record.sender_id.as_deref() == Some(my_id),
            media: crate::media::Content::from_record(record),
        })
        .collect()
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
