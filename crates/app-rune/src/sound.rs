//! Teams sounds (notification, urgent, ring) from the Teams CDN, cached in the same folder as
//! the GPUI client (`app_backend::notify::sound_files`), played on a thread of their own.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use app_backend::notify::sound_files::{TEAMS_SOUND_BASE_URL, cache_directory, is_mp3, read_cached, write_cached};

/// Blocking: reads the cache, else downloads and caches. Runs on the sound's own thread.
fn load(file_name: &str) -> Option<Vec<u8>> {
    let directory = cache_directory()?;
    if let Some(bytes) = read_cached(&directory, file_name) {
        return Some(bytes);
    }
    let bytes = app_backend::runtime::handle().block_on(download(file_name))?;
    if !is_mp3(&bytes) { return None; }
    if let Err(error) = write_cached(&directory, file_name, &bytes) { app_backend::diagnostics::record(format!("Rune: sound {file_name} not cached: {error}")); }
    Some(bytes)
}

async fn download(file_name: &str) -> Option<Vec<u8>> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build().ok()?;
    let response = client.get(format!("{TEAMS_SOUND_BASE_URL}{file_name}")).send().await.ok()?;
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
