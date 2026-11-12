#[cfg(windows)]
fn load_icon() -> windows::core::Result<windows::Win32::UI::WindowsAndMessaging::HICON> {
    use windows::Win32::Foundation::HINSTANCE;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::LoadIconW;
    // Resource 1 is shared by every window and stays loaded for the process lifetime.
    unsafe {
        LoadIconW(
            Some(HINSTANCE(GetModuleHandleW(None)?.0)),
            windows::core::PCWSTR(1 as *const u16),
        )
    }
}

pub fn install(window: &rune::WindowHandle) {
    #[cfg(windows)]
    {
        use rune::raw_window_handle::RawWindowHandle;
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            ICON_BIG, ICON_SMALL, SendMessageW, WM_SETICON,
        };
        let icon = match load_icon() {
            Ok(icon) => icon,
            Err(error) => {
                app_backend::diagnostics::record(format!("Window icon unavailable: {error}"));
                return;
            }
        };
        let result = window.with_raw_handle(|raw| {
            if let RawWindowHandle::Win32(raw) = raw.as_raw() {
                // Rune calls this hook on the window's event thread while its handle is live.
                unsafe {
                    for size in [ICON_SMALL, ICON_BIG] {
                        SendMessageW(
                            HWND(raw.hwnd.get() as *mut _),
                            WM_SETICON,
                            Some(WPARAM(size as usize)),
                            Some(LPARAM(icon.0 as isize)),
                        );
                    }
                }
            }
        });
        if let Err(error) = result {
            app_backend::diagnostics::record(format!("Window icon unavailable: {error}"));
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn the_embedded_window_icon_loads() {
        assert!(
            !super::load_icon()
                .expect("embedded icon resource")
                .is_invalid()
        );
    }
}
