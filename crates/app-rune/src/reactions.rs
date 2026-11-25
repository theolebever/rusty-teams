//! Message reactions: the chips under a message, and the quick picker that adds or removes one.
//! A chip and a picker entry both toggle the viewer's own reaction through `Actions::react`.

use crate::avatar::rgb;
use crate::types::{Actions, MessageRow};
use crate::ui::{UiState, ts};
use crate::view_messages::action_button;
use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

const HEART: &str = "\u{2764}";
const VARIATION_SELECTOR: char = '\u{FE0F}';
const PILL_HEIGHT: f32 = 24.0;
const WHITE: u32 = 0xFFFFFF;

/// The message whose quick picker is open. One at a time, cleared when the chat changes.
#[derive(Default)]
pub struct Picker {
    pub people: Option<(String, Result<Vec<teams_core::ReactionInfo>, String>)>,
    open: Option<String>,
    all: bool,
}

impl Picker {
    pub fn close(&mut self) {
        self.people = None;
        self.open = None;
        self.all = false;
    }

    fn toggle(&mut self, message_id: &str) {
        self.all = false;
        self.open = match self.open.take() {
            Some(open) if open == message_id => None,
            _ => Some(message_id.to_owned()),
        };
    }
}

pub fn people<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let (_, people) = s.picker.people.as_ref()?;
    let mut body = col().w_full().gap(8.0).padding(16.0).child(text("Reactions").font(ts(18.0, 600.0)));
    match people {
        Err(error) => body = body.child(text(error.clone()).font(ts(13.0, 400.0))),
        Ok(people) if people.is_empty() => body = body.child(text("No reactions")),
        Ok(people) => for person in people {
            let glyph = chatsvc::reaction_glyph_for_key(&person.reaction_type).unwrap_or_else(|| person.reaction_type.clone());
            let name = person.user_name.as_deref().or(person.user_id.as_deref()).unwrap_or("Unknown");
            body = body.child(text(format!("{glyph}  {name}")).font(ts(14.0, 400.0)));
        },
    }
    Some(crate::shortcuts::card("reaction-people-dialog", 420.0, body, |s: &mut UiState<A>| s.picker.people = None))
}

pub fn pick<A: Actions>(s: &mut UiState<A>, glyph: &str) -> bool {
    if !s.picker.all { return false; }
    if !s.read_only
        && let (Some(conversation), Some(id)) = (&s.selected, &s.picker.open)
        && let Some(message) = s.messages.iter().find(|message| &message.id == id)
    {
        s.actions.react(conversation, id, &wire(glyph), is_mine(message, glyph));
    }
    s.picker.close();
    true
}

/// The Teams quick set (like, heart, laugh, surprised) as glyphs.
pub fn quick_glyphs() -> Vec<String> {
    teams_core::TEAMS_DEFAULT_REACTIONS
        .iter()
        .filter_map(|key| chatsvc::reaction_glyph_for_key(key))
        .collect()
}

fn preferred_glyphs<A: Actions>(s: &UiState<A>) -> Vec<String> {
    let chosen: Vec<_> = s.settings.prefs.reactions.iter()
        .filter_map(|key| chatsvc::reaction_glyph_for_key(key)).collect();
    let glyphs = if chosen.is_empty() { quick_glyphs() } else { chosen };
    glyphs.iter().map(|glyph| app_base::emoticons::with_tone(glyph, s.settings.prefs.skin_tone)).collect()
}

fn bare(glyph: &str) -> String {
    glyph.chars().filter(|character| *character != VARIATION_SELECTOR).collect()
}

/// The reaction string Teams takes: the heart needs its variation selector, the summaries drop it.
pub fn wire(glyph: &str) -> String {
    if bare(glyph) == HEART {
        format!("{HEART}{VARIATION_SELECTOR}")
    } else {
        glyph.to_owned()
    }
}

/// Whether the viewer already has `glyph` on `message`, so picking it again removes it.
pub fn is_mine(message: &MessageRow, glyph: &str) -> bool {
    message
        .reactions
        .iter()
        .any(|reaction| reaction.mine && bare(&reaction.emoji) == bare(glyph))
}

/// The hover toolbar's react button: it opens or closes the quick set under the message.
pub fn trigger<A: Actions + 'static>(message: &MessageRow) -> El {
    let message_id = message.id.clone();
    action_button::<A>(format!("react-{}", message.id), "mood", move |s: &mut UiState<A>, _| s.picker.toggle(&message_id))
}

/// The quick set heading the message menu: picking one reacts (or takes the own one back) and
/// closes the menu.
pub fn quick_bar<A: Actions + 'static>(s: &UiState<A>, message: &MessageRow) -> El {
    let palette = active_palette();
    let conversation_id = s.selected.clone().unwrap_or_default();
    let mut bar = row().key(format!("menu-react-{}", message.id)).h(36.0).px(3.0).gap(1.0).center_y().radius(8.0)
        .fill(rgb(palette.popup)).border(1.0, rgb(palette.border_strong));
    for glyph in preferred_glyphs(s) {
        let (conversation_id, message_id) = (conversation_id.clone(), message.id.clone());
        let mine = is_mine(message, &glyph);
        let label = glyph.clone();
        let mut button = row().key(format!("menu-pick-{}-{glyph}", message.id)).size(30.0, 30.0).radius(6.0).center()
            .hover(rgb(palette.popup_hover))
            .on_press_tx(move |s: &mut UiState<A>, _| {
                crate::message_menu::close(s);
                s.actions.react(&conversation_id, &message_id, &wire(&glyph), mine);
            })
            .child(text(label).font(ts(18.0, 400.0)));
        if mine { button = button.fill(rgb(palette.reaction_on_own)); }
        bar = bar.child(button);
    }
    let message_id = message.id.clone();
    bar.child(crate::controls::button::<A>("menu-reaction-more", "+", crate::controls::Variant::Secondary, crate::controls::Size::Sm, move |s: &mut UiState<A>, tx| {
        crate::message_menu::close(s);
        s.picker.open = Some(message_id.clone());
        s.picker.all = true;
        s.emoji_open = false;
        s.compose.emoji.query.clear();
        tx.focus("emoji-search", false);
    }))
}

/// The reaction pills of a message, plus the quick set when it is open. Read-only views show the
/// pills without the quick set.
pub fn controls<A: Actions + 'static>(s: &UiState<A>, message: &MessageRow) -> El {
    let conversation_id = s.selected.clone().unwrap_or_default();
    let palette = active_palette();
    let mut chips = row().gap(4.0).center_y();
    for reaction in &message.reactions {
        let glyph = reaction.emoji.clone();
        let (fill, ink) = if reaction.mine {
            (palette.accent, WHITE)
        } else if message.mine {
            (palette.reaction_on_own, palette.text_soft)
        } else {
            (palette.border_strong, palette.text_soft)
        };
        let mut chip = row()
            .key(format!("reaction-{}-{glyph}", message.id))
            .h(PILL_HEIGHT)
            .pl(5.0)
            .pr(8.0)
            .gap(3.0)
            .center_y()
            .radius(PILL_HEIGHT / 2.0)
            .fill(rgb(fill))
            .child(text(glyph.clone()).font(ts(14.0, 400.0)).color(rgb(ink)))
            .child(text(reaction.count.to_string()).font(ts(12.0, 400.0)).color(rgb(ink)));
        if !s.read_only {
            let (conversation_id, message_id, mine) = (conversation_id.clone(), message.id.clone(), reaction.mine);
            chip = chip.hover(rgb(palette.row_hover)).on_press_tx(move |s: &mut UiState<A>, _| {
                s.actions.react(&conversation_id, &message_id, &wire(&glyph), mine)
            });
        }
        chips = chips.child(chip);
    }
    if s.read_only {
        return chips;
    }
    let mut column = col().gap(4.0).child(chips);
    if s.picker.open.as_deref() == Some(message.id.as_str()) {
        let mut quick = row().key(format!("quick-{}", message.id)).gap(4.0);
        for glyph in preferred_glyphs(s) {
            let (conversation_id, message_id) = (conversation_id.clone(), message.id.clone());
            let mine = is_mine(message, &glyph);
            let label = glyph.clone();
            quick = quick.child(
                row()
                    .key(format!("pick-{}-{glyph}", message.id))
                    .padding(4.0)
                    .radius(PILL_HEIGHT / 2.0)
                    .fill(rgb(if mine { palette.reaction_on_own } else { palette.border_strong }))
                    .hover(rgb(palette.row_hover))
                    .on_press_tx(move |s: &mut UiState<A>, _| {
                        s.picker.close();
                        s.actions.react(&conversation_id, &message_id, &wire(&glyph), mine);
                    })
                    .child(text(label).font(ts(14.0, 400.0))),
            );
        }
        column = column.child(quick);
        column = column.child(crate::controls::button::<A>("reaction-more", "More emoji", crate::controls::Variant::Secondary, crate::controls::Size::Sm, |s: &mut UiState<A>, tx| {
            s.picker.all = !s.picker.all;
            s.emoji_open = false;
            s.compose.emoji.query.clear();
            if s.picker.all { tx.focus("emoji-search", false); }
        }));
        if s.picker.all { column = column.child(crate::emoji_picker::picker(s)); }
    }
    column
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SendTarget, Update};
    use rune::{Env, Runtime};
    use std::cell::RefCell;
    use teams_core::ReactionSummary;

    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<String>>,
    }

    impl Actions for Fake {
        fn open(&self, _conversation_id: &str) {}
        fn send(&self, _target: SendTarget, _text: String, _files: Vec<teams_core::FileReference>) {}
        fn react(&self, conversation_id: &str, message_id: &str, reaction: &str, remove: bool) {
            self.calls
                .borrow_mut()
                .push(format!("react {conversation_id} {message_id} {reaction} {remove}"));
        }
    }

    fn msg(id: &str, reactions: Vec<ReactionSummary>) -> MessageRow {
        MessageRow {
            forwarded: false, importance: Default::default(),
            sender_id: None,
            system: false,
            id: id.into(),
            reply_to_id: None,
            deleted: false,
            can_delete: false,
            reactions,
            sender: "Ada".into(),
            when: "14:06".into(),
            text: "hi".into(),
            mine: false,
            media: Default::default(),
        }
    }

    fn summary(emoji: &str, count: usize, mine: bool) -> ReactionSummary {
        ReactionSummary { emoji: emoji.into(), count, mine }
    }

    fn rt() -> Runtime<UiState<Fake>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Fake::default()), crate::ui::view::<Fake>, (1000.0, 700.0), env);
        rt.frame(0.0);
        rt.update(|s, _| s.selected = Some("a".into()));
        rt
    }

    fn show(rt: &mut Runtime<UiState<Fake>>, rows: Vec<MessageRow>) {
        rt.update(move |s, _| s.apply(Update::Messages { conversation_id: "a".into(), rows }));
        rt.frame(1.0 / 60.0);
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn calls(rt: &Runtime<UiState<Fake>>) -> Vec<String> {
        rt.state.actions.calls.borrow().clone()
    }

    #[test]
    fn quick_set_is_the_four_teams_defaults() {
        assert_eq!(
            quick_glyphs(),
            ["\u{1F44D}", "\u{2764}", "\u{1F606}", "\u{1F62E}"]
        );
    }

    #[test]
    fn the_full_picker_reacts_without_modifying_the_draft() {
        let mut rt = rt();
        show(&mut rt, vec![msg("m", vec![])]);
        rt.update(|s, _| s.composer = TextEdit::new("keep this draft"));
        click(&mut rt, "react-m");
        click(&mut rt, "reaction-more");
        rt.update(|s, _| s.compose.emoji.query = TextEdit::new("rocket"));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "emoji-🚀");
        assert_eq!(calls(&rt), ["react a m 🚀 false"]);
        assert_eq!(rt.state.composer.text, "keep this draft");
        assert!(rt.find("quick-m").is_none());
    }

    #[test]
    fn quick_reactions_use_the_account_choices_and_skin_tone() {
        let mut s = UiState::new(Fake::default());
        s.settings.prefs.reactions = vec!["like".into(), "clap".into()];
        s.settings.prefs.skin_tone = 3;
        assert_eq!(preferred_glyphs(&s), ["👍🏽", "👏🏽"]);
    }

    #[test]
    fn the_context_menu_can_open_the_full_reaction_picker() {
        let mut rt = rt();
        show(&mut rt, vec![msg("m", vec![])]);
        click(&mut rt, "more-m");
        click(&mut rt, "menu-reaction-more");
        assert!(rt.find("emoji-search").is_some());
        assert!(rt.find("message-menu").is_none());
        click(&mut rt, "emoji-😀");
        assert_eq!(calls(&rt), ["react a m 😀 false"]);
    }

    #[test]
    fn only_the_heart_gets_the_variation_selector() {
        assert_eq!(wire("\u{2764}"), "\u{2764}\u{FE0F}");
        assert_eq!(wire("\u{2764}\u{FE0F}"), "\u{2764}\u{FE0F}");
        assert_eq!(wire("\u{1F44D}"), "\u{1F44D}");
    }

    #[test]
    fn own_reaction_matches_whatever_the_selector() {
        let mine = msg("m", vec![summary("\u{2764}", 2, true)]);
        assert!(is_mine(&mine, "\u{2764}\u{FE0F}"));
        let theirs = msg("m", vec![summary("\u{2764}", 2, false)]);
        assert!(!is_mine(&theirs, "\u{2764}"));
    }

    #[test]
    fn picking_a_quick_reaction_adds_it_and_closes_the_picker() {
        let mut rt = rt();
        show(&mut rt, vec![msg("m", vec![])]);
        click(&mut rt, "react-m");
        click(&mut rt, "pick-m-\u{1F44D}");
        assert_eq!(calls(&rt), ["react a m \u{1F44D} false"]);
        assert!(rt.find("quick-m").is_none());
    }

    #[test]
    fn clicking_an_own_chip_removes_it() {
        let mut rt = rt();
        show(&mut rt, vec![msg("m", vec![summary("\u{1F44D}", 1, true)])]);
        click(&mut rt, "reaction-m-\u{1F44D}");
        assert_eq!(calls(&rt), ["react a m \u{1F44D} true"]);
    }

    #[test]
    fn picking_an_own_quick_reaction_removes_it() {
        let mut rt = rt();
        show(&mut rt, vec![msg("m", vec![summary("\u{1F44D}", 1, true)])]);
        click(&mut rt, "react-m");
        click(&mut rt, "pick-m-\u{1F44D}");
        assert_eq!(calls(&rt), ["react a m \u{1F44D} true"]);
    }

    #[test]
    fn read_only_shows_chips_without_controls() {
        let mut rt = rt();
        rt.update(|s, _| s.read_only = true);
        show(&mut rt, vec![msg("m", vec![summary("\u{1F44D}", 1, false)])]);
        assert!(rt.find("react-m").is_none());
        click(&mut rt, "reaction-m-\u{1F44D}");
        assert!(calls(&rt).is_empty());
    }
}
