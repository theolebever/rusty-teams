//! New chat: a people picker over the directory search, then a 1:1 or group chat. The same picker
//! serves "Add people" in the members panel (`members.rs`). GPUI: `views/new_chat.rs`.

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

use crate::icons::sym;
use crate::types::{Actions, Person};
use crate::ui::{open_chat, ts, UiState};
use crate::view_chatlist::tone;

const RESULT_H: f32 = 44.0;
const RESULT_LIMIT: usize = 8;

/// A search box, the people picked so far and the last directory answer.
pub struct Draft {
    pub query: TextEdit,
    /// Optional group name, asked once two or more people are picked.
    pub name: TextEdit,
    pub chips: Vec<Person>,
    pub results: Vec<Person>,
}

impl Default for Draft {
    fn default() -> Self {
        Draft { query: TextEdit::new(""), name: TextEdit::new(""), chips: Vec::new(), results: Vec::new() }
    }
}

/// Which draft of the state a picker edits.
pub type Select<A> = fn(&mut UiState<A>) -> &mut Draft;

pub fn open<A: Actions>(s: &mut UiState<A>) {
    s.chatlist.manage.new_chat = Draft::default();
    s.chatlist.manage.new_chat_open = true;
}

/// A directory answer is kept only by the drafts still holding the query it was asked for.
pub fn apply_people<A: Actions>(s: &mut UiState<A>, query: &str, rows: Vec<Person>) {
    for draft in [&mut s.chatlist.manage.new_chat, &mut s.chatlist.manage.add] {
        if draft.query.text.trim() == query {
            draft.results = rows.clone();
        }
    }
}

fn search<A: Actions>(s: &mut UiState<A>, select: Select<A>) {
    let draft = select(s);
    let query = draft.query.text.trim().to_owned();
    if query.is_empty() {
        draft.results.clear();
    } else {
        s.actions.search_people(&query);
    }
}

fn choose<A: Actions>(s: &mut UiState<A>, select: Select<A>, person: Person) {
    let draft = select(s);
    if draft.chips.iter().all(|chip| chip.user_id != person.user_id) {
        draft.chips.push(person);
    }
    draft.query.clear();
    draft.results.clear();
}

/// "To:" chips, the search box and the matches. `skip` holds user ids that cannot be picked
/// (people already in the chat). `key` prefixes every element key.
pub fn picker<A: Actions + 'static>(s: &UiState<A>, draft: &Draft, key: &'static str, select: Select<A>, skip: &[String]) -> El {
    let p = active_palette();
    let mut chips = row().w_full().gap(6.0).wrap().center_y();
    for (index, chip) in draft.chips.iter().enumerate() {
        chips = chips.child(
            row()
                .key(format!("{key}-chip-{index}"))
                .h(26.0)
                .pl(3.0)
                .pr(6.0)
                .gap(6.0)
                .center_y()
                .radius(13.0)
                .fill(tone(p.surface_raised))
                .hover(tone(p.border_strong))
                .on_press(move |s: &mut UiState<A>| { select(s).chips.remove(index); })
                .child(crate::avatar::person(s, &chip.name, Some(&chip.user_id), 20.0, None))
                .child(text(chip.name.clone()).font(ts(13.0, 400.0)).color(tone(p.foreground)).truncate().max_w(160.0))
                .child(sym("close", 12.0).color(tone(p.text_muted))),
        );
    }
    let input = text_input(&draft.query, ts(13.0, 400.0))
        .key(format!("{key}-query"))
        .label("Enter name or email")
        .placeholder("Enter name or email", tone(p.text_muted))
        .color(tone(p.foreground))
        .autofocus()
        .w_full()
        .on_edit(move |s: &mut UiState<A>| &mut select(s).query)
        .on_input_change(move |s: &mut UiState<A>, _| search(s, select))
        .on_submit(move |s: &mut UiState<A>, _| search(s, select));
    let mut list = col().w_full().gap(2.0);
    let shown = draft
        .results
        .iter()
        .filter(|person| !skip.contains(&person.user_id) && draft.chips.iter().all(|chip| chip.user_id != person.user_id))
        .take(RESULT_LIMIT);
    for (index, person) in shown.enumerate() {
        let picked = person.clone();
        list = list.child(
            row()
                .key(format!("{key}-result-{index}"))
                .w_full()
                .h(RESULT_H)
                .px(6.0)
                .gap(8.0)
                .center_y()
                .radius(6.0)
                .hover(tone(p.popup_hover))
                .on_press(move |s: &mut UiState<A>| choose(s, select, picked.clone()))
                .child(crate::avatar::person(s, &person.name, Some(&person.user_id), 28.0, None))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .child(text(person.name.clone()).font(ts(13.0, 400.0)).color(tone(p.foreground)).truncate())
                        .children(person.mail.clone().map(|mail| text(mail).font(ts(11.5, 400.0)).color(tone(p.text_muted)).truncate())),
                ),
        );
    }
    col().w_full().gap(8.0).child(chips).child(input).child(list)
}

/// The floating "New chat" panel under the chat list header.
pub fn panel<A: Actions + 'static>(s: &UiState<A>) -> El {
    let p = active_palette();
    let draft = &s.chatlist.manage.new_chat;
    let mut body = col()
        .key("new-chat")
        .w(320.0)
        .padding(12.0)
        .gap(10.0)
        .radius(10.0)
        .fill(tone(p.surface))
        .border(1.0, tone(p.border_strong))
        .child(text("New chat").font(ts(14.0, 600.0)).color(tone(p.foreground)))
        .child(picker(s, draft, "new-chat", |s| &mut s.chatlist.manage.new_chat, &[]));
    if draft.chips.len() >= 2 {
        body = body.child(
            text_input(&draft.name, ts(13.0, 400.0))
                .key("new-chat-name")
                .label("Group name")
                .placeholder("Group name (optional)", tone(p.text_muted))
                .color(tone(p.foreground))
                .w_full()
                .on_edit(|s: &mut UiState<A>| &mut s.chatlist.manage.new_chat.name),
        );
    }
    if !draft.chips.is_empty() {
        body = body.child(crate::screens::button("Start chat", "new-chat-start", |s: &mut UiState<A>, tx| start(s, tx)));
    }
    body
}

/// Opens the existing 1:1 with the one person picked, else asks the bridge for a new chat.
pub fn start<A: Actions>(s: &mut UiState<A>, tx: &mut Tx) {
    let draft = &s.chatlist.manage.new_chat;
    let ids: Vec<String> = draft.chips.iter().map(|chip| chip.user_id.clone()).collect();
    let topic = Some(draft.name.text.trim().to_owned()).filter(|name| !name.is_empty());
    if ids.is_empty() { return; }
    s.chatlist.manage.new_chat_open = false;
    if let [one] = ids.as_slice() {
        // A group of others is non-empty in `ChatDetails::group`; a 1:1 has none.
        let existing = s.chats.iter().find(|chat| {
            s.details.get(&chat.id).is_some_and(|d| d.group.is_empty() && d.members.len() == 2
                && d.members.iter().any(|m| m.user_id.as_deref() == Some(one.as_str())))
        });
        if let Some(chat) = existing {
            let id = chat.id.clone();
            open_chat(s, &id, tx);
            tx.focus("composer", false);
            return;
        }
    }
    s.actions.create_chat(ids, topic);
}
