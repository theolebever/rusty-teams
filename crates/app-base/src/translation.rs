use std::collections::{HashMap, HashSet, VecDeque};

use chrono::{DateTime, Utc};
use store::MessageRecord;
use teams_core::{
    FALLBACK_TARGET, Language, LanguageSettings, LanguageStamp, TranslationBehavior,
    default_target, language_name, offered_language,
};

pub type MessageKey = (String, String);

/// Translated HTML is the largest per-message payload kept in memory. Browsing a long history
/// must not keep every translation of every message for the whole session.
const MAX_CACHED_TRANSLATIONS: usize = 512;
/// Bookkeeping sets grow one entry per message seen, without holding the HTML.
const MAX_TRACKED_MESSAGES: usize = 2_000;

#[derive(Debug, Clone, PartialEq)]
pub enum TranslationLine {
    Offer {
        language_code: String,
        language: String,
    },
    Working,
    Failed,
    Translated {
        language: Option<String>,
        showing_original: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CachedTranslation {
    pub html: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    conversation_id: String,
    message_id: String,
    version: i64,
    target: String,
}

#[derive(Debug, Clone, Default)]
pub struct TranslationContext {
    pub enabled: bool,
    pub target: String,
    pub behavior: TranslationBehavior,
    pub never: Vec<String>,
    pub languages: Vec<Language>,
    pub stamps: HashMap<String, String>,
    pub cached: HashMap<(String, i64), CachedTranslation>,
    pub shown: HashSet<String>,
    pub working: HashSet<String>,
    pub failed: HashSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RowTranslation {
    pub line: Option<TranslationLine>,
    pub translated_html: Option<String>,
}

pub fn message_version(record: &MessageRecord) -> i64 {
    record
        .edited_at
        .map_or(0, |edited| edited.timestamp_millis())
}

impl TranslationContext {
    pub fn resolve(&self, record: &MessageRecord, own: bool) -> RowTranslation {
        if !self.enabled || record.deleted {
            return RowTranslation::default();
        }
        let id = &record.message_id;
        if self.working.contains(id) {
            return line_only(TranslationLine::Working);
        }
        let version = message_version(record);
        if let Some(cached) = self.cached.get(&(id.clone(), version)) {
            let showing = self.shown.contains(id);
            return RowTranslation {
                line: Some(TranslationLine::Translated {
                    language: cached
                        .source
                        .as_deref()
                        .map(|code| language_name(code, &self.languages)),
                    showing_original: !showing,
                }),
                translated_html: showing.then(|| cached.html.clone()),
            };
        }
        if self.failed.contains(id) {
            return line_only(TranslationLine::Failed);
        }
        if own || self.behavior != TranslationBehavior::Ask {
            return RowTranslation::default();
        }
        let offered = self
            .stamps
            .get(id)
            .and_then(|stamp| offered_language(stamp, &self.target, &self.never));
        match offered {
            Some(code) => line_only(TranslationLine::Offer {
                language: language_name(&code, &self.languages),
                language_code: code,
            }),
            None => RowTranslation::default(),
        }
    }
}

fn line_only(line: TranslationLine) -> RowTranslation {
    RowTranslation {
        line: Some(line),
        translated_html: None,
    }
}

#[derive(Default)]
pub struct TranslationState {
    pub settings: Option<LanguageSettings>,
    pub languages: Vec<Language>,
    pub languages_fetched_at: Option<DateTime<Utc>>,
    pub stamps: HashMap<String, HashMap<String, String>>,
    pub stamps_in_flight: HashSet<String>,
    asked: HashMap<String, HashSet<String>>,
    pub auto_scheduled: HashSet<String>,
    cache: HashMap<CacheKey, CachedTranslation>,
    shown: HashSet<MessageKey>,
    working: HashSet<MessageKey>,
    failed: HashSet<MessageKey>,
    /// Oldest-first insertion order for each bounded collection above.
    cache_order: VecDeque<CacheKey>,
    stamp_order: VecDeque<MessageKey>,
    asked_order: VecDeque<MessageKey>,
    shown_order: VecDeque<MessageKey>,
    working_order: VecDeque<MessageKey>,
    failed_order: VecDeque<MessageKey>,
}

/// Tracks a key in a set and its oldest-first queue, evicting the oldest once `cap` is exceeded.
fn track(
    set: &mut HashSet<MessageKey>,
    order: &mut VecDeque<MessageKey>,
    key: MessageKey,
    cap: usize,
) {
    order.retain(|tracked| tracked != &key);
    order.push_back(key.clone());
    set.insert(key);
    while order.len() > cap
        && let Some(oldest) = order.pop_front()
    {
        set.remove(&oldest);
    }
}

impl TranslationState {
    pub fn target(&self) -> String {
        match &self.settings {
            Some(settings) => default_target(
                settings.target_locale.as_deref(),
                settings.display_locale.as_deref(),
                &self.languages,
            ),
            None => FALLBACK_TARGET.to_owned(),
        }
    }

    pub fn behavior(&self) -> TranslationBehavior {
        self.settings
            .as_ref()
            .map_or(TranslationBehavior::Ask, |settings| settings.behavior)
    }

    /// The target counts as known, so nothing in it is ever offered.
    pub fn never_codes(&self) -> Vec<String> {
        let mut codes = self
            .settings
            .as_ref()
            .map(LanguageSettings::never_translate_codes)
            .unwrap_or_default();
        let target = self.target();
        if !codes.contains(&target) {
            codes.push(target);
        }
        codes
    }

    pub fn display_codes(&self) -> Vec<String> {
        self.settings
            .as_ref()
            .map(LanguageSettings::never_translate_codes)
            .unwrap_or_default()
    }

    pub fn language_name(&self, code: &str) -> String {
        language_name(code, &self.languages)
    }

    pub fn context_for(&self, conversation_id: &str) -> TranslationContext {
        let target = self.target();
        let of_conversation = |key: &MessageKey| (key.0 == conversation_id).then(|| key.1.clone());
        TranslationContext {
            enabled: self.settings.is_some(),
            behavior: self.behavior(),
            never: self.never_codes(),
            languages: self.languages.clone(),
            stamps: self
                .stamps
                .get(conversation_id)
                .cloned()
                .unwrap_or_default(),
            cached: self
                .cache
                .iter()
                .filter(|(key, _)| key.conversation_id == conversation_id && key.target == target)
                .map(|(key, cached)| ((key.message_id.clone(), key.version), cached.clone()))
                .collect(),
            shown: self.shown.iter().filter_map(of_conversation).collect(),
            working: self.working.iter().filter_map(of_conversation).collect(),
            failed: self.failed.iter().filter_map(of_conversation).collect(),
            target,
        }
    }

    pub fn is_cached(&self, conversation_id: &str, message_id: &str, version: i64) -> bool {
        self.cache.contains_key(&CacheKey {
            conversation_id: conversation_id.to_owned(),
            message_id: message_id.to_owned(),
            version,
            target: self.target(),
        })
    }

    pub fn is_pending(&self, conversation_id: &str, message_id: &str) -> bool {
        self.working
            .contains(&(conversation_id.to_owned(), message_id.to_owned()))
    }

    pub fn is_failed(&self, conversation_id: &str, message_id: &str) -> bool {
        self.failed
            .contains(&(conversation_id.to_owned(), message_id.to_owned()))
    }

    pub fn start(&mut self, conversation_id: &str, message_id: &str) {
        let key = (conversation_id.to_owned(), message_id.to_owned());
        self.failed.remove(&key);
        track(
            &mut self.working,
            &mut self.working_order,
            key,
            MAX_TRACKED_MESSAGES,
        );
    }

    pub fn fail(&mut self, conversation_id: &str, message_id: &str) {
        let key = (conversation_id.to_owned(), message_id.to_owned());
        self.working.remove(&key);
        track(
            &mut self.failed,
            &mut self.failed_order,
            key,
            MAX_TRACKED_MESSAGES,
        );
    }

    pub fn finish(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        version: i64,
        target: &str,
        html: String,
    ) {
        let key = (conversation_id.to_owned(), message_id.to_owned());
        self.working.remove(&key);
        self.failed.remove(&key);
        let source = self
            .stamps
            .get(conversation_id)
            .and_then(|stamps| stamps.get(message_id))
            .and_then(|stamp| LanguageStamp::parse(stamp))
            .and_then(|stamp| stamp.candidates.first().map(|(code, _)| code.clone()));
        let cache_key = CacheKey {
            conversation_id: conversation_id.to_owned(),
            message_id: message_id.to_owned(),
            version,
            target: target.to_owned(),
        };
        self.cache_order.retain(|cached| cached != &cache_key);
        self.cache_order.push_back(cache_key.clone());
        self.cache
            .insert(cache_key, CachedTranslation { html, source });
        while self.cache_order.len() > MAX_CACHED_TRANSLATIONS
            && let Some(oldest) = self.cache_order.pop_front()
        {
            self.cache.remove(&oldest);
        }
        track(
            &mut self.shown,
            &mut self.shown_order,
            key,
            MAX_TRACKED_MESSAGES,
        );
    }

    /// Flips between the translation and the original of a message that has a stored translation.
    pub fn toggle(&mut self, conversation_id: &str, message_id: &str) {
        let key = (conversation_id.to_owned(), message_id.to_owned());
        if !self.shown.remove(&key) {
            track(
                &mut self.shown,
                &mut self.shown_order,
                key,
                MAX_TRACKED_MESSAGES,
            );
        } else {
            self.shown_order.retain(|tracked| tracked != &key);
        }
    }

    pub fn mark_asked(&mut self, conversation_id: &str, message_ids: Vec<String>) {
        for message_id in message_ids {
            if self.was_asked(conversation_id, &message_id) {
                continue;
            }
            let key = (conversation_id.to_owned(), message_id.clone());
            self.asked_order.retain(|tracked| tracked != &key);
            self.asked_order.push_back(key);
            self.asked
                .entry(conversation_id.to_owned())
                .or_default()
                .insert(message_id);
        }
        while self.asked_order.len() > MAX_TRACKED_MESSAGES
            && let Some(oldest) = self.asked_order.pop_front()
        {
            if let Some(ids) = self.asked.get_mut(&oldest.0) {
                ids.remove(&oldest.1);
                if ids.is_empty() {
                    self.asked.remove(&oldest.0);
                }
            }
        }
    }

    pub fn was_asked(&self, conversation_id: &str, message_id: &str) -> bool {
        self.asked
            .get(conversation_id)
            .is_some_and(|asked| asked.contains(message_id))
    }

    pub fn merge_stamps(&mut self, conversation_id: &str, stamps: HashMap<String, String>) -> bool {
        let before = self
            .stamps
            .get(conversation_id)
            .map_or(0, |stamps| stamps.len());
        let mut changed = false;
        for (message_id, stamp) in stamps {
            let known = self.stamps.entry(conversation_id.to_owned()).or_default();
            if known.get(&message_id) == Some(&stamp) {
                continue;
            }
            known.insert(message_id.clone(), stamp);
            let key = (conversation_id.to_owned(), message_id);
            self.stamp_order.retain(|tracked| tracked != &key);
            self.stamp_order.push_back(key);
            changed = true;
        }
        while self.stamp_order.len() > MAX_TRACKED_MESSAGES
            && let Some(oldest) = self.stamp_order.pop_front()
        {
            if let Some(known) = self.stamps.get_mut(&oldest.0) {
                known.remove(&oldest.1);
                if known.is_empty() {
                    self.stamps.remove(&oldest.0);
                }
            }
        }
        changed
            || self
                .stamps
                .get(conversation_id)
                .map_or(0, |stamps| stamps.len())
                != before
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    const FRENCH: &str = "languages=fr:100;en:10;length:80;&detector=Bling";

    fn record(id: &str) -> MessageRecord {
        MessageRecord {
            message_id: id.to_owned(),
            conversation_id: "chat".to_owned(),
            body_html: "<p>Bonjour</p>".to_owned(),
            ..MessageRecord::default()
        }
    }

    fn state_with_stamp() -> TranslationState {
        let mut state = TranslationState {
            settings: Some(LanguageSettings::default()),
            ..TranslationState::default()
        };
        state.merge_stamps(
            "chat",
            HashMap::from([("m1".to_owned(), FRENCH.to_owned())]),
        );
        state
    }

    #[test]
    fn a_confident_foreign_message_gets_an_offer_in_ask_mode() {
        let context = state_with_stamp().context_for("chat");
        let resolved = context.resolve(&record("m1"), false);
        assert_eq!(
            resolved.line,
            Some(TranslationLine::Offer {
                language_code: "fr".into(),
                language: "French".into()
            })
        );
        assert_eq!(resolved.translated_html, None);
    }

    #[test]
    fn own_messages_and_unknown_settings_get_no_offer() {
        let context = state_with_stamp().context_for("chat");
        assert_eq!(
            context.resolve(&record("m1"), true),
            RowTranslation::default()
        );
        let mut unready = state_with_stamp();
        unready.settings = None;
        assert_eq!(
            unready.context_for("chat").resolve(&record("m1"), false),
            RowTranslation::default()
        );
    }

    #[test]
    fn other_modes_and_the_never_list_get_no_offer() {
        let mut state = state_with_stamp();
        let settings = state.settings.as_mut().unwrap();
        settings.behavior = TranslationBehavior::Never;
        assert_eq!(
            state.context_for("chat").resolve(&record("m1"), false),
            RowTranslation::default()
        );
        let settings = state.settings.as_mut().unwrap();
        settings.behavior = TranslationBehavior::Ask;
        settings.authoring_locales = vec!["fr-FR".into()];
        assert_eq!(
            state.context_for("chat").resolve(&record("m1"), false),
            RowTranslation::default()
        );
    }

    #[test]
    fn a_message_without_a_stamp_gets_no_offer() {
        let context = state_with_stamp().context_for("chat");
        assert_eq!(
            context.resolve(&record("m2"), false),
            RowTranslation::default()
        );
    }

    #[test]
    fn working_then_translated_then_toggled_back() {
        let mut state = state_with_stamp();
        state.start("chat", "m1");
        let working = state.context_for("chat").resolve(&record("m1"), false);
        assert_eq!(working.line, Some(TranslationLine::Working));

        state.finish("chat", "m1", 0, "en", "<p>Hello</p>".into());
        let shown = state.context_for("chat").resolve(&record("m1"), false);
        assert_eq!(shown.translated_html.as_deref(), Some("<p>Hello</p>"));
        assert_eq!(
            shown.line,
            Some(TranslationLine::Translated {
                language: Some("French".into()),
                showing_original: false
            })
        );

        state.toggle("chat", "m1");
        let original = state.context_for("chat").resolve(&record("m1"), false);
        assert_eq!(original.translated_html, None);
        assert_eq!(
            original.line,
            Some(TranslationLine::Translated {
                language: Some("French".into()),
                showing_original: true
            })
        );

        state.toggle("chat", "m1");
        assert!(
            state
                .context_for("chat")
                .resolve(&record("m1"), false)
                .translated_html
                .is_some()
        );
    }

    #[test]
    fn a_failed_request_shows_the_retry_line_until_started_again() {
        let mut state = state_with_stamp();
        state.start("chat", "m1");
        state.fail("chat", "m1");
        assert_eq!(
            state.context_for("chat").resolve(&record("m1"), false).line,
            Some(TranslationLine::Failed)
        );
        state.start("chat", "m1");
        assert!(!state.is_failed("chat", "m1"));
        assert!(state.is_pending("chat", "m1"));
    }

    #[test]
    fn an_edit_or_a_new_target_misses_the_cache() {
        let mut state = state_with_stamp();
        state.finish("chat", "m1", 0, "en", "<p>Hello</p>".into());
        assert!(state.is_cached("chat", "m1", 0));
        assert!(!state.is_cached("chat", "m1", 1));
        let mut edited = record("m1");
        edited.edited_at = Utc.timestamp_millis_opt(5_000).single();
        assert_eq!(message_version(&edited), 5_000);
        assert!(
            state
                .context_for("chat")
                .resolve(&edited, true)
                .translated_html
                .is_none()
        );

        state.settings.as_mut().unwrap().target_locale = Some("de".into());
        assert!(!state.is_cached("chat", "m1", 0));
        assert!(state.context_for("chat").cached.is_empty());
    }

    #[test]
    fn contexts_only_carry_their_own_conversation() {
        let mut state = state_with_stamp();
        state.finish("chat", "m1", 0, "en", "<p>Hello</p>".into());
        let other = state.context_for("other");
        assert!(other.cached.is_empty());
        assert!(other.shown.is_empty());
        assert!(other.stamps.is_empty());
    }

    #[test]
    fn the_target_is_always_a_known_language() {
        let state = state_with_stamp();
        assert_eq!(state.never_codes(), vec!["en".to_owned()]);
        assert!(state.display_codes().is_empty());
    }

    #[test]
    fn asked_messages_are_remembered_per_conversation() {
        let mut state = TranslationState::default();
        state.mark_asked("chat", vec!["m1".into()]);
        assert!(state.was_asked("chat", "m1"));
        assert!(!state.was_asked("chat", "m2"));
        assert!(!state.was_asked("other", "m1"));
    }

    #[test]
    fn merging_stamps_reports_changes_only() {
        let mut state = TranslationState::default();
        let stamps = HashMap::from([("m1".to_owned(), FRENCH.to_owned())]);
        assert!(state.merge_stamps("chat", stamps.clone()));
        assert!(!state.merge_stamps("chat", stamps));
    }

    #[test]
    fn translation_caches_are_bounded() {
        let mut state = TranslationState::default();
        for index in 0..MAX_CACHED_TRANSLATIONS + 5 {
            let id = format!("m{index}");
            state.finish("chat", &id, 0, "en", format!("<p>{id}</p>"));
        }
        assert_eq!(state.cache.len(), MAX_CACHED_TRANSLATIONS);
        assert_eq!(state.cache_order.len(), MAX_CACHED_TRANSLATIONS);
        assert!(state.is_cached("chat", "m5", 0));
        assert!(!state.is_cached("chat", "m0", 0));

        for index in 0..MAX_TRACKED_MESSAGES + 5 {
            let id = format!("asked{index}");
            state.mark_asked("chat", vec![id.clone()]);
            assert!(state.was_asked("chat", &id));
        }
        assert!(state.asked_order.len() <= MAX_TRACKED_MESSAGES);
        assert!(!state.was_asked("chat", "asked0"));
    }
}
