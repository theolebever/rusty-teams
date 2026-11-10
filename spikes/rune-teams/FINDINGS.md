# Rune spike: Teams shell

The spike renders the Teams chat layout in Rune (rev `7ed827a`) with static demo data. It covers the title bar, app rail, chat list, conversation and composer. See `screenshot.png` (headless, 1600x900).

## Status

- `cargo build` passes with no warnings. One headless test selects a chat, sends a message and checks that the message is appended.
- Window mode was not launched. Typing into the composer and pressing Enter were not tested.
- First debug build with dependencies: about 1 min 38 s. Release build: about 2 min.
- Binary size: 26.5 MB release, 51.5 MB debug.

## Risks for a full port

1. Composer. `text_input` handles plain single-line text. The GPUI composer is 5459 lines, with a format bar, link editor, mention pills, attachments and HTML send. Rune has `TextArea`, `rich()` spans and `ime_cursor()` for a custom editor, but none of that was tried here.
2. Long lists. `virtual_rows` needs a growth stack as the first child of the scroll viewport. The spike shows 8 messages, so virtualization is untested.
3. Title bar. Headless mode draws Rune's native caption buttons, so the spike has no custom min/max/close. Dragging and caption behaviour on a real Windows window are unchecked.
4. Dead clicks. Shared, `+`, filter, new chat, add people, pop-out, `...` and the composer icons have no action. GUARDRAILS.md forbids that, so each one needs an action or has to go before shipping.
5. Toolchain. On this machine Git's coreutils `link.exe` shadows the MSVC linker, and `vcvars64.bat` fails on `vswhere`. The build works with the MSVC `bin` directory first on `PATH` and `LIB`/`INCLUDE` set by hand. Cargo prints the package as `rune-staging`.

## What mapped well

- Flexbox covers the three panes. `scroll` with `stick_bottom` plus `justify_end` keeps messages at the bottom.
- Absolute positioning handles the presence dot, unread badge and selection bar.
- `truncate()` with `min_w(0)` ellipsizes names and previews.
- `Icon` has public fields, so inline SVG icons work. All 25 icons rendered on the first try.
- `App::titlebar` with `window_drag` and `chrome_insets()` works as the Rune example shows.

## Mapping the GPUI views

| GPUI file | Lines | Port effort |
|---|---|---|
| `crates/app/src/views/rail.rs` | 307 | Close to a line-for-line rewrite. |
| `crates/app/src/views/sidebar.rs` | 2222 | The basic list ports quickly. Context menus, drag between sections and the team/channel tree take most of the time. |
| `crates/app/src/views/composer.rs` | 5459 | Hardest. Only the plain send path is covered. |
