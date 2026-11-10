//! Search box above the chat list. One query filters the chats by title and lists the messages
//! that match, both through the backend's cached search. The bridge runs the query; the view
//! shows the answer in place of the chat list until the box is cleared.

use chrono::Local;
use rune::tok::Family;
use rune::*;


use crate::types::Actions;
use crate::ui::{HOVER, INK, MUTED, UiState, chat_row, open_chat};
use app_backend::backend::Engine;
use store::{HIGHLIGHT_END, HIGHLIGHT_START};

const CHAT_LIMIT: usize = 50;
const MESSAGE_LIMIT: usize = 50;
const HIT_H: f32 = 56.0;

/// One message found by the backend search.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageHit {
    pub conversation_id: String,
    pub sender: String,
    /// Local time, as in `MessageRow::when`.
    pub when: String,
    /// Plain text, match markers removed.
    pub snippet: String,
}

/// The answer to one query. `chats` holds the ids of the matching chats, best match first.
#[derive(Debug, Clone, PartialEq)]
pub struct Results {
    pub query: String,
    pub chats: Vec<String>,
    pub messages: Vec<MessageHit>,
}

pub struct SearchState {
    pub edit: TextEdit,
    pub results: Option<Results>,
}

impl Default for SearchState {
    fn default() -> Self {
        SearchState { edit: TextEdit::new(""), results: None }
    }
}

/// Runs the query against the backend's cached chats and messages.
pub fn run(engine: &Engine, query: &str) -> Result<Results, String> {
    let chats = engine
        .search_conversations(query, CHAT_LIMIT)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|hit| hit.conversation_id)
        .collect();
    let messages = engine
        .search_messages(query, MESSAGE_LIMIT)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|hit| MessageHit {
            conversation_id: hit.conversation_id,
            sender: hit.sender_name.unwrap_or_default(),
            when: crate::bridge::format_when(hit.created_at, Local::now(), true),
            snippet: plain_snippet(&hit.snippet),
        })
        .collect();
    Ok(Results { query: query.to_owned(), chats, messages })
}

/// Keeps the answer only while the box still holds the query it was asked for.
pub fn apply(state: &mut SearchState, results: Results) {
    if state.edit.text.trim() == results.query {
        state.results = Some(results);
    }
}

fn plain_snippet(snippet: &str) -> String {
    snippet.replace([HIGHLIGHT_START, HIGHLIGHT_END], "")
}

fn submit<A: Actions>(s: &mut UiState<A>) {
    let query = s.global_search.edit.text.trim().to_owned();
    if query.is_empty() {
        s.global_search.results = None;
    } else {
        s.actions.search_all(&query);
    }
}

pub fn search_box<A: Actions + 'static>(s: &UiState<A>) -> El {
    row()
        .key("chat-search")
        .w_full()
        .px(8.0)
        .pb(8.0)
        .gap(6.0)
        .center_y()
        .child(
            text_input(&s.global_search.edit, ts(13.0, 400.0))
                .key("chat-search-input")
                .label("Search chats and messages")
                .placeholder("Search chats and messages", c(MUTED))
                .color(c(INK))
                .grow(1.0)
                .on_edit(|s: &mut UiState<A>| &mut s.global_search.edit)
                .on_submit(|s: &mut UiState<A>, _| submit(s)),
        )
        .child(
            row()
                .key("chat-search-clear")
                .padding(8.0)
                .hover(c(HOVER))
                .on_press_tx(|s: &mut UiState<A>, _| {
                    s.global_search.edit.clear();
                    s.global_search.results = None;
                })
                .child(text("Clear").font(ts(12.0, 400.0)).color(c(INK))),
        )
}

/// The chat list while a search has an answer: matching chats, then matching messages.
pub fn results_list<A: Actions + 'static>(s: &UiState<A>) -> El {
    let Some(results) = &s.global_search.results else { return el() };
    let mut list = col().key("chat-search-results").w_full().px(8.0).pb(8.0).gap(2.0);
    list = list.child(heading("Chats"));
    let matched: Vec<usize> = results
        .chats
        .iter()
        .filter_map(|id| s.chats.iter().position(|chat| &chat.id == id))
        .collect();
    if matched.is_empty() {
        list = list.child(note("No chats match"));
    }
    for i in matched {
        list = list.child(col().key(format!("chat-{}", s.chats[i].id)).w_full().child(chat_row(s, i)));
    }
    list = list.child(heading("Messages"));
    if results.messages.is_empty() {
        list = list.child(note("No messages match"));
    }
    for (n, hit) in results.messages.iter().enumerate() {
        let id = hit.conversation_id.clone();
        list = list.child(
            col()
                .key(format!("search-hit-{n}"))
                .w_full()
                .h(HIT_H)
                .px(10.0)
                .py(6.0)
                .gap(2.0)
                .radius(8.0)
                .hover(c(HOVER))
                .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| open_chat(s, &id, tx))
                .child(
                    row()
                        .w_full()
                        .gap(6.0)
                        .center_y()
                        .child(
                            text(title_of(s, &hit.conversation_id))
                                .font(ts(13.0, 700.0))
                                .color(c(INK))
                                .truncate()
                                .grow(1.0)
                                .min_w(0.0),
                        )
                        .child(text(hit.when.clone()).font(ts(12.0, 400.0)).color(c(MUTED))),
                )
                .child(
                    text(format!("{}: {}", hit.sender, hit.snippet))
                        .font(ts(13.0, 400.0))
                        .color(c(MUTED))
                        .truncate(),
                ),
        );
    }
    list
}

fn heading(label: &str) -> El {
    row().w_full().h(28.0).px(10.0).center_y().child(
        text(label.to_owned()).font(ts(12.0, 700.0)).color(c(MUTED)),
    )
}

fn note(label: &str) -> El {
    row().w_full().px(10.0).h(28.0).center_y().child(
        text(label.to_owned()).font(ts(13.0, 400.0)).color(c(MUTED)),
    )
}

/// Chat or channel name for a conversation id, from the lists the view already holds.
fn title_of<A: Actions>(s: &UiState<A>, id: &str) -> String {
    s.chats
        .iter()
        .find(|chat| chat.id == id)
        .map(|chat| chat.title.clone())
        .or_else(|| {
            s.teams
                .iter()
                .flat_map(|team| &team.channels)
                .find(|channel| channel.id == id)
                .map(|channel| format!("# {}", channel.name))
        })
        .unwrap_or_else(|| "Conversation".into())
}

fn c(s: &str) -> Color {
    crate::settings::color(s)
}

fn ts(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Segoe UI"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRow, TeamRow, Update};
    use rune::{Env, Runtime};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        queries: RefCell<Vec<String>>,
    }

    impl Actions for Fake {
        fn open(&self, _conversation_id: &str) {}
        fn send(&self, _target: crate::types::SendTarget, _text: String, _files: Vec<teams_core::FileReference>) {}
        fn search_all(&self, query: &str) {
            self.queries.borrow_mut().push(query.to_owned());
        }
    }

    fn chat(id: &str, title: &str) -> ChatRow {
        ChatRow { id: id.into(), title: title.into(), preview: String::new(), unread: false, when: String::new() }
    }

    fn hit(conversation_id: &str, snippet: &str) -> MessageHit {
        MessageHit { conversation_id: conversation_id.into(), sender: "Ada".into(), when: "14:05".into(), snippet: snippet.into() }
    }

    fn rt() -> Runtime<UiState<Fake>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Fake::default()), crate::ui::view::<Fake>, (1000.0, 700.0), env);
        rt.frame(0.0);
        rt
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn type_and_submit(rt: &mut Runtime<UiState<Fake>>, query: &str) {
        click(rt, "chat-search-input");
        rt.key(KeyEvent { key: Key::Text(query.into()), mods: Mods::default() });
        rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn plain_snippet_drops_the_match_markers() {
        let raw = format!("lunch at {HIGHLIGHT_START}noon{HIGHLIGHT_END}");
        assert_eq!(plain_snippet(&raw), "lunch at noon");
    }

    #[test]
    fn stale_answers_are_dropped() {
        let mut state = SearchState::default();
        state.edit = TextEdit::new("bu");
        let answer = |query: &str| Results { query: query.into(), chats: vec![], messages: vec![] };
        apply(&mut state, answer("b"));
        assert!(state.results.is_none());
        apply(&mut state, answer("bu"));
        assert_eq!(state.results.map(|r| r.query), Some("bu".to_owned()));
    }

    #[test]
    fn submitting_asks_the_bridge_and_shows_only_matching_chats_and_messages() {
        let mut rt = rt();
        rt.update(|s, _| {
            s.apply(Update::Chats(vec![chat("a", "Ada"), chat("b", "Bo")]));
            s.apply(Update::Teams(vec![TeamRow { id: "t".into(), name: "T".into(), channels: vec![] }]));
        });
        rt.frame(1.0 / 60.0);
        type_and_submit(&mut rt, "bo");
        assert_eq!(rt.state.actions.queries.borrow().as_slice(), ["bo"]);

        rt.update(|s, _| {
            apply(&mut s.global_search, Results {
                query: "bo".into(),
                chats: vec!["b".into()],
                messages: vec![hit("a", "lunch?")],
            });
        });
        rt.frame(1.0 / 60.0);
        assert!(rt.find("chat-b").is_some());
        assert!(rt.find("chat-a").is_none());
        assert!(rt.find("search-hit-0").is_some());
    }

    #[test]
    fn clicking_a_message_hit_opens_its_chat() {
        let mut rt = rt();
        rt.update(|s, _| {
            s.apply(Update::Chats(vec![chat("a", "Ada")]));
            s.global_search.edit = TextEdit::new("lunch");
            apply(&mut s.global_search, Results {
                query: "lunch".into(),
                chats: vec![],
                messages: vec![hit("a", "lunch?")],
            });
        });
        rt.frame(1.0 / 60.0);
        click(&mut rt, "search-hit-0");
        assert_eq!(rt.state.selected.as_deref(), Some("a"));
        assert!(rt.state.global_search.results.is_some());
    }

    #[test]
    fn clear_drops_the_answer_and_the_chat_list_returns() {
        let mut rt = rt();
        rt.update(|s, _| {
            s.apply(Update::Chats(vec![chat("a", "Ada")]));
            s.global_search.edit = TextEdit::new("ad");
            s.global_search.results = Some(Results { query: "ad".into(), chats: vec![], messages: vec![] });
        });
        rt.frame(1.0 / 60.0);
        click(&mut rt, "chat-search-clear");
        assert!(rt.state.global_search.results.is_none());
        assert!(rt.state.global_search.edit.text.is_empty());
        assert!(rt.find("chat-a").is_some());
    }
}
