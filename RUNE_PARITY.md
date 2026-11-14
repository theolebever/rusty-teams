# Rune feature completion order

Priority follows message safety first, daily workflows next, then administration and desktop integration.
Unchecked items are unfinished or still need an implementation audit against GPUI.

See [RUNE_PARITY_TESTS.md](RUNE_PARITY_TESTS.md) for the functional verification,
confirmed differences from GPUI, automated coverage and native/live-test limits.

## Continuation audit

- The checklist lives at the repository root (`RUNE_PARITY.md`).
- Calls exposes ringtone choices and previews; incoming calls use the shared ringtone rules.
- Chat density changes message spacing. The right-click preference hides the hover toolbar;
  channel posts and replies must retain their context menus in this mode.
- Up arrow supports editing the last own message, focusing messages, or staying in compose;
  a nonempty draft must not be replaced.
- Settings search includes the up-arrow, density and right-click controls.
- Scheduled sending, slash commands, inline images, translation, meeting creation, adaptive
  card actions, channel files and updates already have implementations and tests. Their
  unchecked entries below still require comparison against GPUI before claiming full parity.
- The reported Clippy diagnostics have been corrected; strict Rune Clippy passes.
  The full workspace passes 3,885 tests (two intentionally ignored), and Rune passes 296.
  The report records implemented corrections and remaining native/live-test limits.

## 1. Sending and draft safety

- [x] Persist messages before delivery and show the outgoing queue.
- [x] Resume pending messages after startup, with shared retries and manual retry after failure.
- [x] Preserve formatting, mentions, GIFs, replies and files in the queue.
- [x] Restore the full composition if saving to the queue fails.
- [x] Keep composition options separate for each conversation and thread.
- [x] Important and Urgent delivery options.
- [x] Channel post subject and announcement headline.
- [ ] Scheduled sending and management of scheduled messages.
- [ ] Slash commands and their suggestion menu.

## 2. Composition

- [x] Emoji skin tones and account reaction preferences.
- [x] Suggestions while typing `:emoji`.
- [x] Formatting shortcuts, lists, headings, quotes, links, tables and code blocks.
- [x] Render table cells and highlight code with the shared tokenizer.
- [ ] Complete the remaining formatting controls against GPUI.
- [ ] Paste and drop images, with inline placement.

## 3. Message actions

- [x] Save messages, copy links and open an Outlook draft.
- [x] Pick reactions beyond the four shortcuts.
- [ ] Forward multiple messages.
- [ ] Follow and unfollow channel posts.
- [ ] Show the people behind reactions.
- [ ] Translate messages and switch back to the original.

## 4. Calendar and notifications

- [x] Live meeting reminder lookup.
- [ ] Create meetings and complete event menus and Join flows.
- [ ] Notify when a person becomes available.
- [ ] Notification Reply and Mark as read.
- [ ] Repeating urgent message notifications.

## 5. Adaptive cards

- [ ] Inputs and validation.
- [ ] Submit, Execute and ShowCard.
- [ ] Refresh and task modules.

## 6. Sidebar and administration

- [ ] Rename, move and delete sections.
- [ ] Complete team and channel administration.
- [ ] Details panel and multiple pinned messages.
- [ ] Remember the last conversation per tab.
- [ ] Create a chat with its first message.

## 7. Media

- [ ] Complete image viewer controls.
- [ ] Channel files from SharePoint.
- [ ] Inline video.

## 8. Settings

- [ ] Audit and complete Notifications, Chats, General, Calls and About options.
- [ ] Verify that every exposed preference affects app behavior.

## 9. Agents

- [ ] Finish the interrupted agent integration and audit it against GPUI.

## 10. Desktop shell

- [x] Pop-out chats (shared backend, independent draft restoration; see report for handoff limits).
- [ ] Updates from this fork's releases.
- [ ] Remaining launch arguments, full demo mode and splash screen.
- [ ] Complete the status menu against GPUI.

Copilot, meeting Recap and advanced device processing remain outside this scope.
