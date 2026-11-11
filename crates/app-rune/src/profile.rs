//! The profile card: photo, name, job title, presence, status message, actions and contact
//! details of one person (`crates/app/src/views/profile_card.rs`). Any view opens it with
//! `profile::open(s, user_id, name)`; the card shows what is known at once and fills in when the
//! directory answers.

use std::collections::HashMap;

use app_backend::schemes::active_palette;
use chrono::Utc;
use rune::*;
use teams_core::PersonProfile;

use crate::avatar::{person, rgb};
use crate::screens::{self, Screen};
use crate::status_menu::presence_of;
use crate::types::Actions;
use crate::ui::{UiState, text, ts};

const CARD_WIDTH: f32 = 340.0;

enum Entry {
    Loading,
    Ready(Box<PersonProfile>),
    Failed(String),
}

#[derive(Default)]
pub struct State {
    open: Option<(String, String)>,
    entries: HashMap<String, Entry>,
    /// A chat created for a person, waiting for the window loop to select it (it needs a `Tx`).
    pub chat_request: Option<String>,
}

impl State {
    pub fn loaded(&mut self, user_id: String, result: Result<PersonProfile, String>) {
        // A failed refresh keeps a profile that was already shown.
        match result {
            Ok(profile) => { self.entries.insert(user_id, Entry::Ready(Box::new(profile))); }
            Err(_) if matches!(self.entries.get(&user_id), Some(Entry::Ready(_))) => {}
            Err(error) => { self.entries.insert(user_id, Entry::Failed(error)); }
        }
    }
}

/// Opens the card for `user_id`; `name` is shown until the profile arrives.
pub fn open<A: Actions>(s: &mut UiState<A>, user_id: &str, name: &str) {
    s.profile.open = Some((user_id.to_owned(), name.to_owned()));
    s.profile.entries.entry(user_id.to_owned()).or_insert(Entry::Loading);
    if matches!(s.profile.entries.get(user_id), Some(Entry::Failed(_))) {
        s.profile.entries.insert(user_id.to_owned(), Entry::Loading);
    }
    s.actions.load_profile(user_id);
}

/// Makes `target` open the profile card of `user_id` when pressed; unchanged without an id.
pub fn link<A: Actions + 'static>(target: El, key: String, user_id: Option<&str>, name: &str) -> El {
    let Some(user_id) = user_id else { return target };
    let (user_id, name) = (user_id.to_owned(), name.to_owned());
    row().key(key).on_press(move |s: &mut UiState<A>| open(s, &user_id, &name)).child(target)
}

fn availability_label(availability: teams_core::Availability) -> &'static str {
    use teams_core::Availability as A;
    match availability {
        A::Available => "Available",
        A::Away => "Away",
        A::Busy => "Busy",
        A::DoNotDisturb => "Do not disturb",
        A::Offline => "Offline",
        A::Unknown => "",
    }
}

/// The other person of a 1:1 chat: its details carry their id as the avatar and no group faces.
pub fn direct_user<'a, A: Actions>(s: &'a UiState<A>, chat_id: &str) -> Option<&'a str> {
    let details = s.details.get(chat_id)?;
    details.avatar_id.as_deref().filter(|_| details.group.is_empty())
}

/// An existing 1:1 chat with `user_id`, found through the chat details.
fn direct_chat<A: Actions>(s: &UiState<A>, user_id: &str) -> Option<String> {
    s.details.iter().find(|(_, details)| details.avatar_id.as_deref() == Some(user_id) && details.group.is_empty()).map(|(id, _)| id.clone())
}

fn button<A: Actions + 'static>(key: &str, glyph: Option<&str>, label: &str, filled: bool, press: impl Fn(&mut UiState<A>, &mut Tx) + 'static) -> El {
    let palette = active_palette();
    let (fill, ink) = if filled { (palette.accent, palette.on_accent) } else { (palette.popup_hover, palette.text_soft) };
    let mut button = row().key(key.to_owned()).h(32.0).px(12.0).gap(6.0).center().radius(6.0).fill(rgb(fill)).on_press_tx(press);
    if let Some(glyph) = glyph {
        button = button.child(crate::icons::sym(glyph, 14.0).color(rgb(ink)));
    }
    button.child(text(label.to_owned()).font(ts(13.0, 600.0)).color(rgb(ink)))
}

fn field(label: &str, value: &str) -> El {
    let palette = active_palette();
    col()
        .child(text(label.to_owned()).font(ts(11.0, 400.0)).color(rgb(palette.text_muted)))
        .child(text(value.to_owned()).font(ts(13.0, 400.0)).color(rgb(palette.foreground)).wrap())
}

/// The card with its scrim, for the root of the window.
pub fn layer<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let (user_id, name) = s.profile.open.as_ref()?;
    let palette = active_palette();
    let entry = s.profile.entries.get(user_id);
    let profile = match entry {
        Some(Entry::Ready(profile)) => Some(profile.as_ref()),
        _ => None,
    };
    let title = profile.and_then(|p| p.display_name.clone()).unwrap_or_else(|| name.clone());
    let availability = profile.and_then(|p| p.availability);
    let is_me = s.own.me.as_ref().is_some_and(|me| &me.user_id == user_id);
    let presence = availability.and_then(presence_of);

    let mut lines = col().grow(1.0).min_w(0.0).gap(2.0).child(text(title.clone()).key("profile-name").font(ts(18.0, 600.0)).color(rgb(palette.foreground)).wrap());
    if let Some(job) = profile.and_then(|p| p.job_title.clone()).filter(|job| !job.is_empty()) {
        lines = lines.child(text(job).key("profile-job").font(ts(13.0, 400.0)).color(rgb(palette.text_soft)).wrap());
    }
    let local = profile.and_then(|p| p.time_zone.as_deref()).and_then(|zone| teams_core::local_time(zone, Utc::now())).map(|time| time.format("%H:%M").to_string());
    let status_line = match (availability.map(availability_label).filter(|label| !label.is_empty()), local) {
        (Some(label), Some(time)) => format!("{label} - {time} local time"),
        (Some(label), None) => label.to_owned(),
        (None, Some(time)) => format!("{time} local time"),
        (None, None) => String::new(),
    };
    if !status_line.is_empty() {
        lines = lines.child(text(status_line).key("profile-presence").font(ts(12.0, 400.0)).color(rgb(palette.text_muted)));
    }

    let mut card = col()
        .key("profile-card")
        .w(CARD_WIDTH)
        .padding(20.0)
        .gap(14.0)
        .radius(12.0)
        .fill(rgb(palette.popup))
        .border(1.0, rgb(palette.border))
        .role("dialog")
        .label("Profile")
        .modal(true, |s: &mut UiState<A>| s.profile.open = None)
        .child(row().w_full().gap(14.0).center_y().child(person(s, &title, Some(user_id), 64.0, presence)).child(lines));

    let note = profile.and_then(|p| p.out_of_office.as_ref().map(|text| if text.trim().is_empty() { "Out of office".to_owned() } else { text.clone() }).or_else(|| p.status_message.clone()));
    if let Some(note) = note {
        card = card.child(row().key("profile-note").w_full().padding(10.0).radius(8.0).fill(rgb(palette.surface)).child(text(note).font(ts(13.0, 400.0)).color(rgb(palette.text_soft)).wrap()));
    }

    let email = profile.and_then(|p| p.email.clone()).filter(|email| !email.trim().is_empty());
    let mut actions = row().w_full().gap(8.0).wrap();
    if !is_me {
        let (chat_id, call_id, call_name) = (user_id.clone(), user_id.clone(), title.clone());
        actions = actions
            .child(button("profile-chat", Some("chat_bubble"), "Chat", true, move |s: &mut UiState<A>, tx| {
                s.profile.open = None;
                match direct_chat(s, &chat_id) {
                    Some(id) => {
                        screens::navigate(s, Screen::Chat);
                        crate::ui::open_chat(s, &id, tx);
                    }
                    None => s.actions.open_direct(&chat_id),
                }
            }))
            .child(button("profile-call", Some("call"), "Call", false, move |s: &mut UiState<A>, _| {
                s.profile.open = None;
                s.actions.call_person(&call_id, &call_name);
            }));
    }
    if let Some(address) = email.clone() {
        let copied = address.clone();
        actions = actions
            .child(button("profile-email", None, "Email", false, move |s: &mut UiState<A>, tx| {
                if let Err(error) = tx.open_url(&format!("mailto:{address}")) {
                    s.toast.show(error.to_string());
                }
                s.profile.open = None;
            }))
            .child(button("profile-copy-email", None, "Copy email", false, move |s: &mut UiState<A>, tx| {
                tx.copy(copied.clone());
                s.toast.show("Email copied");
                s.profile.open = None;
            }));
    }
    card = card.child(actions);

    match entry {
        Some(Entry::Loading) | None => {
            card = card.child(text("Loading...").key("profile-loading").font(ts(12.0, 400.0)).color(rgb(palette.text_muted)));
        }
        Some(Entry::Failed(error)) => {
            card = card.child(text(format!("Profile unavailable: {error}")).key("profile-failed").font(ts(12.0, 400.0)).color(rgb(palette.text_muted)).wrap());
        }
        Some(Entry::Ready(_)) => {}
    }
    if let Some(p) = profile {
        if let Some(email) = &email {
            card = card.child(field("Email", email));
        }
        for phone in p.work_phones.iter().chain(&p.mobile_phone).filter(|phone| !phone.trim().is_empty()) {
            card = card.child(field("Phone", phone));
        }
        if let Some(office) = p.office_location.as_deref().filter(|office| !office.trim().is_empty()) {
            card = card.child(field("Office", office));
        }
        if let Some(manager) = &p.manager {
            card = card.child(field("Manager", manager.display_name.as_deref().unwrap_or_default()));
        }
        match p.direct_reports.len() {
            0 => {}
            1 => card = card.child(field("Organization", "1 direct report")),
            count => card = card.child(field("Organization", &format!("{count} direct reports"))),
        }
    }
    Some(el().key("profile-scrim").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(Color::BLACK.alpha(0.2)).center().child(card))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_refresh_keeps_the_profile_already_loaded() {
        let mut state = State::default();
        let profile = PersonProfile {
            user_id: "u".into(), display_name: Some("Ada".into()), job_title: None, department: None, company: None,
            office_location: None, email: None, work_phones: vec![], mobile_phone: None, manager: None,
            direct_reports: vec![], time_zone: None, availability: None, status_message: None,
            out_of_office: None, work_location: None,
        };
        state.loaded("u".into(), Ok(profile));
        state.loaded("u".into(), Err("offline".into()));
        assert!(matches!(state.entries.get("u"), Some(Entry::Ready(_))));
        state.loaded("v".into(), Err("offline".into()));
        assert!(matches!(state.entries.get("v"), Some(Entry::Failed(_))));
    }
}
