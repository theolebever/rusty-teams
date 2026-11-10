# Rune spike: Teams shell

The spike renders the Teams chat layout in Rune (rev `7ed827a`) with static demo data. It covers the title bar, app rail, chat list, conversation and composer. See `screenshot.png` (headless, 1600x900).

## Status

- `cargo build` passes with no warnings. One headless test selects a chat, sends a message and checks that the message is appended.
- Window mode was launched in the follow-up below. Typing into the composer and pressing Enter were not tested.
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

## Follow-up: long lists and the real window (risks 2 and 3)

The spike now has 2,000 chats and a 10,000-message conversation (Marie). Both lists use `virtual_rows`. Messages vary in height: one line, three lines with `\n`, wrapped paragraphs, and 320x180 image blocks (every 10th). The chat opens with the newest 58 loaded. `on_visible_rows` prepends the next 50 when the first visible row is below index 10. `window.png` is a PrintWindow capture of the real window.

### Virtualization

- Test `older_pages_prepend_without_moving_the_reader_and_reach_10000` passes. A prepended page leaves every visible row within 0.5 pt of where it was. Wheel scrolling then pages in all 10,000 and reaches `m0`. Both lists start with fewer than 2,000 elements built.
- `justify_end()` on the scroll viewport breaks it: the wheel no longer scrolls the virtual list, and the paging test fails. It was removed, so a chat with only a few messages now sits at the top instead of the bottom. A port needs another way to pin short chats to the bottom.
- Keys are built with `format!` per row. `virtual_rows_version((selected, first id))` skips the per-row key pass when nothing changed.

### Measurements

Release build, 1600x900 at scale 1, Windows 11, RTX 2060, 12 logical cores. Other agents were compiling on the same machine, so the numbers spread.

Headless scripted scroll (`--script` with a generated JSON of 3,500 wheel events of 300 pt up over the messages, then 600 down the chat list, 4,220 frames). The run loads all 10,000 messages. CPU per frame covers update plus GPU encode, without present. Three runs:

| Metric | Range |
|---|---|
| Mean | 1.6 to 3.9 ms |
| p50 | 1.5 to 3.7 ms |
| p95 | 3.2 to 10.9 ms |
| Max | 26 to 52 ms (the first frame) |
| Frames with a commit, p95 | 7.8 to 17.4 ms |

Windowed (`rune-teams --bench`, sampled by `bench.ps1`). The poll hook pages up 90% of the viewport every frame until all 10,000 are loaded and `m0` shows, then pages the chat list down for 300 frames. Frames are not vsync-capped, so the interval is close to frame cost.

| Phase | Frames | Time | Interval p50 / p95 / p99 / max | Over 25 ms |
|---|---|---|---|---|
| Conversation, 199 pages prepended | 1,164 | 4.9 s | 3.9 / 8.5 / 11.2 / 31.3 ms | 1 |
| Chat list, 2,000 rows | 301 | 0.9 s | 4.1 / 5.2 / 5.6 / 7.2 ms | 0 |

During the run CPU was 7.5 to 8.3% of 12 cores (about one core, uncapped). Working set was 113 to 118 MB and private memory 163 to 169 MB, sampled once a second. Memory did not climb while the 10,000 messages loaded.

### Real window (`window-check.ps1` driving the mouse and keyboard with `SetCursorPos`, `mouse_event` and `keybd_event`)

| Check | Result |
|---|---|
| Drag the title bar by +200,+100 | Window moved exactly 200,100 |
| Double-click title bar | Maximized; second double-click restored |
| Maximize caption button | Maximized; click again restored |
| Win+Left | Snapped to the left half (974x1039) |
| Drag the title bar to the top screen edge | Aero Snap maximized |
| Minimize caption button | Iconic |
| Close caption button | Process exited |

Rune's `window_drag` calls winit `drag_window`, so the system move loop provides Snap. Rune draws the caption buttons and answers them as non-client hits.

### Not verified

- DPI change: both monitors run at 96 DPI. Moving the window to the second monitor kept 96 and the process stayed up. A scale change was not exercised.
- Snap layouts flyout on hovering maximize (Windows 11), the Alt+Space window menu and right-click on the title bar. These need a human eye.
- Caption button hover and pressed visuals, and whether the maximized window's top 8 px clip anything. Only window state was checked, not pixels.
- Touch and pen drag, and a high-refresh or vsync-capped display.
- Real message content: markdown, mentions, actual images and reactions per row. The image block is a flat rectangle.
- Frame pacing as perceived by a person. The figures above are frame intervals and CPU, not visual smoothness.

### Build note

During this run, executables freshly written under `C:\Users\theol\.ct` (the global `CARGO_TARGET_DIR`) failed to start with "Access denied", build scripts included. The same file copied elsewhere ran. Building with `CARGO_TARGET_DIR` under `%TEMP%` worked. The cause was not found.
