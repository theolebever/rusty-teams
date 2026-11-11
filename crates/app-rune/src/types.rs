//! Contract between the backend bridge (`bridge.rs`) and the Rune view (`ui.rs`).
//! Data snapshots only: no Rune or runtime handles.

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
    pub hidden: bool,
    pub id: String,
    pub name: String,
    pub unread: bool,
}

/// One message of the open conversation, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageRow {
    pub sender_id: Option<String>,
    pub system: bool,
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

/// The pinned message of a conversation, shown in the bar under its header.
#[derive(Debug, Clone, PartialEq)]
pub struct PinnedRow {
    pub conversation_id: String,
    pub sender: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChatDetails {
    pub avatar_id: Option<String>,
    /// Up to two other people of a group chat, drawn as a pair when it has no picture of its own.
    pub group: Vec<store::MemberRecord>,
    pub members: Vec<store::MemberRecord>,
    pub muted: bool,
    pub presence: Option<teams_core::Presence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SendTarget {
    pub conversation_id: String,
    pub thread_root_id: Option<String>,
    pub channel: bool,
}

/// A directory search hit, for the new-chat and add-people panels.
#[derive(Debug, Clone, PartialEq)]
pub struct Person {
    pub user_id: String,
    pub name: String,
    pub mail: Option<String>,
}

/// What the composer hands over at send: the text, its real mentions and chosen GIFs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Composed {
    pub text: String,
    pub mentions: Vec<teams_core::MentionInput>,
    pub gifs: Vec<chatsvc::Gif>,
}

/// What the bridge reports to the view. The view applies them in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// Directory search answer for `query` (`new_chat.rs`).
    People { query: String, rows: Vec<Person> },
    /// A chat made by `create_chat`; the view opens it.
    ChatCreated(String),
    Audio { url: String, result: Result<Vec<u8>, String> },
    CallHistory(Result<Vec<chatsvc::CallLogEntry>, String>),
    AgentSettings(agents::AgentSettings),
    AgentResult(String),
    Details(std::collections::HashMap<String, ChatDetails>),
    Pinned { conversation_id: String, pin: Option<PinnedRow> },
    /// Ids of the edited messages of the open conversation (`message_menu.rs`).
    Edited { conversation_id: String, ids: Vec<String> },
    Typing(chatsvc::TypingEvent),
    /// Human-readable connection line: "Connecting…", "Online", "Sign in in the browser window", …
    Status(String),
    /// The signed-in user, known once the first sidebar refresh stored it.
    Me { user_id: String, name: String, email: String },
    OwnStatus(teams_core::PresenceStatus),
    Profile { user_id: String, result: Result<teams_core::PersonProfile, String> },
    /// A 1:1 chat found or created for a person; the view selects it.
    OpenChat(String),
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
    /// Calendar events of the agenda window (next seven days), raw from the backend.
    Agenda(Vec<chatsvc::CalendarEvent>),
    Search { conversation_id: String, query: String, rows: Vec<MessageRow> },
    /// Answer to the chat-list search box (`search.rs`).
    GlobalSearch(crate::search::Results),
    /// Full message list of one conversation, replaces the previous one for that id.
    Messages {
        conversation_id: String,
        rows: Vec<MessageRow>,
    },
    /// Answer to `search_gifs`; empty query means trending.
    Gifs { query: String, result: Result<Vec<chatsvc::Gif>, String> },
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
    fn set_chat_muted(&self, _id: &str, _muted: bool) {}
    /// `read == false` marks the chat unread.
    fn set_chat_read(&self, _id: &str, _read: bool) {}
    fn hide_chat(&self, _id: &str) {}
    fn leave_chat(&self, _id: &str) {}
    fn search_people(&self, _query: &str) {}
    /// One id makes a 1:1, several a group; the answer is `Update::ChatCreated`.
    fn create_chat(&self, _user_ids: Vec<String>, _topic: Option<String>) {}
    fn add_members(&self, _id: &str, _people: Vec<store::MemberRecord>) {}
    fn remove_member(&self, _id: &str, _user_id: &str) {}
    fn load_audio(&self, _url: &str) {}
    fn call_back(&self, _entry: &chatsvc::CallLogEntry) {}
    fn load_call_history(&self) {}
    fn run_agent(&self, _id: &str, _prompt: &str) {}
    fn stop_agent(&self) {}
    fn rename_chat(&self, _id: &str, _title: &str) {}
    fn meet_now(&self, _conversation_id: &str) {}
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
    fn edit_message(&self, _conversation_id: &str, _message_id: &str, _text: &str) {}
    fn pin_message(&self, _conversation_id: &str, _message_id: &str, _pin: bool) {}
    fn forward_message(&self, _source_id: &str, _target_id: &str, _message_id: &str) {}
    fn mark_unread_from(&self, _conversation_id: &str, _message_id: &str) {}
    fn set_appearance(&self, _settings: app_backend::appearance::AppearanceSettings) {}
    fn load_calendar(&self, _week: chrono::NaiveDate) {}
    fn load_agenda(&self) {}
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
    fn set_availability(&self, _forced: Option<teams_core::ForcedAvailability>) {}
    fn set_status_note(&self, _note: Option<teams_core::StatusNote>) {}
    fn load_profile(&self, _user_id: &str) {}
    /// Finds or creates the 1:1 chat with a person; answered with `Update::OpenChat`.
    fn open_direct(&self, _user_id: &str) {}
    fn call_person(&self, _user_id: &str, _name: &str) {}
    /// `send` with mentions and GIFs; fakes that only know `send` still get the text.
    fn send_composed(&self, target: SendTarget, composed: Composed, files: Vec<teams_core::FileReference>) {
        self.send(target, composed.text, files);
    }
    /// Answered with `Update::Gifs`.
    fn search_gifs(&self, _query: &str) {}
    /// Stores the unsent text of a conversation; blank text deletes the draft.
    fn save_draft(&self, _conversation_id: &str, _text: &str) {}
    fn load_draft(&self, _conversation_id: &str) -> Option<String> { None }
}
