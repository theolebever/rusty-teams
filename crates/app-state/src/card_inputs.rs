use regex::Regex;
use teams_core::{
    CardAction, CardActionKind, CardInput, ChoiceInput, ChoiceQuery, ChoiceStyle, InputChoice,
    collect_input_values,
};
use tokio::sync::oneshot;

use crate::Host;
use crate::app_state::AppState;
use crate::card_actions::CardTask;
use crate::card_state::{CardScope, Surface, short_reason};
use crate::runtime;

const NOT_CONNECTED: &str = "Not connected";
const READ_ONLY: &str = "Read-only mode";
pub const SEARCH_FAILED: &str = "Search failed";
const INLINE_ACTION_SUFFIX: &str = "inline";

pub type SearchAnswer = Result<Vec<InputChoice>, String>;

pub fn uses_select(choice: &ChoiceInput) -> bool {
    choice.query.is_some() || (!choice.multi && choice.style != ChoiceStyle::Expanded)
}

pub fn length_pattern(max_length: usize) -> Option<Regex> {
    Regex::new(&format!("^(?s:.{{0,{max_length}}})$")).ok()
}

pub fn inline_action_key(input_key: &str) -> String {
    format!("{input_key}-{INLINE_ACTION_SUFFIX}")
}

impl AppState {
    pub fn validate_card_input(
        &mut self,
        scope: &CardScope,
        key: &str,
        input: &CardInput,
        cx: &mut dyn Host,
    ) {
        let value = self.cards.input_value(key).to_owned();
        match input.validate(&value) {
            Some(message) => self.cards.set_input_error(key, message),
            None => self.cards.clear_input_errors(&[key.to_owned()]),
        }
        self.announce_cards(scope, cx);
    }

    pub fn run_inline_action(
        &mut self,
        scope: &CardScope,
        action_key: String,
        action: &CardAction,
        inputs: &[CardInput],
        cx: &mut dyn Host,
    ) {
        match &action.kind {
            CardActionKind::OpenUrl(url) => cx.open_url(url),
            _ => self.run_card_action(
                scope.clone(),
                action_key,
                action.clone(),
                inputs.to_vec(),
                cx,
            ),
        }
    }

    pub fn card_choice_search(
        &self,
        scope: &CardScope,
        query: &ChoiceQuery,
        query_text: &str,
    ) -> oneshot::Receiver<SearchAnswer> {
        if self.mode.demo {
            let (dataset, query_text) = (query.dataset.clone(), query_text.to_owned());
            return runtime::spawn(async move {
                Ok(crate::demo_input_cards::search(&dataset, &query_text))
            });
        }
        if self.mode.read_only {
            return runtime::spawn(async { Err(READ_ONLY.to_owned()) });
        }
        let Some(engine) = self.engine.clone() else {
            return runtime::spawn(async { Err(NOT_CONNECTED.to_owned()) });
        };
        let (conversation_id, message_id) =
            (scope.conversation_id.clone(), scope.message_id.clone());
        let (query, query_text) = (query.clone(), query_text.to_owned());
        runtime::spawn(async move {
            engine
                .card_search(&conversation_id, &message_id, &query, &query_text)
                .await
                .map_err(|error| short_reason(&error.to_string()))
        })
    }

    /// Records a remote choice search answer and hands back the choices it found.
    pub fn note_choice_search(
        &mut self,
        scope: &CardScope,
        key: &str,
        answer: SearchAnswer,
        cx: &mut dyn Host,
    ) -> Option<Vec<InputChoice>> {
        match answer {
            Ok(choices) => {
                if self.cards.input_error(key) == Some(SEARCH_FAILED) {
                    self.cards.clear_input_errors(&[key.to_owned()]);
                    self.announce_cards(scope, cx);
                }
                Some(choices)
            }
            Err(_) => {
                self.cards.set_input_error(key, SEARCH_FAILED.to_owned());
                self.announce_cards(scope, cx);
                None
            }
        }
    }

    pub fn edit_card_input(
        &mut self,
        scope: &CardScope,
        key: &str,
        value: String,
        cx: &mut dyn Host,
    ) {
        if self.cards.set_input_value(key, value) {
            self.announce_cards(scope, cx);
        }
    }

    pub fn pick_card_input(
        &mut self,
        scope: &CardScope,
        key: &str,
        value: String,
        cx: &mut dyn Host,
    ) {
        self.cards.set_input_value(key, value);
        self.announce_cards(scope, cx);
    }

    pub fn card_task(
        &mut self,
        scope: &CardScope,
        action: CardAction,
        inputs: &[CardInput],
        cx: &mut dyn Host,
    ) -> Option<CardTask> {
        let submits = matches!(
            action.kind,
            CardActionKind::Submit(_) | CardActionKind::Execute(_)
        );
        let values = if submits && action.collects_inputs {
            let keys: Vec<String> = inputs
                .iter()
                .map(|input| scope.input_key(&input.id))
                .collect();
            self.cards.clear_input_errors(&keys);
            let collected = collect_input_values(inputs, |input| {
                self.cards
                    .input_value(&scope.input_key(&input.id))
                    .to_owned()
            });
            match collected {
                Ok(values) => values,
                Err(errors) => {
                    for error in errors {
                        self.cards
                            .set_input_error(&scope.input_key(&error.id), error.message);
                    }
                    self.announce_cards(scope, cx);
                    return None;
                }
            }
        } else {
            serde_json::Map::new()
        };
        let action = action.with_inputs(&values);
        Some(match (&scope.surface, &action.kind) {
            (Surface::Dialog, CardActionKind::Submit(submit)) => {
                CardTask::Submit(submit.data.clone())
            }
            _ => CardTask::Action(action),
        })
    }
}
