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
    pub first_message: TextEdit,
    pub creating: bool,
    pub error: Option<String>,
    pub chips: Vec<Person>,
    pub results: Vec<Person>,
}

impl Default for Draft {
    fn default() -> Self {
        Draft { query: TextEdit::new(""), name: TextEdit::new(""), first_message: TextEdit::default(), creating: false, error: None, chips: Vec::new(), results: Vec::new() }
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
    for draft in [&mut s.chatlist.manage.new_chat, &mut s.chatlist.manage.add, &mut s.meeting.people] {
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
                        .children(person.mail.clone().map(|mail| text(mail).font(ts(12.0, 400.0)).color(tone(p.text_muted)).truncate())),
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
        .radius(8.0)
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
        body = body.child(text_input(&draft.first_message, ts(14.0, 400.0)).key("new-chat-first-message").label("First message")
            .placeholder("First message (optional)", tone(p.text_muted)).w_full().on_edit(|s: &mut UiState<A>| &mut s.chatlist.manage.new_chat.first_message))
            .children(draft.error.as_ref().map(|error| text(error.clone()).key("new-chat-error").font(ts(12.0, 400.0))))
            .child(if draft.creating {
                // Keep a press target so the click does not fall through to the menu backdrop,
                // while the Quiet variant carries no action and leaves the tab order.
                crate::controls::button::<A>("new-chat-start", "Creating…", crate::controls::Variant::Quiet, crate::controls::Size::Md, |_, _| {})
                    .on_press(|_: &mut UiState<A>| {})
            } else {
                crate::controls::button::<A>("new-chat-start", "Start chat", crate::controls::Variant::Primary, crate::controls::Size::Md, |s: &mut UiState<A>, tx| start(s, tx))
            });
    }
    body
}

/// Opens the existing 1:1 with the one person picked, else asks the bridge for a new chat.
pub fn start<A: Actions>(s: &mut UiState<A>, tx: &mut Tx) {
    if s.read_only || s.chatlist.manage.new_chat.creating { return; }
    let draft = &s.chatlist.manage.new_chat;
    let ids: Vec<String> = draft.chips.iter().map(|chip| chip.user_id.clone()).collect();
    let topic = Some(draft.name.text.trim().to_owned()).filter(|name| !name.is_empty());
    let first_message = draft.first_message.text.trim().to_owned();
    if ids.is_empty() { return; }
    if let [one] = ids.as_slice() {
        // A group of others is non-empty in `ChatDetails::group`; a 1:1 has none.
        let existing = s.chats.iter().find(|chat| {
            s.details.get(&chat.id).is_some_and(|d| d.group.is_empty() && d.members.len() == 2
                && d.members.iter().any(|m| m.user_id.as_deref() == Some(one.as_str())))
        });
        if let Some(chat) = existing {
            let id = chat.id.clone();
            open_chat(s, &id, tx);
            s.chatlist.manage.new_chat_open = false;
            if !first_message.is_empty() {
                s.actions.send_composed(crate::types::SendTarget { conversation_id: id, thread_root_id: None, channel: false }, crate::types::Composed { text: first_message, ..Default::default() }, vec![]);
            }
            tx.focus("composer", false);
            return;
        }
    }
    s.chatlist.manage.new_chat.creating = true;
    s.chatlist.manage.new_chat.error = None;
    s.actions.create_chat(ids, topic, first_message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_backend::schemes::active_palette;
    use rune::{ElementInfo, Env, Runtime};

    struct Quiet;
    impl Actions for Quiet {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
    }

    fn start_view(s: &UiState<Quiet>, _: &mut Cx<UiState<Quiet>>) -> El {
        panel(s)
    }

    fn runtime(creating: bool) -> Runtime<UiState<Quiet>> {
        let mut state = UiState::new(Quiet);
        state.chatlist.manage.new_chat.chips.push(Person { user_id: "u".into(), name: "Ada".into(), mail: None });
        state.chatlist.manage.new_chat.creating = creating;
        let mut rt = Runtime::new(state, start_view, (1000.0, 700.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        rt
    }

    fn by_key<'a>(elements: &'a [ElementInfo], key: &str) -> &'a ElementInfo {
        elements
            .iter()
            .find(|e| e.key.as_deref() == Some(key) || e.path.ends_with(&format!("/{key}")))
            .unwrap_or_else(|| panic!("no element {key}"))
    }

    #[test]
    fn start_chat_is_a_focusable_primary_button() {
        let rt = runtime(false);
        let elements = rt.elements();
        let e = by_key(&elements, "new-chat-start");
        assert_eq!(e.role, "button", "the action has a button role");
        assert_eq!(e.label.as_deref(), Some("Start chat"));
        assert!(e.focusable, "the primary action is keyboard reachable");
        let accent = rune::color::hex(&format!("#{:06X}", active_palette().accent)).expect("accent colour");
        assert_eq!(e.fill, accent, "a primary action uses the accent fill");
        assert!((e.radius - 6.0).abs() < 0.5, "radius {}", e.radius);
    }

    #[test]
    fn creating_shows_a_dimmed_waiting_button_without_an_action() {
        let rt = runtime(true);
        let elements = rt.elements();
        let e = by_key(&elements, "new-chat-start");
        assert_eq!(e.label.as_deref(), Some("Creating…"));
        assert!(!e.focusable, "a busy action leaves the tab order");
        assert!(e.opacity < 1.0, "a busy action is dimmed");
        assert_eq!(e.fill.a, 0.0, "a busy action has no resting fill");
    }
}
