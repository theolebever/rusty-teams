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

/// One message of the open conversation, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageRow {
    pub id: String,
    pub sender: String,
    /// Local time, "14:05" today, "08/11 14:05" otherwise.
    pub when: String,
    /// Body as plain text (HTML stripped, entities decoded, paragraphs as newlines).
    pub text: String,
    pub mine: bool,
}

/// What the bridge reports to the view. The view applies them in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// Human-readable connection line: "Connecting…", "Online", "Sign in in the browser window", …
    Status(String),
    /// Full chat list, replaces the previous one.
    Chats(Vec<ChatRow>),
    /// Full message list of one conversation, replaces the previous one for that id.
    Messages {
        conversation_id: String,
        rows: Vec<MessageRow>,
    },
    /// A send failed; shown under the composer.
    SendFailed(String),
}

/// What the view asks of the bridge. Calls return at once; results arrive as `Update`s.
pub trait Actions {
    /// Shows the cached messages at once, then fetches newer ones.
    fn open(&self, conversation_id: &str);
    fn send(&self, conversation_id: &str, text: String);
}
