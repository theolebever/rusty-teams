//! Protocol actions remain usable after the process that posted the toast exits.

fn percent(value: &str) -> String {
    value.bytes().map(|byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') { char::from(byte).to_string() }
        else { format!("%{byte:02X}") }
    }).collect()
}

pub fn xml(id: &str, conversation: &str, title: &str, body: &str) -> String {
    let link = format!("{}{}?conversation_id={}", crate::notifications::ACTIVATION_PREFIX, percent(id), percent(conversation));
    let escape = teams_core::escape_html;
    format!(r#"<toast activationType="protocol" launch="{}"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual><actions><action content="Reply" activationType="protocol" arguments="{}"/><action content="Mark as read" activationType="protocol" arguments="{}"/></actions><audio silent="true"/></toast>"#,
        escape(&link), escape(title), escape(body), escape(&format!("{link}&action=reply")), escape(&format!("{link}&action=read")))
}

#[cfg(windows)]
pub fn post(id: &str, conversation: &str, title: &str, body: &str) -> Result<(), String> {
    use windows::{Data::Xml::Dom::XmlDocument, UI::Notifications::{ToastNotification, ToastNotificationManager}, core::HSTRING};
    let show = || -> windows::core::Result<()> {
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml(id, conversation, title, body)))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (conversation, id).hash(&mut hash);
        toast.SetTag(&HSTRING::from(format!("{:016x}", hash.finish())))?;
        toast.SetGroup(&HSTRING::from("rune"))?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from("rusty-teams-rune"))?.Show(&toast)
    };
    show().map_err(|error| error.to_string())
}

#[cfg(not(windows))]
pub fn post(id: &str, conversation: &str, title: &str, body: &str) -> Result<(), String> {
    if rune_notify::post(&rune_notify::Notification::new(id, title).body(body).info("conversation_id", conversation)) { Ok(()) }
    else { Err("Notifications unavailable".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toast_actions_escape_content_and_encode_the_destination() {
        let xml = xml("a?b", "19:c&d", "Ada <Admin>", "\"hello\"");
        assert!(xml.contains("Ada &lt;Admin&gt;"));
        assert!(xml.contains("19%3Ac%26d&amp;action=reply"));
        assert!(xml.contains("19%3Ac%26d&amp;action=read"));
        assert!(xml.contains("notification/a%3Fb?"));
    }
}
