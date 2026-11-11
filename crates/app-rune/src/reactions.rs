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
    open: Option<String>,
}

impl Picker {
    pub fn close(&mut self) {
        self.open = None;
    }

    fn toggle(&mut self, message_id: &str) {
        self.open = match self.open.take() {
            Some(open) if open == message_id => None,
            _ => Some(message_id.to_owned()),
        };
    }
}

/// The Teams quick set (like, heart, laugh, surprised) as glyphs.
pub fn quick_glyphs() -> Vec<String> {
    // ponytail: the account's own default set (defaultMessageReactions) is not read; add when needed.
    teams_core::TEAMS_DEFAULT_REACTIONS
        .iter()
        .filter_map(|key| chatsvc::reaction_glyph_for_key(key))
        .collect()
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
        for glyph in quick_glyphs() {
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
