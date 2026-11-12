pub use app_backend::notify::badge;
#[cfg_attr(not(windows), allow(dead_code))]
pub mod platform;
pub mod sound_cache;

pub use chatsvc::sounds;
