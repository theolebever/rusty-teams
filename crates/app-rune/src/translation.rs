//! On-demand translation keeps the original message intact and rejects stale answers.

use crate::types::{Actions, MessageRow};
use crate::ui::{UiState, text, ts};
use rune::*;
use std::collections::HashMap;

pub struct Cached {
    pub source: String,
    pub language: String,
    pub spans: Vec<teams_core::Span>,
    pub shown: bool,
}
pub struct State {
    pub stamps: HashMap<String, String>,
    stamp_request: Option<(String, String)>,
    auto_attempted: std::collections::HashSet<(String, String, String, String)>,
    pub language: TextEdit,
    pub dialog: Option<String>,
    pub pending: HashMap<(String, String), (String, String)>,
    pub cached: HashMap<(String, String), Cached>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            stamps: HashMap::new(),
            stamp_request: None,
            auto_attempted: Default::default(),
            language: TextEdit::new("en"),
            dialog: None,
            pending: HashMap::new(),
            cached: HashMap::new(),
        }
    }
}

impl State {
    pub fn target_changed(&mut self, target: &str) {
        self.language = TextEdit::new(target);
        self.cached.clear();
        self.pending.clear();
        self.auto_attempted.clear();
        self.stamp_request = None;
    }
}

pub fn settings<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut pane = col()
        .w_full()
        .child(crate::settings::caption("TRANSLATION"));
    let Some(settings) = &s.settings.prefs.translation else {
        return pane.child(text("Translation settings not loaded yet"));
    };
    for (label, behavior) in [
        (
            "Ask me before translating",
            teams_core::TranslationBehavior::Ask,
        ),
        (
            "Auto-translate all messages",
            teams_core::TranslationBehavior::Auto,
        ),
        ("Never translate", teams_core::TranslationBehavior::Never),
    ] {
        let patch = teams_core::behavior_patch(settings, behavior);
        pane = pane.child(crate::settings::choice(
            format!("translation-behavior-{behavior:?}"),
            label,
            settings.behavior == behavior,
            move |s: &mut UiState<A>| {
                crate::settings::change(s, crate::settings::Change::Translation(patch.clone()))
            },
        ));
    }
    let target = teams_core::default_target(
        settings.target_locale.as_deref(),
        settings.display_locale.as_deref(),
        &s.settings.prefs.languages,
    );
    pane = pane.child(crate::settings::caption("TRANSLATE INTO"));
    for language in &s.settings.prefs.languages {
        let patch = teams_core::target_patch(&language.code);
        pane = pane.child(crate::settings::choice(
            format!("translation-target-{}", language.code),
            &language.name,
            language.code == target,
            move |s: &mut UiState<A>| {
                crate::settings::change(s, crate::settings::Change::Translation(patch.clone()))
            },
        ));
    }
    pane = pane.child(crate::settings::caption("NEVER TRANSLATE THESE LANGUAGES"));
    for language in &s.settings.prefs.languages {
        let on = settings.never_translate_codes().contains(&language.code);
        let locales = if on {
            settings.without_never_translate(&language.code)
        } else {
            settings.with_never_translate(&language.code)
        };
        let patch = teams_core::authoring_patch(&locales);
        pane = pane.child(crate::settings::toggle(
            &format!("translation-exclude-{}", language.code),
            &language.name,
            on,
            true,
            move |s: &mut UiState<A>| {
                crate::settings::change(s, crate::settings::Change::Translation(patch.clone()))
            },
        ));
    }
    pane
}

fn offered<A: Actions>(s: &UiState<A>, message: &MessageRow) -> Option<String> {
    let settings = s.settings.prefs.translation.as_ref()?;
    if message.mine || message.deleted || message.system {
        return None;
    }
    let target = teams_core::default_target(
        settings.target_locale.as_deref(),
        settings.display_locale.as_deref(),
        &s.settings.prefs.languages,
    );
    teams_core::offered_language(
        s.translation.stamps.get(&message.id)?,
        &target,
        &settings.never_translate_codes(),
    )
    .map(|_| target)
}

pub fn poll<A: Actions>(s: &mut UiState<A>) -> bool {
    let Some(settings) = &s.settings.prefs.translation else {
        return false;
    };
    if settings.behavior == teams_core::TranslationBehavior::Never {
        return false;
    }
    let Some(conversation) = s.selected.clone() else {
        return false;
    };
    let key = (
        conversation.clone(),
        format!(
            "{:?}",
            s.messages
                .iter()
                .map(|row| (&row.id, &row.text))
                .collect::<Vec<_>>()
        ),
    );
    if s.translation.stamp_request.as_ref() != Some(&key) {
        s.translation.stamp_request = Some(key);
        s.translation.stamps.clear();
        s.actions.load_language_stamps(&conversation);
    }
    if settings.behavior != teams_core::TranslationBehavior::Auto {
        return false;
    }
    let target = teams_core::default_target(
        settings.target_locale.as_deref(),
        settings.display_locale.as_deref(),
        &s.settings.prefs.languages,
    );
    let messages: Vec<_> = s
        .messages
        .iter()
        .filter(|row| offered(s, row).is_some() && cached(s, row).is_none())
        .map(|row| (row.id.clone(), row.text.clone()))
        .collect();
    let mut changed = false;
    for (id, source) in messages {
        if s.translation.auto_attempted.insert((
            conversation.clone(),
            id.clone(),
            source,
            target.clone(),
        )) {
            s.translation.language = TextEdit::new(&target);
            request(s, &id);
            changed = true;
        }
    }
    changed
}

pub fn request<A: Actions>(s: &mut UiState<A>, message: &str) {
    let Some(conversation) = s.selected.clone() else {
        return;
    };
    let Some(row) = s
        .messages
        .iter()
        .chain(s.search_results.iter().flatten())
        .find(|row| row.id == message)
    else {
        return;
    };
    let language = chatsvc::language_code(s.translation.language.text.trim());
    if language.is_empty()
        || language.len() > 16
        || !language
            .chars()
            .all(|ch| ch.is_ascii_alphabetic() || ch == '-')
    {
        s.toast
            .show("Enter a language code, for example en, fr or de");
        return;
    }
    let key = (conversation.clone(), message.to_owned());
    if s.translation.pending.contains_key(&key) {
        return;
    }
    let source = row.text.clone();
    s.translation
        .pending
        .insert(key, (source.clone(), language.clone()));
    s.translation.dialog = None;
    s.actions
        .translate_message(&conversation, message, source, language);
}

pub fn apply<A: Actions>(
    s: &mut UiState<A>,
    conversation: String,
    message: String,
    source: String,
    language: String,
    result: Result<String, String>,
) {
    let key = (conversation, message);
    if s.translation.pending.get(&key) != Some(&(source.clone(), language.clone())) {
        return;
    }
    s.translation.pending.remove(&key);
    match result {
        Ok(html) => {
            s.translation.cached.insert(
                key,
                Cached {
                    source,
                    language,
                    spans: teams_core::html_to_spans(&html),
                    shown: true,
                },
            );
        }
        Err(error) => s.toast.show(format!("Could not translate: {error}")),
    }
}

pub fn cached<'a, A: Actions>(s: &'a UiState<A>, message: &MessageRow) -> Option<&'a Cached> {
    let key = (s.selected.clone()?, message.id.clone());
    s.translation
        .cached
        .get(&key)
        .filter(|cached| cached.source == message.text)
}

pub fn line<A: Actions + 'static>(s: &UiState<A>, message: &MessageRow) -> Option<El> {
    let Some(translation) = cached(s, message) else {
        if s.settings.prefs.translation.as_ref()?.behavior != teams_core::TranslationBehavior::Ask {
            return None;
        }
        let language = offered(s, message)?;
        let id = message.id.clone();
        return Some(crate::screens::button(
            "Translate",
            &format!("translation-offer-{id}"),
            move |s: &mut UiState<A>, _| {
                s.translation.language = TextEdit::new(&language);
                request(s, &id);
            },
        ));
    };
    let key = (s.selected.clone()?, message.id.clone());
    let label = if translation.shown {
        format!("Translated to {} · Show original", translation.language)
    } else {
        "Show translation".into()
    };
    Some(crate::screens::button(
        &label,
        &format!("translation-toggle-{}", message.id),
        move |s: &mut UiState<A>, _| {
            if let Some(cached) = s.translation.cached.get_mut(&key) {
                cached.shown = !cached.shown;
            }
        },
    ))
}

pub fn dialog<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let message = s.translation.dialog.clone()?;
    let body = col()
        .w_full()
        .gap(8.0)
        .padding(16.0)
        .child(text("Translate message").font(ts(18.0, 600.0)))
        .child(
            text_input(&s.translation.language, ts(14.0, 400.0))
                .key("translation-language")
                .label("Target language code")
                .placeholder("Language code: en, fr, de…", crate::ui::c(crate::ui::MUTED))
                .w_full()
                .on_edit(|s: &mut UiState<A>| &mut s.translation.language),
        )
        .child(crate::screens::button(
            "Translate",
            "translation-submit",
            move |s: &mut UiState<A>, _| request(s, &message),
        ));
    Some(crate::shortcuts::card(
        "translation-dialog",
        420.0,
        body,
        |s: &mut UiState<A>| s.translation.dialog = None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct Fake(RefCell<Vec<String>>);
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn translate_message(&self, c: &str, m: &str, _: String, language: String) {
            self.0.borrow_mut().push(format!("{c} {m} {language}"));
        }
    }
    fn state() -> UiState<Fake> {
        let mut s = UiState::new(Fake::default());
        s.selected = Some("chat".into());
        s.messages = crate::bridge::message_rows(
            &[store::MessageRecord {
                conversation_id: "chat".into(),
                message_id: "m".into(),
                body_html: "<b>Bonjour</b>".into(),
                ..Default::default()
            }],
            "me",
            chrono::Local::now(),
        );
        s
    }
    #[test]
    fn translation_keeps_the_original_and_never_crosses_conversations_or_edits() {
        let mut s = state();
        request(&mut s, "m");
        assert_eq!(*s.actions.0.borrow(), ["chat m en"]);
        apply(
            &mut s,
            "chat".into(),
            "m".into(),
            "Bonjour".into(),
            "en".into(),
            Ok("<b>Hello</b>".into()),
        );
        assert_eq!(s.messages[0].text, "Bonjour");
        assert!(cached(&s, &s.messages[0]).is_some());
        s.selected = Some("other".into());
        assert!(cached(&s, &s.messages[0]).is_none());
        s.selected = Some("chat".into());
        s.messages[0].text = "Edited".into();
        assert!(cached(&s, &s.messages[0]).is_none());
    }
    #[test]
    fn automatic_translation_uses_the_target_once_and_respects_excluded_languages() {
        let mut s = state();
        s.settings.prefs.translation = Some(teams_core::LanguageSettings {
            target_locale: Some("en".into()),
            behavior: teams_core::TranslationBehavior::Auto,
            ..Default::default()
        });
        poll(&mut s);
        s.translation
            .stamps
            .insert("m".into(), "languages=fr:100;length:80".into());
        assert!(poll(&mut s));
        assert!(!poll(&mut s));
        assert_eq!(*s.actions.0.borrow(), ["chat m en"]);
        s.translation.auto_attempted.clear();
        s.settings
            .prefs
            .translation
            .as_mut()
            .unwrap()
            .authoring_locales
            .push("fr-FR".into());
        assert!(!poll(&mut s));
        assert_eq!(s.actions.0.borrow().len(), 1);
    }

    #[test]
    fn invalid_targets_are_rejected_and_failed_requests_can_be_retried() {
        let mut s = state();
        s.translation.language = TextEdit::new("../bad");
        request(&mut s, "m");
        assert!(s.actions.0.borrow().is_empty());
        s.translation.language = TextEdit::new("fr");
        request(&mut s, "m");
        apply(
            &mut s,
            "chat".into(),
            "m".into(),
            "Bonjour".into(),
            "fr".into(),
            Err("offline".into()),
        );
        assert!(s.translation.pending.is_empty());
        assert!(s.translation.cached.is_empty());
        request(&mut s, "m");
        assert_eq!(s.actions.0.borrow().len(), 2);
    }
}
