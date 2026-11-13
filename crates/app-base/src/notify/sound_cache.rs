use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub use app_backend::notify::sound_files::{
    TEAMS_SOUND_BASE_URL, cache_directory, is_mp3, read_cached, write_cached,
};

use super::platform;
use crate::runtime;

/// Runs on the network runtime: reqwest needs tokio.
async fn load(file_name: &'static str) -> Option<Vec<u8>> {
    let directory = cache_directory()?;
    if let Some(bytes) = read_cached(&directory, file_name) {
        return Some(bytes);
    }
    let url = format!("{TEAMS_SOUND_BASE_URL}{file_name}");
    let response = reqwest::get(&url).await.ok()?;
    let status = response.status();
    let body = response.bytes().await.ok()?;
    if !status.is_success() || !is_mp3(&body) {
        log::warn!("teams sound {file_name} not loaded: status {status}");
        return None;
    }
    if let Err(error) = write_cached(&directory, file_name, &body) {
        log::warn!("teams sound {file_name} not cached: {error}");
    }
    Some(body.to_vec())
}

pub fn prefetch(file_names: &[&'static str]) {
    for &file_name in file_names {
        drop(runtime::spawn(load(file_name)));
    }
}

pub fn play(file_name: &'static str) {
    play_or(file_name, || {});
}

pub fn play_or(file_name: &'static str, fallback: fn()) {
    drop(runtime::spawn(async move {
        match load(file_name).await {
            Some(bytes) => platform::play_mp3(bytes),
            None => fallback(),
        }
    }));
}

pub struct SoundHandle {
    stop: Arc<AtomicBool>,
}

impl Drop for SoundHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

pub fn start(file_name: &'static str, looped: bool) -> SoundHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    drop(runtime::spawn(async move {
        if let Some(bytes) = load(file_name).await
            && !flag.load(Ordering::SeqCst)
        {
            platform::play_mp3_until(bytes, looped, flag);
        }
    }));
    SoundHandle { stop }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mp3_files_start_with_a_tag_or_a_frame_sync() {
        assert!(is_mp3(b"ID3\x04\x00"));
        assert!(is_mp3(&[0xFF, 0xFB, 0x90]));
        assert!(!is_mp3(b"<?xml version"));
        assert!(!is_mp3(&[]));
    }

    #[test]
    fn a_cached_sound_round_trips_and_junk_is_ignored() {
        let directory = tempfile::tempdir().unwrap();
        let sounds = directory.path().join("sounds");
        write_cached(&sounds, "end.mp3", b"ID3 tone").unwrap();
        assert_eq!(
            read_cached(&sounds, "end.mp3").as_deref(),
            Some(&b"ID3 tone"[..])
        );
        assert!(!sounds.join("end.mp3.partial").exists());
        write_cached(&sounds, "junk.mp3", b"<Error/>").unwrap();
        assert_eq!(read_cached(&sounds, "junk.mp3"), None);
        assert_eq!(read_cached(&sounds, "missing.mp3"), None);
    }
}
