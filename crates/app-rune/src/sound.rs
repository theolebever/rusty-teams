//! Teams sounds (notification, urgent, ring) from the Teams CDN, cached in the same folder as
//! the GPUI client (`app_base::notify::sound_cache`), played on a thread of their own.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn cache_directory() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", store::DATA_DIR_NAME).map(|directories| directories.cache_dir().join("sounds"))
}

fn is_mp3(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3") || matches!(bytes, [0xFF, second, ..] if second & 0xE0 == 0xE0)
}

/// Blocking: reads the cache, else downloads and caches. Runs on the sound's own thread.
fn load(file_name: &str) -> Option<Vec<u8>> {
    let directory = cache_directory()?;
    if let Ok(bytes) = std::fs::read(directory.join(file_name)) && is_mp3(&bytes) {
        return Some(bytes);
    }
    let bytes = app_backend::runtime::handle().block_on(download(file_name))?;
    if !is_mp3(&bytes) { return None; }
    let partial = directory.join(format!("{file_name}.partial"));
    let saved = std::fs::create_dir_all(&directory).and_then(|()| std::fs::write(&partial, &bytes)).and_then(|()| std::fs::rename(&partial, directory.join(file_name)));
    if let Err(error) = saved { app_backend::diagnostics::record(format!("Rune: sound {file_name} not cached: {error}")); }
    Some(bytes)
}

async fn download(file_name: &str) -> Option<Vec<u8>> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build().ok()?;
    let response = client.get(format!("{}{file_name}", chatsvc::teams_cdn::SOUND_BASE_URL)).send().await.ok()?;
    Some(response.error_for_status().ok()?.bytes().await.ok()?.to_vec())
}

/// Stops the sound it started when dropped (a ringtone loops until then).
pub struct Handle(Arc<AtomicBool>);

impl Drop for Handle {
    fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
}

pub fn play(file_name: &'static str) {
    spawn(file_name, false, Arc::default());
}

pub fn start(file_name: &'static str, looped: bool) -> Handle {
    let stop = Arc::new(AtomicBool::new(false));
    spawn(file_name, looped, stop.clone());
    Handle(stop)
}

fn spawn(file_name: &'static str, looped: bool, flag: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let Some(bytes) = load(file_name) else { return };
        let Ok(mut output) = rodio::DeviceSinkBuilder::open_default_sink() else { return };
        output.log_on_drop(false);
        let player = rodio::Player::connect_new(output.mixer());
        while !flag.load(Ordering::SeqCst) {
            let Ok(source) = rodio::Decoder::try_from(std::io::Cursor::new(bytes.clone())) else { return };
            player.append(source);
            while !player.empty() && !flag.load(Ordering::SeqCst) { std::thread::sleep(std::time::Duration::from_millis(50)); }
            if !looped { break; }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn mp3_files_start_with_a_tag_or_a_frame_sync() {
        assert!(super::is_mp3(b"ID3\x04"));
        assert!(super::is_mp3(&[0xFF, 0xFB]));
        assert!(!super::is_mp3(b"<Error/>"));
    }
}
