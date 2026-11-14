//! Inline video uses authenticated, streamed downloads and Rune's native player.

use std::sync::Arc;
use rune::*;
use crate::types::{Actions, Update};
use crate::ui::{UiState, text, ts};

#[derive(Debug, Clone)]
pub struct Source(Arc<tempfile::NamedTempFile>);
impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool { self.0.path() == other.0.path() }
}

pub fn load(shared: Arc<crate::bridge::Shared>, url: String) {
    let Some(engine) = shared.engine() else { shared.publish(Update::Video { url, result: Err("Not connected yet".into()) }); return; };
    if !url.starts_with("https://") {
        shared.publish(Update::Video { url, result: Err("Invalid video URL".into()) }); return;
    }
    app_backend::runtime::handle().spawn(async move {
        let result = async {
            use std::io::Write;
            let mut file = tempfile::Builder::new().prefix("rusty-teams-video-").suffix(".mp4").tempfile().map_err(|error| error.to_string())?;
            engine.download_file(&url, |bytes| file.write_all(bytes), |_| {}).await.map_err(|error| error.to_string())?;
            file.flush().map_err(|error| error.to_string())?;
            Ok(Source(Arc::new(file)))
        }.await;
        shared.publish(Update::Video { url, result });
    });
}

struct Playback { url: String, player: Player, _source: Source }
#[derive(Default)]
pub struct State { pending: Option<String>, playback: Option<Playback>, error: Option<(String, String)> }

impl State {
    pub fn pause(&mut self) {
        self.pending = None;
        if let Some(playback) = &self.playback { playback.player.pause(); }
    }
    pub fn loaded(&mut self, url: String, result: Result<Source, String>) {
        if self.pending.as_deref() != Some(&url) { return; }
        self.pending = None;
        match result {
            Ok(source) => {
                let player = Player::file(source.0.path());
                if let Some(wake) = crate::WAKER.get() { player.set_waker(wake.clone()); }
                player.play();
                self.playback = Some(Playback { url, player, _source: source });
                self.error = None;
            }
            Err(error) => self.error = Some((url, error)),
        }
    }
}

pub fn view<A: Actions + 'static>(s: &UiState<A>, url: &str, name: &str, key: &str) -> El {
    let state = &s.media.video;
    let playback = state.playback.as_ref().filter(|playback| playback.url == url);
    let playing = playback.is_some_and(|playback| playback.player.is_playing());
    let loading = state.pending.as_deref() == Some(url);
    let selected = url.to_owned();
    let mut body = col().key(key.to_owned()).w_full().max_w(560.0).gap(6.0)
        .child(text(name.to_owned()).font(ts(13.0, 600.0)))
        .child(crate::screens::button(if loading { "Loading…" } else if playing { "Pause" } else { "Play video" }, &format!("{key}-play"), move |s: &mut UiState<A>, _| {
            if s.media.video.pending.as_deref() == Some(&selected) { return; }
            if let Some(playback) = s.media.video.playback.as_ref().filter(|playback| playback.url == selected) {
                if playback.player.is_playing() { playback.player.pause(); } else { playback.player.play(); }
            } else {
                s.media.video.pause();
                s.media.video.playback = None;
                s.media.video.pending = Some(selected.clone());
                s.media.video.error = None;
                s.actions.load_video(&selected);
            }
        }));
    if let Some(playback) = playback {
        let (width, height) = playback.player.natural_size().unwrap_or((16, 9));
        let player = playback.player.clone();
        body = body.child(el().key(format!("{key}-surface")).w_full().h(320.0 * height as f32 / width.max(1) as f32)
            .video(&player, SurfaceFit::Fit).on_visible(move |_: &mut UiState<A>, visible, _| { if !visible { player.pause(); } }));
        let player = playback.player.clone();
        let replay = player.clone();
        let rewind = player.clone();
        let forward = player.clone();
        body = body.child(row().gap(6.0)
            .child(crate::screens::button(if player.muted() { "Unmute" } else { "Mute" }, &format!("{key}-mute"), move |_: &mut UiState<A>, _| player.set_muted(!player.muted())))
            .child(crate::screens::button("−10s", &format!("{key}-back"), move |_: &mut UiState<A>, _| rewind.seek(rewind.time() - 10.0)))
            .child(crate::screens::button("+10s", &format!("{key}-forward"), move |_: &mut UiState<A>, _| forward.seek(forward.time() + 10.0)))
            .child(crate::screens::button("Replay", &format!("{key}-replay"), move |_: &mut UiState<A>, _| { replay.seek(0.0); replay.play(); })));
        if let PlayerState::Failed(error) = playback.player.state() { body = body.child(text(error).wrap()); }
    }
    if let Some((failed, error)) = &state.error && failed == url { body = body.child(text(error.clone()).key(format!("{key}-error")).wrap()); }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leaving_a_chat_prevents_late_downloads_from_starting_playback() {
        let mut state = State { pending: Some("video".into()), ..Default::default() };
        state.pause();
        state.loaded("video".into(), Err("late failure".into()));
        assert!(state.pending.is_none());
        assert!(state.playback.is_none());
        assert!(state.error.is_none());
    }
}
