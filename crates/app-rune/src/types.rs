//! Contract between the backend bridge (`bridge.rs`) and the Rune view (`ui.rs`).
//! Plain data only: no Rune, no tokio, no store types.

/// One row of the chat list, newest activity first.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatRow {
    pub id: String,
    pub title: String,
    /// "Sender: text" of the last message, single line.
    pub preview: String,
    pub unread: bool,
    /// Local time, "14:05" today, "08/11" otherwise. Empty when unknown.
    pub when: String,
}

/// A team and its visible channels in sidebar order.
#[derive(Debug, Clone, PartialEq)]
pub struct TeamRow {
    pub id: String,
    pub name: String,
    pub channels: Vec<ChannelRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChannelRow {
    pub id: String,
    pub name: String,
    pub unread: bool,
}

/// One message of the open conversation, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageRow {
    pub id: String,
    pub reply_to_id: Option<String>,
    pub deleted: bool,
    pub can_delete: bool,
    pub reactions: Vec<teams_core::ReactionSummary>,
    pub sender: String,
    /// Local time, "14:05" today, "08/11 14:05" otherwise.
    pub when: String,
    /// Body as plain text (HTML stripped, entities decoded, paragraphs as newlines).
    pub text: String,
    pub mine: bool,
    pub media: crate::media::Content,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SendTarget {
    pub conversation_id: String,
    pub thread_root_id: Option<String>,
    pub channel: bool,
}

/// What the bridge reports to the view. The view applies them in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// Human-readable connection line: "Connecting…", "Online", "Sign in in the browser window", …
    Status(String),
    /// Full chat list, replaces the previous one.
    Chats(Vec<ChatRow>),
    Teams(Vec<TeamRow>),
    Appearance(app_backend::appearance::AppearanceSettings),
    Activity(teams_core::ActivitySnapshot),
    Call(calling::CallUpdate),
    Ring(calling::EngineEvent),
    VideoReady,
    Image { key: String, bytes: Vec<u8> },
    ImageFailed { key: String, error: String },
    Attachment { target: SendTarget, path: std::path::PathBuf, result: Result<teams_core::UploadedFile, String> },
    Calendar { week: chrono::NaiveDate, rows: Vec<crate::screens::CalendarRow> },
    CalendarRefresh,
    Search { conversation_id: String, query: String, rows: Vec<MessageRow> },
    /// Answer to the chat-list search box (`search.rs`).
    GlobalSearch(crate::search::Results),
    /// Full message list of one conversation, replaces the previous one for that id.
    Messages {
        conversation_id: String,
        rows: Vec<MessageRow>,
    },
    /// Sending could not be confirmed; retain the text for the originating conversation.
    SendFailed {
        target: SendTarget,
        text: String,
        files: Vec<teams_core::FileReference>,
        error: String,
    },
}

/// What the view asks of the bridge. Calls return at once; results arrive as `Update`s.
pub trait Actions {
    /// Shows the cached messages at once, then fetches newer ones.
    fn open(&self, conversation_id: &str);
    fn send(&self, target: SendTarget, text: String, files: Vec<teams_core::FileReference>);
    fn load_older(&self, _conversation_id: &str) {}
    fn open_thread(&self, _conversation_id: &str, _root: &str) {}
    fn search(&self, _conversation_id: &str, _query: &str) {}
    /// Chats matching the query and messages across all cached conversations.
    fn search_all(&self, _query: &str) {}
    fn react(&self, _conversation_id: &str, _message_id: &str, _reaction: &str, _remove: bool) {}
    fn delete_message(&self, _conversation_id: &str, _message_id: &str) {}
    fn read_only(&self) -> bool { false }
    fn set_appearance(&self, _settings: app_backend::appearance::AppearanceSettings) {}
    fn load_calendar(&self, _week: chrono::NaiveDate) {}
    fn respond_event(&self, _id: &str, _ical_uid: Option<&str>, _reply: chatsvc::Reply, _send_response: bool) {}
    fn mark_activity_read(&self, _id: &str) {}
    fn mark_all_activity_read(&self) {}
    fn join_call(&self, _url: &str, _video: bool) {}
    fn call_command(&self, _command: calling::CallCommand) {}
    fn take_video(&self) -> Vec<(calling::VideoKey, std::sync::Arc<calling::VideoPicture>)> { Vec::new() }
    fn accept_call(&self, _ring_id: u64, _video: bool) {}
    fn decline_call(&self, _ring_id: u64) {}
    fn load_image(&self, _image: teams_core::ImageRef) {}
    fn download_file(&self, _url: String, _path: std::path::PathBuf) {}
    fn upload_file(&self, _target: SendTarget, _path: std::path::PathBuf) {}
    fn discard_attachment(&self, _uploaded: teams_core::UploadedFile) {}
}
