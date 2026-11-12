//! Keyboard switcher with local conversations and account search suggestions.

use app_backend::schemes::active_palette;
use rune::*;
use teams_core::substrate_search::{SearchSession, Suggestions, split_highlights};

use crate::screens::{navigate, Screen};
use crate::types::Actions;
use crate::ui::{open_chat, text, ts, UiState};
use crate::view_chatlist::tone;

const LIMIT: usize = 5;
const ROW_H: f32 = 52.0;

#[derive(Default)]
pub struct Switcher {
    pub edit: TextEdit,
    pub highlighted: usize,
    session: SearchSession,
    results: Suggestions,
    error: Option<String>,
}

pub fn open<A: Actions>(s: &mut UiState<A>, tx: &mut Tx) {
    s.nav.shortcuts_open = false;
    s.nav.settings_more = false;
    s.nav.switcher = Some(Switcher::default());
    search(s);
    tx.focus("switcher-input", false);
}

fn close<A: Actions>(s: &mut UiState<A>) { s.nav.switcher = None; }

fn search<A: Actions>(s: &mut UiState<A>) {
    let Some(switcher) = &mut s.nav.switcher else { return };
    switcher.highlighted = 0;
    switcher.results = Suggestions::default();
    switcher.error = None;
    s.actions.search_switcher(switcher.session.clone(), switcher.edit.text.trim().to_owned());
}

pub fn apply<A: Actions>(s: &mut UiState<A>, session: &str, query: &str, result: Result<Suggestions, String>) {
    let Some(switcher) = &mut s.nav.switcher else { return };
    if switcher.session.correlation_id() != session || switcher.edit.text.trim() != query { return; }
    match result {
        Ok(results) => { switcher.results = results; switcher.error = None; }
        Err(error) => switcher.error = Some(error),
    }
}

#[derive(Clone)]
enum Target {
    Conversation(String, bool),
    Person(String),
    Message { conversation: String, root: Option<String>, channel: bool },
}

struct Candidate {
    title: String,
    subtitle: String,
    highlights: Vec<std::ops::Range<usize>>,
    group: &'static str,
    target: Target,
}

fn candidates<A: Actions>(s: &UiState<A>) -> Vec<Candidate> {
    let Some(switcher) = &s.nav.switcher else { return Vec::new() };
    let query = switcher.edit.text.trim().to_lowercase();
    let rank = |id: &str| s.nav.recent.iter().position(|recent| recent == id).unwrap_or(usize::MAX);
    let mut chats: Vec<_> = s.chats.iter().filter(|chat| chat.title.to_lowercase().contains(&query)).collect();
    chats.sort_by_key(|chat| rank(&chat.id));
    let mut found: Vec<_> = chats.into_iter().take(LIMIT).map(|chat| Candidate {
        title: chat.title.clone(), subtitle: chat.preview.clone(), highlights: vec![], group: "Chats", target: Target::Conversation(chat.id.clone(), false),
    }).collect();
    found.extend(s.teams.iter().flat_map(|team| team.channels.iter().map(move |channel| (team, channel)))
        .filter(|(team, channel)| format!("{} {}", team.name, channel.name).to_lowercase().contains(&query)).take(LIMIT)
        .map(|(team, channel)| Candidate { title: channel.name.clone(), subtitle: team.name.clone(), highlights: vec![], group: "Channels", target: Target::Conversation(channel.id.clone(), true) }));
    found.extend(switcher.results.people.iter().filter_map(|person| {
        let user = person.user_id.as_ref()?;
        if s.own.me.as_ref().is_some_and(|me| &me.user_id == user) { return None; }
        Some(Candidate { title: person.display_name.clone(), subtitle: person.job_title.clone().or_else(|| person.email_addresses.first().cloned()).unwrap_or_default(),
            highlights: vec![], group: "People", target: Target::Person(user.clone()) })
    }).take(4));
    if query.chars().count() >= 2 {
        found.extend(switcher.results.messages.iter().filter_map(|message| {
            let chat = s.chats.iter().find(|chat| chat.id == message.conversation_id);
            let channel = s.teams.iter().flat_map(|team| &team.channels).find(|channel| channel.id == message.conversation_id);
            let title = chat.map(|chat| chat.title.as_str()).or_else(|| channel.map(|channel| channel.name.as_str()))?;
            let highlighted = split_highlights(message.highlighted_summary.as_deref().unwrap_or(&message.preview));
            Some(Candidate { title: format!("{} · {title}", message.sender_name.as_deref().unwrap_or("Unknown")),
                subtitle: highlighted.text.replace(['\n', '\r'], " "), highlights: highlighted.hits, group: "Messages",
                target: Target::Message { conversation: message.conversation_id.clone(), root: message.root_message_id.clone(), channel: channel.is_some() } })
        }).take(LIMIT));
    }
    found
}

fn choose<A: Actions>(s: &mut UiState<A>, index: usize, tx: &mut Tx) {
    let Some(candidate) = candidates(s).into_iter().nth(index) else { return };
    close(s);
    match candidate.target {
        Target::Person(id) => s.actions.open_direct(&id),
        Target::Conversation(id, channel) => {
            navigate(s, if channel { Screen::Teams } else { Screen::Chat });
            open_chat(s, &id, tx);
        }
        Target::Message { conversation, root, channel } => {
            navigate(s, if channel { Screen::Teams } else { Screen::Chat });
            open_chat(s, &conversation, tx);
            if channel && root.is_some() { crate::ui::select_thread(s, root); }
        }
    }
}

pub fn on_key<A: Actions>(s: &mut UiState<A>, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(switcher) = &s.nav.switcher else { return false };
    let delta: isize = match ev.key {
        Key::Down => 1,
        Key::Up => -1,
        Key::Tab => if ev.mods.shift { -1 } else { 1 },
        Key::Enter => { choose(s, switcher.highlighted, tx); return true; }
        Key::Escape => { close(s); return true; }
        _ => return false,
    };
    let len = candidates(s).len();
    if let Some(switcher) = &mut s.nav.switcher && len > 0 {
        switcher.highlighted = (switcher.highlighted as isize + delta).rem_euclid(len as isize) as usize;
    }
    true
}

pub fn view<A: Actions + 'static>(s: &UiState<A>, switcher: &Switcher) -> El {
    let p = active_palette();
    let found = candidates(s);
    let at = switcher.highlighted.min(found.len().saturating_sub(1));
    let mut list = col().key("switcher-list").w_full().px(8.0).pb(8.0);
    let mut group = "";
    if found.is_empty() { list = list.child(text("No matches").font(ts(13.0, 400.0)).color(tone(p.text_muted)).padding(12.0)); }
    for (i, candidate) in found.into_iter().enumerate() {
        if group != candidate.group {
            group = candidate.group;
            list = list.child(text(group).key(format!("switcher-group-{group}")).font(ts(12.0, 600.0)).color(tone(p.text_muted)).padding(8.0));
        }
        let avatar = match &candidate.target {
            Target::Conversation(id, false) => crate::avatar::chat(s, &candidate.title, id, 28.0),
            Target::Person(id) => crate::avatar::person(s, &candidate.title, Some(id), 28.0, None),
            _ => crate::avatar::avatar(&candidate.title, 28.0, None),
        };
        let spans = candidate.highlights.into_iter().map(|hit| rune::Span::new(hit.start, hit.end).bold().color(tone(p.accent))).collect();
        let line = row().key(format!("switch-{i}")).w_full().h(ROW_H).px(8.0).gap(10.0).center_y().radius(6.0).hover(tone(p.row_hover))
            .label(candidate.title.clone()).on_press_tx(move |s: &mut UiState<A>, tx| choose(s, i, tx))
            .child(avatar).child(col().grow(1.0).min_w(0.0)
                .child(text(candidate.title).font(ts(13.0, 400.0)).color(tone(p.foreground)).truncate())
                .child(rune::rich(candidate.subtitle, spans).font(ts(12.0, 400.0)).color(tone(p.text_muted)).truncate()));
        list = list.child(if i == at { line.fill(tone(p.selection)) } else { line });
    }
    if let Some(error) = &switcher.error { list = list.child(text(format!("Server search unavailable: {error}")).font(ts(12.0, 400.0)).color(tone(p.text_muted)).wrap().padding(8.0)); }
    let body = col().w_full()
        .child(row().w_full().h(48.0).px(16.0).center_y()
            .child(text_input(&switcher.edit, ts(15.0, 400.0)).key("switcher-input").label("Jump to a chat, channel, person or message")
                .placeholder("Jump to a chat, channel, person or message", tone(p.text_muted)).color(tone(p.foreground)).grow(1.0)
                .on_edit(|s: &mut UiState<A>| &mut s.nav.switcher.get_or_insert_with(Default::default).edit)
                .on_input_change(|s: &mut UiState<A>, _| search(s))
                .on_input_key(on_key::<A>)))
        .child(el().w_full().h(1.0).fill(tone(p.border)))
        .child(col().w_full().max_h(420.0).scroll(ScrollSpec { stick_bottom: false }).child(list));
    crate::shortcuts::card("switcher", 520.0, body, close::<A>)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake;
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
    }

    #[test]
    fn stale_results_do_not_replace_current_search() {
        let mut s = UiState::new(Fake);
        s.nav.switcher = Some(Switcher::default());
        let switcher = s.nav.switcher.as_mut().unwrap();
        switcher.edit = TextEdit::new("Ada");
        let session = switcher.session.correlation_id().to_owned();
        apply(&mut s, &session, "old", Err("stale".into()));
        assert!(s.nav.switcher.as_ref().unwrap().error.is_none());
        apply(&mut s, &session, "Ada", Err("offline".into()));
        assert_eq!(s.nav.switcher.as_ref().unwrap().error.as_deref(), Some("offline"));
    }
}
