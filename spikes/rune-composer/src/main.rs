//! Spike: the Teams composer on Rune's rich `TextEdit`, sending the same HTML as the GPUI
//! composer. `cargo run` opens the window; `cargo run -- --snap composer.png --size 900x420
//! --scale 1` renders it headless with a demo draft. `cargo test` runs the headless checks.

use rune::tok::Family;
use rune::*;
use teams_core::{Draft, DraftLine, LineKind, MarkKind, MentionInput, apply_mentions, link_url};

const PURPLE: &str = "#5B5FC7";
const LAVENDER: &str = "#E8EBFA";
const LINE: &str = "#E0E0E0";
const INK: &str = "#242424";
const MUTED: &str = "#616161";
const WHITE: &str = "#FFFFFF";
const AUTOLINK_PREFIXES: [&str; 4] = ["http://", "https://", "www.", "mailto:"];
const AUTOLINK_TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', '\'', '"'];
/// Stand-in for the people search the real composer runs.
const PEOPLE: [(&str, &str); 3] = [("u1", "Ada Example"), ("u2", "Bo Lindqvist"), ("u3", "Adrian Moe")];

fn c(s: &str) -> Color {
    rune::color::hex(s).expect("hex colour")
}

fn ts(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Segoe UI"))
}

fn empty_draft() -> TextEdit {
    // Marks the Teams HTML path can carry; Rune's quote/list/code blocks map to Draft lines.
    TextEdit::new("").rich().allow_marks([
        Mark::Bold,
        Mark::Italic,
        Mark::Underline,
        Mark::Strike,
        Mark::Code,
        Mark::Link(String::new()),
    ])
}

struct State {
    draft: TextEdit,
    sent: Vec<String>,
}

impl State {
    fn new() -> Self {
        State { draft: empty_draft(), sent: Vec::new() }
    }

    fn demo() -> Self {
        let mut s = State::new();
        s.draft.insert("Plan for ");
        s.draft.toggle_mark(Mark::Bold);
        s.draft.insert("Friday");
        s.draft.toggle_mark(Mark::Bold);
        s.draft.insert(", ");
        s.draft.insert_pill("u1", "@Ada Example");
        s.draft.insert(" see ");
        paste_linked(&mut s.draft, "https://example.com/plan");
        s
    }
}

/// The `@query` right before the caret, as the byte range to replace and the query.
fn mention_query(edit: &TextEdit) -> Option<(std::ops::Range<usize>, String)> {
    let (a, b) = edit.range();
    if a != b {
        return None;
    }
    let before = &edit.text[..a];
    let at = before.rfind('@')?;
    let query = &before[at + 1..];
    let opens = before[..at].chars().next_back().is_none_or(char::is_whitespace);
    (opens && !query.contains(char::is_whitespace) && !edit.pills().any(|(r, _)| r.contains(&at)))
        .then(|| (at..a, query.to_lowercase()))
}

fn candidates(query: &str) -> Vec<(&'static str, &'static str)> {
    PEOPLE.into_iter().filter(|(_, name)| name.to_lowercase().starts_with(query)).collect()
}

fn pick_mention(s: &mut State, id: &str, name: &str) {
    if let Some((range, _)) = mention_query(&s.draft) {
        s.draft.replace_with_pill(range, id, &format!("@{name}"));
        s.draft.insert(" ");
    }
}

/// Paste plain text and link each URL word in it, as the GPUI composer's paste does.
fn paste_linked(edit: &mut TextEdit, text: &str) {
    let start = edit.range().0;
    edit.paste(text);
    let end = edit.focus;
    let mut links = Vec::new();
    let mut offset = start;
    for word in edit.text[start..end].split(|c: char| c.is_whitespace()) {
        let trimmed = word.trim_end_matches(AUTOLINK_TRAILING);
        let lower = trimmed.to_ascii_lowercase();
        if AUTOLINK_PREFIXES.iter().any(|p| lower.starts_with(p) && lower.len() > p.len())
            && let Some(url) = link_url(trimmed)
        {
            links.push((offset, offset + trimmed.len(), url));
        }
        offset += word.len() + 1;
    }
    // `set_mark` on a selection is its own undo step, so Ctrl+Z first unlinks, as in GPUI.
    edit.undo_group(|edit| {
        for (a, b, url) in links {
            edit.set_selection(a, b);
            edit.set_mark(Mark::Link(url));
        }
    });
    edit.set_selection(end, end);
}

/// The GPUI composer's send body: Rune's model as `teams_core::Draft` lines, then
/// `Draft::trimmed().to_html()` and `apply_mentions`, the exact functions it calls today.
fn teams_html(edit: &TextEdit) -> String {
    let mut mentions = Vec::new();
    for (range, id) in edit.pills() {
        mentions.push(MentionInput::user(id, edit.text[range].trim_start_matches('@')));
    }
    let lines: Vec<DraftLine> = edit
        .paragraphs()
        .into_iter()
        .map(|p| {
            let marks = edit
                .marks()
                .iter()
                .filter_map(|m| {
                    let kind = match &m.mark {
                        Mark::Bold => MarkKind::Bold,
                        Mark::Italic => MarkKind::Italic,
                        Mark::Underline => MarkKind::Underline,
                        Mark::Strike => MarkKind::Strike,
                        Mark::Code => MarkKind::Code,
                        Mark::Link(url) => MarkKind::Link(url.clone()),
                        _ => return None, // pills stay plain `@Name` text for apply_mentions
                    };
                    let (a, b) = (m.start.max(p.range.start), m.end.min(p.range.end));
                    (a < b).then(|| teams_core::Mark { range: a - p.range.start..b - p.range.start, kind })
                })
                .collect();
            let kind = match p.block {
                Block::Bullet(l) => LineKind::Bullet(l.min(teams_core::MAX_DEPTH)),
                Block::Numbered(l) => LineKind::Numbered(l.min(teams_core::MAX_DEPTH)),
                Block::Quote => LineKind::Quote,
                Block::Code => LineKind::Code(None),
                _ => LineKind::Text,
            };
            DraftLine { kind, number: None, content: edit.text[p.range].to_owned(), marks }
        })
        .collect();
    let html = Draft::from_lines(&lines).trimmed().to_html();
    apply_mentions(&html, &mentions).0
}

fn send(s: &mut State) {
    if s.draft.text.trim().is_empty() {
        return;
    }
    s.sent.push(teams_html(&s.draft));
    s.draft = empty_draft();
}

fn format_button(s: &State, key: &str, label: &str, mark: Mark) -> El {
    let on = s.draft.has_mark(&mark);
    let weight = if matches!(mark, Mark::Bold) { 700.0 } else { 400.0 };
    let style = TextStyle { italic: matches!(mark, Mark::Italic), ..ts(14.0, weight) };
    let t = text(label).font(style).color(c(INK));
    row()
        .key(key)
        .size(28.0, 28.0)
        .radius(4.0)
        .center()
        .fill(c(if on { LAVENDER } else { WHITE }))
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            s.draft.toggle_mark(mark.clone());
            tx.focus("composer", false);
        })
        .child(t)
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let bar = row()
        .gap(2.0)
        .child(format_button(s, "fmt-bold", "B", Mark::Bold))
        .child(format_button(s, "fmt-italic", "I", Mark::Italic))
        .child(format_button(s, "fmt-underline", "U", Mark::Underline));
    let field = text_input(&s.draft, ts(14.0, 400.0))
        .key("composer")
        .label("Message")
        .placeholder("Type a message", c(MUTED))
        .color(c(INK))
        .w_full()
        .pill_colors(c(LAVENDER), Some(c(PURPLE)))
        .on_edit(|s: &mut State| &mut s.draft)
        .on_submit(|s: &mut State, _| send(s))
        .on_input_paste(|s: &mut State, content: &ClipboardContent, _| {
            paste_linked(&mut s.draft, content.text());
            true
        });
    let mut root = col()
        .w_full().h_full()
        .padding(16.0)
        .gap(8.0)
        .fill(c("#F5F5F5"))
        .children(s.sent.iter().map(|html| text(html.clone()).font(ts(12.0, 400.0)).color(c(MUTED))))
        .child(
            col()
                .w_full()
                .padding(8.0)
                .gap(6.0)
                .radius(8.0)
                .fill(c(WHITE))
                .border(1.0, c(LINE))
                .child(bar)
                .child(auto_grow(field, 1, 8)),
        );
    // Mention picker, placed at the caret the way the GPUI popup is.
    if let Some((_, query)) = mention_query(&s.draft)
        && let Some([x, y, _, h]) = cx.caret("composer")
    {
        let list = col().absolute(x, y + h + 4.0).padding(4.0).radius(6.0).fill(c(WHITE)).border(1.0, c(LINE)).children(
            candidates(&query).into_iter().map(|(id, name)| {
                row()
                    .key(format!("mention-{id}"))
                    .px(8.0)
                    .py(4.0)
                    .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                        pick_mention(s, id, name);
                        tx.focus("composer", false);
                    })
                    .child(text(name).font(ts(13.0, 400.0)).color(c(INK)))
            }),
        );
        root = root.child(list);
    }
    root
}

fn main() {
    App::new(AppSpec::new("Composer", (900.0, 420.0), State::demo, view)).run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    fn rt() -> Runtime<State> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(State::new(), view, (900.0, 420.0), env);
        rt.frame(0.0);
        click(&mut rt, "composer");
        rt
    }

    fn click(rt: &mut Runtime<State>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn press(rt: &mut Runtime<State>, key: Key, mods: Mods) {
        rt.key(KeyEvent { key, mods });
        rt.frame(1.0 / 60.0);
    }

    fn typed(rt: &mut Runtime<State>, s: &str) {
        press(rt, Key::Text(s.into()), Mods::default());
    }

    fn ctrl(rt: &mut Runtime<State>, ch: char) {
        press(rt, Key::Command(ch), Mods { cmd: true, ctrl: true, ..Mods::default() });
    }

    #[test]
    fn enter_sends_and_shift_enter_breaks_the_line() {
        let mut rt = rt();
        typed(&mut rt, "line one");
        press(&mut rt, Key::Enter, Mods { shift: true, ..Mods::default() });
        typed(&mut rt, "line two");
        assert_eq!(rt.state.draft.text, "line one\nline two");
        assert!(rt.state.sent.is_empty());
        press(&mut rt, Key::Enter, Mods::default());
        assert_eq!(rt.state.sent, ["line one<br>line two"]);
        assert!(rt.state.draft.text.is_empty());
    }

    #[test]
    fn ctrl_b_and_the_format_bar_toggle_marks() {
        let mut rt = rt();
        typed(&mut rt, "a");
        ctrl(&mut rt, 'b');
        typed(&mut rt, "b");
        ctrl(&mut rt, 'b');
        typed(&mut rt, "c");
        assert_eq!(teams_html(&rt.state.draft), "a<b>b</b>c");
        ctrl(&mut rt, 'a');
        click(&mut rt, "fmt-italic");
        assert_eq!(teams_html(&rt.state.draft), "<i>a</i><b><i>b</i></b><i>c</i>");
        // Focus came back to the field: Ctrl+U still formats.
        ctrl(&mut rt, 'u');
        assert_eq!(teams_html(&rt.state.draft), "<i><u>a</u></i><b><i><u>b</u></i></b><i><u>c</u></i>");
    }

    #[test]
    fn a_mention_is_a_pill_that_backspace_deletes_whole() {
        let mut rt = rt();
        typed(&mut rt, "Hi @ad");
        assert!(rt.find("mention-u1").is_some() && rt.find("mention-u3").is_some());
        assert!(rt.find("mention-u2").is_none());
        click(&mut rt, "mention-u1");
        assert_eq!(rt.state.draft.text, "Hi @Ada Example ");
        assert_eq!(teams_html(&rt.state.draft), "Hi <at id=\"0\">Ada Example</at>");
        // The caret cannot land inside the pill.
        press(&mut rt, Key::Left, Mods::default());
        press(&mut rt, Key::Left, Mods::default());
        assert_eq!(rt.state.draft.focus, 3);
        press(&mut rt, Key::End, Mods::default());
        press(&mut rt, Key::Backspace, Mods::default());
        press(&mut rt, Key::Backspace, Mods::default());
        assert_eq!(rt.state.draft.text, "Hi ");
        assert_eq!(rt.state.draft.pills().count(), 0);
        // Both backspaces are one undo step.
        ctrl(&mut rt, 'z');
        assert_eq!(rt.state.draft.text, "Hi @Ada Example ");
        assert_eq!(rt.state.draft.pills().count(), 1);
    }

    #[test]
    fn a_pasted_url_becomes_a_link() {
        let mut rt = rt();
        rt.paste("see https://example.com/x.");
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.draft.text, "see https://example.com/x.");
        assert_eq!(rt.state.draft.active_link(), None);
        assert_eq!(
            teams_html(&rt.state.draft),
            "see <a href=\"https://example.com/x\">https://example.com/x</a>."
        );
        // As in GPUI: Ctrl+Z keeps the pasted text and drops the link.
        ctrl(&mut rt, 'z');
        assert_eq!(rt.state.draft.text, "see https://example.com/x.");
        assert_eq!(teams_html(&rt.state.draft), "see https://example.com/x.");
    }

    #[test]
    fn html_matches_the_gpui_composer_for_mixed_content() {
        let mut s = State::demo();
        s.draft.insert("\n");
        s.draft.toggle_block(Block::Bullet(0));
        s.draft.insert("one & <two>");
        s.draft.insert("\n");
        s.draft.toggle_block(Block::Bullet(0)); // `insert("\n")` does not continue a list; Return does
        s.draft.insert("three");
        assert_eq!(
            teams_html(&s.draft),
            "Plan for <b>Friday</b>, <at id=\"0\">Ada Example</at> see \
             <a href=\"https://example.com/plan\">https://example.com/plan</a>\
             <ul><li>one &amp; &lt;two&gt;</li><li>three</li></ul>"
        );
    }

    #[test]
    fn ime_composes_at_the_caret_and_undo_redo_work() {
        let mut rt = rt();
        typed(&mut rt, "ab");
        rt.ime(Ime::Preedit("か".into(), Some((3, 3))));
        rt.frame(1.0 / 60.0);
        let first = rt.ime_area.expect("IME area while composing");
        assert_eq!(rt.state.draft.text, "ab", "preedit is not part of the text");
        rt.ime(Ime::Commit("漢字".into()));
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.draft.text, "ab漢字");
        typed(&mut rt, "wwww");
        let later = rt.ime_area.expect("IME area after typing");
        assert!(later[0] > first[0], "IME area follows the caret: {first:?} -> {later:?}");
        ctrl(&mut rt, 'z');
        assert_ne!(rt.state.draft.text, "ab漢字wwww");
        ctrl(&mut rt, 'y');
        assert_eq!(rt.state.draft.text, "ab漢字wwww");
    }
}
