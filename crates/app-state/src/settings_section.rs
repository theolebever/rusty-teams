use crate::Host;
use crate::app_state::{AppEvent, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    General,
    Notifications,
    Appearance,
    Chats,
    Calls,
    Devices,
    Agents,
    Mcp,
    About,
}

impl AppState {
    pub fn open_settings(&mut self, section: SettingsSection, cx: &mut dyn Host) {
        cx.emit(AppEvent::Settings(section));
    }
}
