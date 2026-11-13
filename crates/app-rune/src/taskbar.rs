//! The main window's taskbar button: the unread badge and the flash on new messages, drawn as
//! the GPUI client does (`app_base::notify::platform`). Windows only; elsewhere these do nothing.

use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

use app_backend::notify::badge::{badge_for, render_badge};

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

#[cfg(windows)]
fn hwnd() -> Option<windows::Win32::Foundation::HWND> {
    let handle = MAIN_WINDOW.load(Ordering::Relaxed);
    (handle != 0).then_some(windows::Win32::Foundation::HWND(handle as *mut _))
}

#[cfg(windows)]
fn flash_with(flags: windows::Win32::UI::WindowsAndMessaging::FLASHWINFO_FLAGS) {
    use windows::Win32::UI::WindowsAndMessaging::{FLASHWINFO, FlashWindowEx};
    let Some(hwnd) = hwnd() else { return };
    let info = FLASHWINFO { cbSize: size_of::<FLASHWINFO>() as u32, hwnd, dwFlags: flags, uCount: 0, dwTimeout: 0 };
    // SAFETY: `info` is a valid FLASHWINFO; a stale window handle only makes the call fail.
    unsafe { let _ = FlashWindowEx(&info); }
}

/// Flashes the taskbar button until the window comes to the front.
pub fn flash() {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{FLASHW_TIMERNOFG, FLASHW_TRAY};
        flash_with(FLASHW_TRAY | FLASHW_TIMERNOFG);
    }
}

/// Shows `count` on the taskbar button (none at zero); clearing it also stops a flash.
pub fn set_badge(count: usize) {
    let previous = SHOWN.swap(count, Ordering::Relaxed);
    if previous == count { return; }
    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject, HGDIOBJ};
        use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
        use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
        use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, FLASHW_STOP, GetSystemMetrics, HICON, ICONINFO, SM_CXSMICON};
        use windows::core::{BOOL, w};
        let Some(hwnd) = hwnd() else { SHOWN.store(usize::MAX, Ordering::Relaxed); return };
        if count == 0 && previous != usize::MAX { flash_with(FLASHW_STOP); }
        // SAFETY: COM is initialized on this (UI) thread before use; the bitmaps and icon are
        // created and released here, and the taskbar copies the icon it is given.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            // On a COM failure, forget the count so the next frame retries.
            let Ok(taskbar) = CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER) else { SHOWN.store(usize::MAX, Ordering::Relaxed); return };
            if taskbar.HrInit().is_err() { SHOWN.store(usize::MAX, Ordering::Relaxed); return; }
            let Some(badge) = badge_for(count) else {
                let _ = taskbar.SetOverlayIcon(hwnd, HICON::default(), w!(""));
                return;
            };
            let size = GetSystemMetrics(SM_CXSMICON).max(16) as u32;
            let pixels = render_badge(&badge, size);
            let color = CreateBitmap(size as i32, size as i32, 1, 32, Some(pixels.as_ptr().cast()));
            let mask = CreateBitmap(size as i32, size as i32, 1, 1, None);
            let icon = CreateIconIndirect(&ICONINFO { fIcon: BOOL(1), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color });
            let _ = DeleteObject(HGDIOBJ(color.0));
            let _ = DeleteObject(HGDIOBJ(mask.0));
            if let Ok(icon) = icon {
                let _ = taskbar.SetOverlayIcon(hwnd, icon, w!("Unread chats"));
                let _ = DestroyIcon(icon);
            }
        }
    }
}
