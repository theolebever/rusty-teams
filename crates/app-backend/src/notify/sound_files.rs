//! The on-disk cache of Teams sounds, shared by both clients.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const TEAMS_SOUND_BASE_URL: &str = chatsvc::teams_cdn::SOUND_BASE_URL;
const SOUND_DIRECTORY: &str = "sounds";
const PARTIAL_SUFFIX: &str = ".partial";

pub fn cache_directory() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", store::DATA_DIR_NAME)
        .map(|directories| directories.cache_dir().join(SOUND_DIRECTORY))
}

pub fn is_mp3(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3") || matches!(bytes, [0xFF, second, ..] if second & 0xE0 == 0xE0)
}

pub fn read_cached(directory: &Path, file_name: &str) -> Option<Vec<u8>> {
    fs::read(directory.join(file_name))
        .ok()
        .filter(|bytes| is_mp3(bytes))
}

pub fn write_cached(directory: &Path, file_name: &str, bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let partial = directory.join(format!("{file_name}{PARTIAL_SUFFIX}"));
    fs::write(&partial, bytes)?;
    fs::rename(&partial, directory.join(file_name))
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
