//! The main window's taskbar button: the unread badge and the flash on new messages, drawn by
//! the GPUI client's code (`app_base::notify::platform`). Windows only; elsewhere these do nothing.

use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

use app_base::notify::platform;
use app_backend::notify::badge::badge_for;

static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);
/// The count drawn last, so an unchanged count costs nothing; `usize::MAX` before the first.
static SHOWN: AtomicUsize = AtomicUsize::new(usize::MAX);

/// `App::on_window` hook of the main window.
pub fn remember(window: &rune::WindowHandle) {
    let _ = window.with_raw_handle(|raw| {
        if let rune::raw_window_handle::RawWindowHandle::Win32(raw) = raw.as_raw() {
            MAIN_WINDOW.store(raw.hwnd.get(), Ordering::Relaxed);
        }
    });
}

fn handle() -> Option<platform::NativeHandle> {
    Some(MAIN_WINDOW.load(Ordering::Relaxed)).filter(|&handle| handle != 0)
}

/// Flashes the taskbar button until the window comes to the front.
pub fn flash() {
    if let Some(handle) = handle() { platform::flash(handle); }
}

/// Shows `count` on the taskbar button (none at zero); clearing it also stops a flash.
pub fn set_badge(count: usize) {
    let Some(handle) = handle() else { return };
    let previous = SHOWN.swap(count, Ordering::Relaxed);
    if previous == count { return; }
    if count == 0 && previous != usize::MAX { platform::stop_flash(handle); }
    #[cfg(windows)]
    {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: called on the UI thread; a repeated initialization only returns S_FALSE.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    }
    platform::set_badge(handle, badge_for(count).as_ref());
}
