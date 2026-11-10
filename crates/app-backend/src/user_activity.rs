use std::time::{Duration, Instant};

pub const IDLE_AFTER: Duration = Duration::from_secs(5 * 60);

pub fn latest_input(
    app_input: Option<Instant>,
    system_idle: Option<Duration>,
    now: Instant,
) -> Option<Instant> {
    let system_input = system_idle.and_then(|idle| now.checked_sub(idle));
    app_input.max(system_input)
}

pub fn is_active(last_input: Option<Instant>, now: Instant, idle_after: Duration) -> bool {
    last_input.is_some_and(|last_input| now.saturating_duration_since(last_input) < idle_after)
}

pub fn activity_change(
    previous: bool,
    last_input: Option<Instant>,
    now: Instant,
    idle_after: Duration,
) -> Option<bool> {
    let current = is_active(last_input, now, idle_after);
    (current != previous).then_some(current)
}

#[cfg(windows)]
pub fn system_idle() -> Option<Duration> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe { GetLastInputInfo(&mut info) }.as_bool().then(|| {
        let idle_millis = unsafe { GetTickCount() }.wrapping_sub(info.dwTime);
        Duration::from_millis(u64::from(idle_millis))
    })
}

#[cfg(not(windows))]
pub fn system_idle() -> Option<Duration> {
    None
}

