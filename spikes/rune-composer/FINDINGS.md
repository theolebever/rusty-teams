# Rune spike: composer

Question: can Rune (rev `7ed827a`) carry the Teams composer, risk #1 in `spikes/rune-teams/FINDINGS.md`? For the core of it, yes. Rune ships a rich text editor, `rune::TextEdit` (about 4,100 lines in `crates/rune/src/edit*`), that `text_input` draws. The spike is one file, `src/main.rs` (386 lines with tests). `composer.png` is a headless render of the demo draft.

## What Rune provides

| Need | Rune primitive | Spike result |
|---|---|---|
| Multi-line, Enter sends, Shift+Enter breaks the line | `text_input` + `.on_submit`. `TextEdit::key` sends on Return in a paragraph and inserts a soft break on Shift+Return | Tested |
| Ctrl+B / I / U | `TextEdit::rich()` with `format_chord` handles them | Tested (B, U) |
| Format bar | `toggle_mark`, `has_mark`; `tx.focus("composer")` returns focus | Tested (I button) |
| Atomic @mention | `Mark::Pill(id)`, `replace_with_pill`, `pills()`, chip drawn via `.pill_colors` / `.pill_style` | Tested: the caret steps over it, two Backspaces delete space + pill, and Ctrl+Z brings both back as one step |
| Mention picker at the caret | `Cx::caret("composer")` returns the caret rect | Tested: popup appears and filters `@ad` |
| Pasted link | `.on_input_paste` + `set_mark(Mark::Link)` | Tested: link on paste, and Ctrl+Z drops the link and keeps the text, as GPUI's test expects |
| Lists, quote, code block | `Block::{Bullet, Numbered, Quote, Code}`, Tab / Shift+Tab nesting, Return continues a list | Bullets tested via API only |
| Undo / redo | `TextEdit` history with Ctrl+Z, Ctrl+Y, Ctrl+Shift+Z | Tested |
| IME | Focused field turns the IME on. `Runtime::ime_area` is the candidate rect. Preedit stays out of `text` until commit | Tested headless: preedit, commit, area moves with the caret |
| Rich paste | `paste_html`, `paste_html_with_pills` | Not tested |
| Auto height | `auto_grow(field, 1, 8)` | Rendered, not measured |

## HTML sent

The spike does not use Rune's `to_html` (it writes `<p>` and `<pre><code>`, which is not Teams HTML). `teams_html` maps `TextEdit` paragraphs and marks to `teams_core::DraftLine`s, then calls the functions the GPUI composer calls today: `Draft::from_lines(..).trimmed().to_html()` and `apply_mentions`. Pills stay `@Name` text in the draft and become `<at id="n">Name</at>`. The spike depends on `crates/core` by path, so the output comes from the same code. Only the inline image and `emoticon_tags` steps of `Outgoing::html` are left out. Asserted outputs:

- `line one<br>line two`
- `a<b>b</b>c`, then `<i>a</i><b><i>b</i></b><i>c</i>` after select all + Italic
- `Hi <at id="0">Ada Example</at>`
- `see <a href="https://example.com/x">https://example.com/x</a>.`
- `Plan for <b>Friday</b>, <at id="0">Ada Example</at> see <a href="https://example.com/plan">https://example.com/plan</a><ul><li>one &amp; &lt;two&gt;</li><li>three</li></ul>`

## What Rune lacks

The GPUI toolbar (`format_toolbar.rs`, 27 buttons) and `teams_core::Draft` hold more than `TextEdit` can. `Mark` and `Block` are closed enums inside Rune, so the app cannot add variants.

| Missing in `TextEdit` | GPUI feature | Rough cost |
|---|---|---|
| Marks: superscript, subscript, font size, font colour, highlight | Toolbar buttons and colour menus | Rune patch: new `Mark` variants plus drawing in the field, about 300 to 500 lines |
| Blocks: heading 1-3, indent levels | Paragraph style menu, indent | Rune patch, about 200 lines |
| Inline objects with their own size: tables, horizontal rule, inline images | `table.rs` (813 lines), `rule.rs`, pasted images | No primitive. A pill draws as a chip, not as an image or a grid |
| Numbered list start (`<ol start>`) | Pasted lists | Small |

Paths for the gap:

1. Patch or fork Rune for the extra marks and blocks (about 500 to 700 lines), and keep tables and images out of the text flow (a table editor and an image tray under the field). The second half changes Teams' behaviour: Teams shows tables and images inline.
2. Write a custom editor widget on `Draft`: Rune text layout with `rich()` spans, our own caret, selection, hit testing and key handling, with `El::ime_cursor` for the IME. Rune's own editor is about 4,100 lines without rendering, so expect 4,000 to 6,000 lines.

## Not tested

- Window mode. Nothing was typed on a real keyboard, and no real Windows IME (TSF, Japanese, Chinese) was used. IME was checked only through `Runtime::ime` and `ime_area`.
- Rich paste from Word, a browser or Teams, drag and drop, spellcheck, screen reader output.
- Ctrl+K link editor, emoji autocomplete, Markdown typing shortcuts (`**`, `- `), the slash menu, drafts, editing a sent message. Rune passes keys to the app first (`on_input_key`), so these live in app code, but none was built.
- Lists by keyboard (Return continuing a list, Tab nesting). Known from the source only.
- Performance with long drafts.

## Build

`cargo test`: 6 passed, no warnings. The first build of `teams-core` and its dependencies (rusqlite and others) took 255 s on top of an already compiled Rune. A rebuild after an edit takes 8 to 25 s. It needs the same MSVC `PATH` / `LIB` / `INCLUDE` setup as the other spikes: MSVC `bin\Hostx64\x64` first on `PATH`, UCRT and UM in `LIB` and `INCLUDE`. The spike was built with `CARGO_TARGET_DIR` set to `spikes/rune-teams/target` to reuse that spike's compiled Rune.
