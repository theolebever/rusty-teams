//! Chat windows share the main backend and receive its updates.
use crate::{bridge::Bridge, types::Update, ui::UiState};
use rune::*;

pub fn open(s: &mut UiState<Bridge>, id: String) {
    crate::view_composer::persist_draft(s);
    if s.selected.as_deref() == Some(&id)
        && (s.thread_root_id.is_some()
            || crate::ui::active_target(s).is_some_and(|target| s.media.is_uploading(&target))
            || s.compose.draft_save_failed())
    {
        s.toast.show("Finish the upload, save the draft and return to the main conversation before detaching it");
        return;
    }
    if let Some(window) = s.chat_windows.get(&id) {
        let restore = !window.is_visible();
        window.open();
        window.update(move |state, _| {
            if restore
                && !crate::ui::active_target(state)
                    .is_some_and(|target| state.media.is_uploading(&target))
                && !state.compose.draft_save_failed()
            {
                crate::ui::forget_cached_draft(state);
                crate::ui::restore_draft(state);
            }
            if let Some(id) = state.selected.clone() {
                crate::types::Actions::open(&state.actions, &id);
            }
        });
        if s.selected.as_deref() == Some(&id) {
            crate::ui::save_draft(s);
            crate::ui::forget_cached_draft(s);
            s.selected = None;
            s.messages.clear();
            s.composer.clear();
        }
        return;
    }
    let mut state = UiState::new(s.actions.chat_window_actions(&id));
    state.detached_chat = true;
    state.chats = s.chats.clone();
    state.teams = s.teams.clone();
    state.details = s.details.clone();
    state.settings.prefs = s.settings.prefs.clone();
    state.appearance = s.appearance;
    crate::ui::open_chat(&mut state, &id, &mut Tx::default());
    let key = format!("chat-{id}");
    let app = App::new(AppSpec {
        poll: Some(poll),
        on_key: Some(crate::shortcuts::on_key::<Bridge>),
        ..AppSpec::new(
            "Rusty Teams - Chat",
            (850.0, 700.0),
            crate::init,
            crate::ui::view::<Bridge>,
        )
    })
    .init(move || state)
    .titlebar(Titlebar::new(crate::view_chrome::TITLE_BAR_HEIGHT))
    .on_window(crate::window_icon::install)
    .on_close(|state, _| {
        crate::view_composer::persist_draft(state);
        crate::view_composer::stop_typing(state);
        if !crate::ui::active_target(state).is_some_and(|target| state.media.is_uploading(&target))
            && !state.compose.draft_save_failed()
        {
            state.actions.close_chat_window();
        }
    });
    let handle = app.handle();
    open_window(WindowSpec::from_app(key, app));
    s.chat_windows.insert(id.clone(), handle);
    if s.selected.as_deref() == Some(&id) {
        crate::ui::save_draft(s);
        crate::ui::forget_cached_draft(s);
        s.selected = None;
        s.messages.clear();
        s.composer.clear();
    }
}

pub fn forward(s: &UiState<Bridge>, update: &Update) {
    if matches!(
        update,
        Update::OpenChat(_)
            | Update::OpenDirect(_)
            | Update::PopOutChat(_)
            | Update::NotificationReply(_)
            | Update::NotificationRead(_)
    ) {
        return;
    }
    for window in s.chat_windows.values() {
        let update = update.clone();
        window.update(move |state, _| state.apply(update));
    }
}

fn poll(s: &mut UiState<Bridge>, _: f64, tx: &mut Tx) -> bool {
    crate::view_composer::persist_draft(s);
    crate::chat_menu::drain(s, tx);
    if let Some(bytes) = s.media.clipboard_image.take()
        && let Some(image) = ClipboardImage::from_png(bytes)
    {
        tx.copy_rich(ClipboardContent::default().with_image(image));
    }
    if let Some(link) = s.chatlist.copy_link.take() {
        tx.copy(link);
    }
    crate::translation::poll(s)
}
