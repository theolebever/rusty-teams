//! `@` mentions in the composer: the suggestion list above the field and the mentions the sent
//! message carries (`teams_core::MentionInput`). Suggestions come from what the view already
//! knows (chat members, senders in the open conversation, the team of a channel).
//! ponytail: no directory or tag search; add `engine.mention_candidates` through an action when needed.

use app_backend::schemes::active_palette;
use rune::*;
use teams_core::MentionInput;
use crate::ui::text;

use crate::avatar::rgb;
use crate::types::Actions;
use crate::ui::{INK, MUTED, UiState, active_target, c, ts};

const LIMIT: usize = 6;
const EVERYONE: &str = "Everyone";
const QUERY_MAX_CHARS: usize = 30;

#[derive(Default)]
pub struct State {
    /// Everything picked so far; `resolve` keeps the ones still spelled out in the text.
    pub chosen: Vec<MentionInput>,
    pub highlighted: usize,
    /// Byte offset of an `@` whose list the user closed with Escape.
    dismissed: Option<usize>,
}

/// The `@word` being typed at the caret: the byte offset of the `@` and the text after it.
pub struct Active {
    pub at: usize,
    pub query: String,
}

pub fn active(edit: &TextEdit) -> Option<Active> {
    if edit.anchor != edit.focus {
        return None;
    }
    let before = edit.text.get(..edit.focus)?;
    let at = before.rfind('@')?;
    let query = &before[at + 1..];
    let opens = before[..at].chars().next_back().is_none_or(|c| c.is_whitespace() || "([{\"'".contains(c));
    let fits = query.chars().count() <= QUERY_MAX_CHARS && !query.contains('\n') && !query.starts_with(char::is_whitespace);
    (opens && fits).then(|| Active { at, query: query.to_owned() })
}

pub struct Candidate {
    pub label: String,
    pub hint: &'static str,
    pub input: MentionInput,
}

fn candidate(label: &str, hint: &'static str, input: MentionInput) -> Candidate {
    Candidate { label: label.to_owned(), hint, input }
}

pub fn candidates<A: Actions>(s: &UiState<A>, query: &str) -> Vec<Candidate> {
    let Some(target) = active_target(s) else { return Vec::new() };
    let id = &target.conversation_id;
    let mut all = Vec::new();
    let members = s.details.get(id).map(|details| details.members.as_slice()).unwrap_or_default();
    if !target.channel && members.len() > 2 {
        all.push(candidate(EVERYONE, "Everyone", MentionInput::everyone(id, EVERYONE)));
    }
    if let Some(team) = s.teams.iter().find(|team| team.channels.iter().any(|channel| channel.id == *id)) {
        if let Some(channel) = team.channels.iter().find(|channel| channel.id == *id) {
            all.push(candidate(&channel.name, "Channel", MentionInput::channel(id, &channel.name)));
        }
        all.push(candidate(&team.name, "Team", MentionInput::team(&team.id, &team.name)));
    }
    let senders = s.messages.iter().filter(|m| !m.mine && !m.system).filter_map(|m| Some((m.sender_id.as_deref()?, m.sender.as_str())));
    let people = members.iter().filter(|m| !m.is_bot()).filter_map(|m| Some((m.user_id.as_deref()?, m.display_name.as_str())));
    let mut seen = Vec::new();
    for (user_id, name) in people.chain(senders) {
        if !name.is_empty() && !seen.contains(&user_id) {
            seen.push(user_id);
            all.push(candidate(name, "", MentionInput::user(user_id, name)));
        }
    }
    let query = query.to_lowercase();
    let rank = |label: &str| {
        let label = label.to_lowercase();
        if label.starts_with(&query) { Some(0) }
        else if label.split_whitespace().any(|word| word.starts_with(&query)) { Some(1) }
        else { label.contains(&query).then_some(2) }
    };
    let mut ranked: Vec<_> = all.into_iter().filter_map(|c| Some((rank(&c.label)?, c))).collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.into_iter().map(|(_, c)| c).take(LIMIT).collect()
}

/// The open list, if the caret sits in an `@word` that has matches.
pub fn open_list<A: Actions>(s: &UiState<A>) -> Option<(Active, Vec<Candidate>)> {
    let active = active(&s.composer)?;
    if s.compose.mentions.dismissed == Some(active.at) {
        return None;
    }
    let found = candidates(s, &active.query);
    (!found.is_empty()).then_some((active, found))
}

/// Replaces the `@word` at the caret with the person's name and remembers the mention.
pub fn pick<A: Actions>(s: &mut UiState<A>, index: usize) {
    let Some((active, mut found)) = open_list(s) else { return };
    if index >= found.len() {
        return;
    }
    let chosen = found.swap_remove(index);
    s.composer.set_selection(active.at, s.composer.focus);
    s.composer.insert(&format!("@{} ", chosen.label));
    s.compose.mentions.chosen.push(chosen.input);
    s.compose.mentions.highlighted = 0;
}

/// Arrows, Enter, Tab and Escape while the list is open. True when the key was used.
pub fn handle_key<A: Actions>(s: &mut UiState<A>, key: &KeyEvent) -> bool {
    if matches!(key.key, Key::Text(_) | Key::Backspace) {
        s.compose.mentions.highlighted = 0;
        s.compose.mentions.dismissed = None;
        return false;
    }
    let Some((active, found)) = open_list(s) else { return false };
    let highlighted = s.compose.mentions.highlighted.min(found.len() - 1);
    match key.key {
        Key::Down => s.compose.mentions.highlighted = (highlighted + 1) % found.len(),
        Key::Up => s.compose.mentions.highlighted = (highlighted + found.len() - 1) % found.len(),
        Key::Enter | Key::Tab if !key.mods.shift => pick(s, highlighted),
        Key::Escape => s.compose.mentions.dismissed = Some(active.at),
        _ => return false,
    }
    true
}

/// The mentions to send: those still written as `@name` in `text`, in text order, each matched
/// to its own occurrence (the core matches them in order).
pub fn resolve(text: &str, chosen: &[MentionInput]) -> Vec<MentionInput> {
    let mut taken: Vec<usize> = Vec::new();
    let mut found: Vec<(usize, MentionInput)> = Vec::new();
    for mention in chosen {
        let tag = format!("@{}", mention.text);
        let spot = text.match_indices(&tag).map(|(at, _)| at).find(|at| !taken.contains(at));
        if let Some(at) = spot {
            taken.push(at);
            found.push((at, mention.clone()));
        }
    }
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, mention)| mention).collect()
}

pub fn list<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let (_, found) = open_list(s)?;
    let palette = active_palette();
    let highlighted = s.compose.mentions.highlighted.min(found.len() - 1);
    let mut rows = col().key("mention-list").w_full().padding(4.0).radius(6.0).border(1.0, rgb(palette.border_strong)).fill(rgb(palette.surface));
    for (index, item) in found.into_iter().enumerate() {
        rows = rows.child(
            row()
                .key(format!("mention-{index}"))
                .w_full()
                .h(32.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .radius(4.0)
                .fill(if index == highlighted { rgb(palette.popup_hover) } else { rgb(palette.surface) })
                .hover(rgb(palette.popup_hover))
                .on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| {
                    pick(s, index);
                    tx.focus("composer", false);
                })
                .child(text(item.label).font(ts(13.0, 400.0)).color(c(INK)))
                .child(text(item.hint).font(ts(12.0, 400.0)).color(c(MUTED))),
        );
    }
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_at_opens_a_mention_only_after_whitespace() {
        let mut edit = TextEdit::new("hi @Ad");
        assert_eq!(active(&edit).map(|a| (a.at, a.query)), Some((3, "Ad".into())));
        edit = TextEdit::new("mail ops@acme");
        assert!(active(&edit).is_none());
        edit = TextEdit::new("a @ b");
        assert!(active(&edit).is_none());
    }

    #[test]
    fn mentions_keep_text_order_and_drop_deleted_ones() {
        let chosen = [MentionInput::user("2", "Bo"), MentionInput::user("1", "Ada"), MentionInput::user("3", "Cy"), MentionInput::user("4", "Bo")];
        let got = resolve("@Ada hi @Bo and @Bo", &chosen);
        let ids: Vec<_> = got.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(ids, ["Ada", "Bo", "Bo"]);
        assert_eq!(got[1], chosen[0]);
        assert_eq!(got[2], chosen[3]);
    }
}
