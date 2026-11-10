//! Backend bridge for the Rune view. Starts the Teams backend on the shared tokio runtime and
//! turns its events into `Update`s that the view drains once per frame.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use state::backend::{BackendEvent, ConnectionState, Engine};
use chrono::{DateTime, Local, Utc};
use scraper::{Html, Node};
use store::{ChatRecord, MessageRecord, Store};
use teams_core::CoreEvent;
use tokio::sync::{mpsc as tokio_mpsc, watch};

use crate::types::{Actions, ChatRow, MessageRow, Update};

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
struct Shared {
    engine: Mutex<Option<Arc<Engine>>>,
    open: Mutex<Option<String>>,
    updates: Sender<Update>,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn publish(&self, update: Update) {
        let _ = self.updates.send(update);
        (self.wake)();
    }

    fn engine(&self) -> Option<Arc<Engine>> {
        self.engine.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn is_open(&self, conversation_id: &str) -> bool {
        self.open.lock().unwrap_or_else(PoisonError::into_inner).as_deref() == Some(conversation_id)
    }

    fn publish_chats(&self, engine: &Engine) {
        match engine.sidebar() {
            Ok(sidebar) => self.publish(Update::Chats(chat_rows(sidebar.chats, Local::now()))),
            Err(error) => self.publish(Update::Status(format!("Chats unavailable: {error}"))),
        }
    }

    fn publish_messages(&self, engine: &Engine, conversation_id: &str) {
        match engine.open_conversation(conversation_id) {
            Ok(records) => {
                let me = engine.store().meta(META_USER_ID).ok().flatten().unwrap_or_default();
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
    /// Starts the backend. `wake` is called (from any thread) after every new `Update` so the UI redraws.
    pub fn start(
        database: Option<PathBuf>,
        no_sso: bool,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Bridge {
        let (updates, receiver) = mpsc::channel();
        let shared = Arc::new(Shared {
            engine: Mutex::new(None),
            open: Mutex::new(None),
            updates,
            wake: Box::new(wake),
        });
        let database = database.unwrap_or_else(|| data_path(DATABASE_FILE));
        let store = Arc::new(open_store(&database));
        let transport = transport(&database, no_sso);
        let (activity_sender, activity) = watch::channel(true);
        let events = state::backend::start(store, transport, false, activity);
        app_base::runtime::handle().spawn(forward(shared.clone(), events));
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
}

impl Actions for Bridge {
    fn open(&self, conversation_id: &str) {
        *self.shared.open.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(conversation_id.to_owned());
        let Some(engine) = self.shared.engine() else {
            return;
        };
        self.shared.publish_messages(&engine, conversation_id);
        let shared = self.shared.clone();
        let conversation_id = conversation_id.to_owned();
        app_base::runtime::handle().spawn(async move {
            match engine.fetch_newer(&conversation_id).await {
                Ok(_) => {
                    if shared.is_open(&conversation_id) {
                        shared.publish_messages(&engine, &conversation_id);
                    }
                }
                Err(error) => {
                    shared.publish(Update::Status(format!("Could not refresh messages: {error}")))
                }
            }
        });
    }

    fn send(&self, conversation_id: &str, text: String) {
        if text.trim().is_empty() {
            return;
        }
        let Some(engine) = self.shared.engine() else {
            self.shared.publish(Update::SendFailed("Not connected yet".into()));
            return;
        };
        let shared = self.shared.clone();
        let conversation_id = conversation_id.to_owned();
        app_base::runtime::handle().spawn(async move {
            match engine.send_message(&conversation_id, &text, None).await {
                Ok(_) => shared.publish_messages(&engine, &conversation_id),
                Err(error) => shared.publish(Update::SendFailed(error.to_string())),
            }
        });
    }
}

async fn forward(shared: Arc<Shared>, mut events: tokio_mpsc::UnboundedReceiver<BackendEvent>) {
    while let Some(event) = events.recv().await {
        match event {
            BackendEvent::Connection(state) => shared.publish(Update::Status(status_text(&state))),
            BackendEvent::Engine(engine) => {
                *shared.engine.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(engine.clone());
                shared.publish_chats(&engine);
            }
            BackendEvent::Core(event) => {
                let Some(engine) = shared.engine() else {
                    continue;
                };
                match event {
                    CoreEvent::SidebarChanged => shared.publish_chats(&engine),
                    CoreEvent::MessagesChanged { conversation_id }
                    | CoreEvent::MessagesStoredLate { conversation_id, .. } => {
                        if shared.is_open(&conversation_id) {
                            shared.publish_messages(&engine, &conversation_id);
                        }
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
pub fn message_rows(records: &[MessageRecord], my_id: &str, now: DateTime<Local>) -> Vec<MessageRow> {
    records
        .iter()
        .map(|record| MessageRow {
            id: record.message_id.clone(),
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
            vec![chat("old", Some(early)), chat("none", None), chat("new", Some(late))],
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
        assert_eq!(rows[0].when, at.with_timezone(&Local).format("%H:%M").to_string());
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
