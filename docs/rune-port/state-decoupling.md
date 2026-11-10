# Decoupling app-state, app-base and app-call-model from GPUI

Goal: the GPUI app (`crates/app`) and a Rune app drive the same `AppState`. Rune (rev `7ed827a`) has no entity system. A Rune app owns one plain struct, handlers get `&mut S`, `poll(s, now, tx) -> bool` runs once per frame, and `AppSpec::setup` hands out a `Waker` for other threads (see `spikes/rune-call/src/main.rs`).

Counts below come from a regex scan of non-test code (`#[cfg(test)]` modules counted apart), on `feat/rune-gui` at `b56d030`.

## Inventory

### app-state (111 files, 38k lines, 61 files mention GPUI)

| GPUI API | Uses | Files | Class |
|---|---|---|---|
| `cx: &mut Context<Self>` / `Context<AppState>` in signatures | 422 | 50 | seam: becomes `&mut dyn Host` |
| `cx.notify()` | 172 | 39 | state notification |
| `cx.emit(AppEvent::..)` | 111 | 33 | state notification |
| `cx.spawn(async move \|this, cx\| ..)` + `.detach()` | 68 | 34 | async spawning |
| `this.update(cx, ..)` inside those spawns | ~90 | 34 | async spawning |
| `cx.background_executor().timer(..)` | 17 | 13 | async spawning |
| `cx.background_spawn` | 2 | `agent_api/mod.rs`, `team_admin.rs` | async spawning |
| `cx.open_url` | 9 | 7 | platform effect |
| `cx.write_to_clipboard` | 4 | `channel_menu.rs`, `message_actions.rs`, `profile_state.rs` | platform effect |
| `cx.prompt_for_paths` | 1 | `call/actions.rs` (`add_call_background`) | UI that leaked into state |
| `gpui_kit::component::{input, select, date_picker, time_field, searchable_list}`, `Entity<InputState>` etc., `Subscription`, `on_blur`, `Window` | 45 type uses | `card_inputs.rs`, `card_state.rs` | widget state in state |
| `AnyView` fields | 5 | `app_state.rs` (`transfer_picker`, `transcript_panel`), `call/features.rs`, `call/people_actions.rs` | view in state |
| `ActivityCenter` (entity with `subscribe`, `observe`, `AnyWindowHandle`) | 1 type | `activity/service.rs` | derived model, view-side |
| `Global` + `Entity<_>` handles: `AppHandle`, `AgentApiHandle`, `AgentRunsHandle` | 3 globals, 6 `try_global`/`global` | `agents/mod.rs`, `agent_api/mod.rs`, `task_dialog/hosted.rs`, `card_inputs.rs` | app wiring |
| `AgentRuns` and `AgentApi` entities, `&mut App` free functions | 2 models, 47 `&App`/`&mut App` | `agents/*`, `agent_api/*` | second and third model |
| `Option<Entity<CallMedia>>` | 1 field, 3 calls | `app_state.rs`, `call/actions.rs` | state notification (redraw of video only) |
| `gpui_kit::Image` / `ImageFormat` as a bytes carrier | 37 | `data.rs`, `rows.rs`, `outgoing.rs`, `stored_outgoing.rs`, `agent_api/*`, `demo.rs` | data type borrowed from GPUI |
| `app_ui_kit::render::blocks`, `widgets::Unread` | 6 | `rows.rs`, `pending_rows.rs`, `agent_api/body.rs`, `sidebar_model.rs` | pure code living in a GPUI crate |
| `app_ui_kit::{settings_controls, spinner}` re-export | 1 | `agents/mod.rs` | GPUI widgets re-exported from state |
| `pub use app_base::{theme, zoom, frame_log, user_activity, ..}` | 1 | `lib.rs` | re-export for `crates/app` |
| `#[gpui_kit::test]`, `TestAppContext`, `cx.new`, `run_until_parked` | 220 markers, 200 `cx.update` | 8 files (`call/actions.rs` 85, `agents/runs.rs` 80, `call/consult.rs`, `call/people_actions.rs`, `app_state.rs`, `card_actions.rs`, `call/join.rs`) | test harness only |

Every `cx.spawn` has the same shape: await a tokio `oneshot` (from `runtime::spawn`) or a timer, then `this.update(cx, |state, cx| ..)` once or in a loop. Some read a value back (`app_state.rs:1065` returns the next typing expiry). None needs `Window`.

### app-call-model (35 files, 9k lines, 4 files mention GPUI)

| File | GPUI API | Class |
|---|---|---|
| `model.rs` | `ActiveCall::people_scroll: ScrollHandle` | widget state in state. **Done in step 0.** |
| `media.rs` | `Entity<CallMedia>`, `cx.notify()` | redraw signal for video. **Moved to app-state in step 0.** |
| `pictures.rs` | `Arc<RenderImage>`, `cx.drop_image` (3) | GPUI texture cache inside `ActiveCall` |
| `preview.rs` | `CallPictures`, `RenderImage`, `&mut App` (2) | same cache for the camera preview |

### app-base (58 files, 12k lines, 15 files mention GPUI)

| File | GPUI API | Class |
|---|---|---|
| `zoom.rs` (927) | wraps `Platform`/`PlatformWindow`, `Pixels`, `Bounds`, `Task` | GPUI host glue |
| `theme.rs` (432) | `gpui_kit::component::Theme`, `Hsla` (52), `px`, `set_global`, `text_system` | GPUI host glue |
| `frame_log.rs` | `canvas`, `IntoElement` | GPUI host glue |
| `user_activity.rs` | `canvas` + input events, `UserInput` itself is plain | mixed: element half is glue |
| `assets.rs` | `AssetSource`, `SharedString` | GPUI host glue |
| `remote_image.rs` | `ImageSource`, `SharedString` | GPUI host glue |
| `avatar_image.rs`, `emoticons/strip.rs` | `RenderImage`, `Asset`, `http_client` | GPUI host glue |
| `image_clipboard.rs` | `ClipboardItem`, `Image` | GPUI host glue |
| `inline_image.rs` | `ImageFormat` only | data type borrowed from GPUI |
| `notify/platform/{windows_impl,x11_input,stub}.rs` | `&Window`, used only through `HasWindowHandle` | trivially generic |
| `notify/sound_cache.rs`, `release_feed.rs` | `cx.http_client()`, `background_executor().spawn`, `Task` | async spawning |

`app-ui-kit` is GPUI widgets and stays GPUI. Only `render/blocks.rs`, `render/syntax.rs` and `widgets::Unread` are toolkit-free and needed by state.

## The seam

One object-safe trait in app-state, used where `&mut Context<Self>` is used today, plus one handle type for spawned futures:

```rust
// crates/app-state/src/host.rs
pub type Job = Box<dyn FnOnce(&mut AppState, &mut dyn Host)>;

pub trait Host {
    fn notify(&mut self);
    fn emit(&mut self, event: AppEvent);
    fn spawn(&mut self, task: LocalBoxFuture<'static, ()>);
    fn handle(&mut self) -> StateHandle;
    fn open_url(&mut self, url: &str);
    fn write_clipboard(&mut self, text: String);
}

/// Posts jobs to the UI thread. Cheap to clone, not Send.
#[derive(Clone)]
pub struct StateHandle(Rc<dyn Fn(Job) -> bool>);

impl StateHandle {
    /// Runs `f` on the UI thread. None once the state is gone.
    pub async fn update<R: 'static>(
        &self,
        f: impl FnOnce(&mut AppState, &mut dyn Host) -> R + 'static,
    ) -> Option<R>;
}
```

Why this shape:

- `&mut Context<'_, AppState>` coerces to `&mut dyn Host` at every call site. The `state.update(cx, |state, cx| state.x(cx))` calls in `crates/app` (about 1300 matches over 108 files) do not change.
- `dyn` instead of generics: 422 signatures stay non-generic, and the closure inside `update` can take `&mut dyn Host` without a lifetime tied to GPUI's `Context<'a>`.
- `LocalBoxFuture`, not `Send`: both GPUI's foreground executor and a Rune-side executor are single-threaded, so spawn bodies need not be `Send`.
- `update` is async in both hosts. GPUI resolves it at once (`WeakEntity::update` on a cloned `AsyncApp`). Rune queues the job and runs it in the next `poll`.
- Timers: `app_base::runtime::sleep(d)` = `runtime::spawn(tokio::time::sleep(d))`. That keeps the existing "receiver is runtime-agnostic" pattern. No test uses `advance_clock`, so fake time is not lost.

Rewrite of a spawn site:

```rust
// before
cx.spawn(async move |this, cx| {
    let answer = receiver.await;
    this.update(cx, |state, cx| state.finish(answer, cx)).ok();
}).detach();
// after
let this = cx.handle();
cx.spawn(Box::pin(async move {
    let answer = receiver.await;
    this.update(move |state, cx| state.finish(answer, cx)).await;
}));
```

Hosts:

- GPUI: `impl Host for gpui::Context<'_, AppState>` in app-state behind a default-on `gpui` feature (orphan rule: the trait is local). About 40 lines.
- Rune: a `RuneHost { dirty, events: Vec<AppEvent>, jobs: Rc<RefCell<VecDeque<Job>>>, tasks: FuturesUnordered<LocalBoxFuture> }`. Its std `Waker` calls the Rune `Waker`. `poll` drains jobs, polls tasks, hands `events` to derived models (activity feed, notification center) and returns `dirty`. About 80 lines in the Rune app.

Not in the seam on purpose:

- No `observe`/`subscribe` abstraction. `emit` already carries everything subscribers need. In Rune, `poll` passes the drained events to whoever cares.
- No window, focus or element API. The one use (`card_inputs.rs`) moves to the view.
- `AgentRuns` (24 signatures) and `AgentApi` (11) stay GPUI until a Rune app needs agents. Then `Host` and `StateHandle` get a type parameter. That is one more sed, not a redesign.

## Plan, file by file

| Step | Files | Change | Effort |
|---|---|---|---|
| 0 (done) | `app-call-model/src/model.rs`, `app/src/views/{shell,call_view,call_people}.rs` | `people_scroll` moves from `ActiveCall` to `Shell`, keyed by call id. | done |
| 0 (done) | `app-call-model/src/media.rs` -> `app-state/src/call/media.rs` | `CallMedia` leaves app-call-model. | done |
| 1 | new `app-state/src/host.rs`, all 50 files with `Context<Self>` | Add `Host`, `StateHandle`, GPUI impl. Replace `cx: &mut Context<Self>` with `cx: &mut dyn Host` (sed). Fix what no longer compiles: the spawn sites, timers, effects. Do it in one commit, since a `dyn Host` method cannot call a method that still wants `Context<Self>`. | 2.5 d |
| 1a | 34 files with `cx.spawn` | Rewrite as above. 17 timers -> `runtime::sleep`. `team_admin.rs` and `agent_api/mod.rs` `background_spawn` -> `runtime::spawn`. | (in 1) |
| 1b | `call/actions.rs`, `channel_menu.rs`, `message_actions.rs`, `profile_state.rs`, `share_outlook.rs`, `task_dialog*.rs`, `call/features.rs`, `card_inputs.rs` | `cx.open_url` / `cx.write_to_clipboard` -> `Host` methods. | (in 1) |
| 1c | `app_state.rs`, `call/actions.rs` | Drop `call_media: Option<Entity<CallMedia>>`. Emit `AppEvent::CallMedia` instead. The GPUI shell subscribes and notifies its video entity. | (in 1) |
| 2 | `call/actions.rs` | `add_call_background` splits: the view runs `prompt_for_paths`, state gets `add_call_background_path(path)`. | 0.25 d |
| 3 | `card_inputs.rs`, `card_state.rs` -> `crates/app/src/views/card_inputs.rs` | Widget entities (`InputState`, `SelectState<ChoiceList>`, date, time), their subscriptions and `show_choice_results(.., window, ..)` move to the GPUI view. `CardState` keeps the string values it already seeds (`seed_input`) and a `set_input(key, value)`. Remote choice search stays in state (`card_choice_search`). | 1.5 d |
| 4 | `app_state.rs`, `call/features.rs`, `call/people_actions.rs` | `transfer_picker`, `transcript_panel` and the pickers passed in as `AnyView` move to `Shell`. State keeps an `open: bool` per panel. | 0.5 d |
| 5 | `activity/service.rs` | `ActivityCenter` becomes a plain struct with `on_event(&mut self, &AppEvent, &AppState)`. The GPUI wrapper entity, its subscriptions and `AnyWindowHandle` move to `crates/app`. | 0.5 d |
| 6 | `app-base/src/inline_image.rs`, `data.rs`, `rows.rs`, `outgoing.rs`, `stored_outgoing.rs`, `agent_api/*`, `demo.rs` | Own `ImageData { format, bytes: Arc<[u8]>, id: u64 }` and `ImageFormat` enum in app-base. GPUI views map to `Arc<gpui::Image>` through a small cache keyed by `id`. Without the cache GPUI re-hashes the bytes on every frame. | 1 d |
| 7 | `app-call-model/src/{pictures,preview}.rs`, `model.rs` | `CallPictures` stores the last `VideoPicture` (BGRA) per key, no `RenderImage`. The GPUI call view keeps the `RenderImage` cache and `drop_image`. Rune pushes the BGRA bytes to `NativeSurface::push_bytes` without conversion (see `spikes/rune-call/FINDINGS.md`). After this, app-call-model drops `gpui-kit`. | 1 d |
| 8 | `app-ui-kit/src/render/{blocks,syntax}.rs`, `widgets.rs` | Move `blocks.rs`, `syntax.rs` and `Unread` to app-base, and app-ui-kit re-exports them. Move the `settings_controls`/`spinner` re-export out of `agents/mod.rs`. app-state drops `app-ui-kit`. | 0.5 d |
| 9 | `app-base`: `zoom`, `theme`, `frame_log`, `assets`, `remote_image`, `avatar_image`, `emoticons/strip`, `image_clipboard`, the element half of `user_activity` | Move to app-ui-kit (they are GPUI host glue and only `crates/app` and app-ui-kit use them), or gate behind an app-base `gpui` feature if the move churns too many imports. Drop the `pub use app_base::{..}` re-exports from app-state `lib.rs`. | 1 d |
| 10 | `app-base/src/notify/platform/*.rs` | `&Window` -> `&impl HasWindowHandle`. Linux needs a check on a Linux box. | 0.25 d |
| 11 | `app-base/src/notify/sound_cache.rs`, `release_feed.rs`, `emoticons/strip.rs` | Build the existing `reqwest_client` directly instead of `cx.http_client()`, run on `runtime::spawn`, return a `oneshot::Receiver` instead of `gpui::Task`. | 0.5 d |
| 12 | `agents/*`, `agent_api/*`, `task_dialog/hosted.rs` | Only when the Rune app needs agents: give `Host`/`StateHandle` a type parameter, make `AgentRuns`/`AgentApi` use it, replace the three `Global`s with fields or explicit handles. | 2 d |
| 13 | tests in 8 files | Tests keep running on `TestAppContext` through the GPUI impl. Port them to a `TestHost` (records events, runs tasks on a `LocalPool`) file by file, then drop the `gpui-kit` dev-dependency at the end. | 1.5 d |
| 14 | `app-state/Cargo.toml`, `app-base/Cargo.toml` | Make `gpui-kit` optional behind `gpui` (default on). Add a CI check: `cargo check -p app-state --no-default-features`. | 0.25 d |

Total: about 13 days for steps 1-11, 13 and 14. Step 12 adds 2 days. Step 1 is the only large commit. The others are small and independent, and each one can be checked with `cargo check -p app` plus the crate's tests.

## Risks

- Re-entrancy. A GPUI `StateHandle::update` while the entity is already leased panics. That cannot happen from a spawned future, because GPUI polls futures outside updates. Never call `update` synchronously from state code.
- Ordering. GPUI runs a job inside the future's poll. Rune runs it on the next frame. Code that relied on `this.update` taking effect before the next `.await` still works, since `update` is awaited.
- `notify` vs `emit`. In Rune both mean "redraw". Keep both: GPUI views subscribe to specific events (`conversation.rs` 35 matches, `notify/center.rs` 12, `shell.rs` 9, `sidebar.rs` 8).
- Card input parity. Moving `card_inputs.rs` touches Adaptive Card inputs. `crates/app/src/views/card_input_tests.rs` must stay green.
