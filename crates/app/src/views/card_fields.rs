//! GPUI widget entities for Adaptive Card inputs. `CardState` keeps the values; this
//! global keeps the widgets that edit them, keyed by input key.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::component::IndexPath;
use gpui_kit::component::calendar::Matcher;
use gpui_kit::component::date_picker::{DatePickerEvent, DatePickerState};
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::searchable_list::{
    SearchableListDelegate, SearchableListItem, SearchableVec,
};
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::component::time_field::{TimeFieldEvent, TimeFieldState};
use gpui_kit::*;
use regex::Regex;
use teams_core::{
    AdaptiveCard, CardInput, CardInputKind, ChoiceInput, ChoiceQuery, ChoiceStyle, InputChoice,
    TextStyle, format_date, format_time,
};

use crate::app_state::{AppHandle, AppState};
use crate::card_inputs::{
    SEARCH_FAILED, SearchAnswer, inline_action_key, length_pattern, uses_select,
};
use crate::card_state::CardScope;

const NUMBER_PATTERN: &str = r"^-?\d*\.?\d*$";
const AREA_MIN_ROWS: usize = 3;
const AREA_MAX_ROWS: usize = 8;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Default)]
struct CardFields(HashMap<String, FieldHandle>);

impl Global for CardFields {}

#[derive(Clone)]
pub struct ChoiceItem {
    pub title: SharedString,
    pub value: String,
}

impl SearchableListItem for ChoiceItem {
    type Value = String;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &String {
        &self.value
    }
}

pub struct RemoteSearch {
    scope: CardScope,
    key: String,
    query: ChoiceQuery,
}

pub struct ChoiceList {
    items: SearchableVec<ChoiceItem>,
    remote: Option<RemoteSearch>,
    search_task: Task<()>,
}

impl ChoiceList {
    fn new(items: Vec<ChoiceItem>, remote: Option<RemoteSearch>) -> Self {
        ChoiceList {
            items: SearchableVec::new(items),
            remote,
            search_task: Task::ready(()),
        }
    }

    fn found(choices: Vec<InputChoice>, remote: Option<RemoteSearch>) -> Self {
        ChoiceList::new(choice_item_list(&choices), remote)
    }
}

impl SearchableListDelegate for ChoiceList {
    type Item = ChoiceItem;

    fn items_count(&self, section: usize) -> usize {
        self.items.items_count(section)
    }

    fn item(&self, ix: IndexPath) -> Option<&ChoiceItem> {
        self.items.item(ix)
    }

    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        ChoiceItem: SearchableListItem<Value = V>,
        V: PartialEq,
    {
        self.items.position(value)
    }

    fn perform_search(&mut self, query: &str, window: &mut Window, cx: &mut App) -> Task<()> {
        let Some(remote) = &self.remote else {
            return self.items.perform_search(query, window, cx);
        };
        if query.is_empty() {
            return Task::ready(());
        }
        let (scope, key, choice_query, query_text) = (
            remote.scope.clone(),
            remote.key.clone(),
            remote.query.clone(),
            query.to_owned(),
        );
        self.search_task = window.spawn(cx, async move |cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            let app = cx.update(|_, cx| cx.global::<AppHandle>().0.clone()).ok();
            let Some(app) = app else {
                return;
            };
            let receiver = cx
                .update(|_, cx| {
                    app.update(cx, |state, _| {
                        state.card_choice_search(&scope, &choice_query, &query_text)
                    })
                })
                .ok();
            let answer = match receiver {
                Some(receiver) => receiver.await.unwrap_or_else(|_| Err(SEARCH_FAILED.into())),
                None => return,
            };
            cx.update(|window, cx| {
                app.update(cx, |state, cx| {
                    show_choice_results(state, &scope, &key, &choice_query, answer, window, cx)
                })
            })
            .ok();
        });
        Task::ready(())
    }
}

pub type ChoiceSelect = SelectState<ChoiceList>;

#[derive(Clone)]
pub enum InputField {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>),
    Select(Entity<ChoiceSelect>),
    Date(Entity<DatePickerState>),
    Time(Entity<TimeFieldState>),
}

struct FieldHandle {
    field: InputField,
    _subscriptions: Vec<Subscription>,
}

/// The widget editing `input_key`, if one was created for it.
pub fn input_field<'a>(input_key: &str, cx: &'a App) -> Option<&'a InputField> {
    cx.try_global::<CardFields>()?
        .0
        .get(input_key)
        .map(|handle| &handle.field)
}

fn choice_item_list(choices: &[InputChoice]) -> Vec<ChoiceItem> {
    choices
        .iter()
        .map(|choice| ChoiceItem {
            title: choice.title.clone().into(),
            value: choice.value.clone(),
        })
        .collect()
}

fn choice_list(choice: &ChoiceInput, scope: &CardScope, key: &str) -> ChoiceList {
    let remote = choice.query.clone().map(|query| RemoteSearch {
        scope: scope.clone(),
        key: key.to_owned(),
        query,
    });
    let mut choices = choice.choices.clone();
    if remote.is_some() && !choice.value.is_empty() {
        let known = choices
            .iter()
            .any(|candidate| candidate.value == choice.value);
        if !known {
            choices.push(InputChoice {
                title: choice.value.clone(),
                value: choice.value.clone(),
            });
        }
    }
    ChoiceList::found(choices, remote)
}

/// Seeds the values of `card`'s inputs in state and creates their widgets once.
pub fn ensure_card_inputs(
    state: &mut AppState,
    scope: &CardScope,
    card: &AdaptiveCard,
    window: &mut Window,
    cx: &mut Context<AppState>,
) {
    // Widgets of cleared cards go with their values.
    cx.default_global::<CardFields>()
        .0
        .retain(|key, _| state.cards.has_input(key));
    let inputs = card.all_inputs();
    for input in &inputs {
        let key = scope.input_key(&input.id);
        if state.cards.has_input(&key) {
            continue;
        }
        if let Some(handle) = create_field(scope, &key, input, &inputs, window, cx) {
            cx.global_mut::<CardFields>().0.insert(key.clone(), handle);
        }
        state.cards.seed_input(key, input.initial_value());
    }
}

fn create_field(
    scope: &CardScope,
    key: &str,
    input: &CardInput,
    inputs: &[CardInput],
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> Option<FieldHandle> {
    let value = input.initial_value();
    let (scope, key) = (scope.clone(), key.to_owned());
    let validated = input.clone();
    match &input.kind {
        CardInputKind::Text(text) if text.multiline => {
            let placeholder = text.placeholder.clone().unwrap_or_default();
            let max_length = text.max_length;
            let state = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(AREA_MIN_ROWS, AREA_MAX_ROWS)
                    .placeholder(placeholder)
                    .default_value(value)
            });
            let subscription = cx.subscribe_in(&state, window, {
                let (scope, key) = (scope.clone(), key.clone());
                move |this, state, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        let mut text = state.read(cx).value().to_string();
                        if let Some(max_length) = max_length
                            && text.chars().count() > max_length
                        {
                            text = text.chars().take(max_length).collect();
                            state.update(cx, |area, cx| area.set_value(text.clone(), window, cx));
                        }
                        this.edit_card_input(&scope, &key, text, cx);
                    }
                    InputEvent::Blur => this.validate_card_input(&scope, &key, &validated, cx),
                    _ => {}
                }
            });
            Some(FieldHandle {
                field: InputField::Area(state),
                _subscriptions: vec![subscription],
            })
        }
        CardInputKind::Text(_) | CardInputKind::Number(_) => {
            let (placeholder, masked, pattern, inline_action) = match &input.kind {
                CardInputKind::Text(text) => (
                    text.placeholder.clone(),
                    text.style == TextStyle::Password,
                    text.max_length.and_then(length_pattern),
                    text.inline_action.clone(),
                ),
                CardInputKind::Number(number) => (
                    number.placeholder.clone(),
                    false,
                    Regex::new(NUMBER_PATTERN).ok(),
                    None,
                ),
                _ => (None, false, None, None),
            };
            let state = cx.new(|cx| {
                let line = InputState::new(window, cx)
                    .placeholder(placeholder.unwrap_or_default())
                    .masked(masked)
                    .default_value(value);
                match pattern {
                    Some(pattern) => line.pattern(pattern),
                    None => line,
                }
            });
            let inputs = inputs.to_vec();
            let subscription = cx.subscribe(&state, {
                let (scope, key) = (scope.clone(), key.clone());
                move |this, state, event: &InputEvent, cx| match event {
                    InputEvent::Change => {
                        let text = state.read(cx).value().to_string();
                        this.edit_card_input(&scope, &key, text, cx);
                    }
                    InputEvent::Blur => this.validate_card_input(&scope, &key, &validated, cx),
                    InputEvent::PressEnter { .. } => {
                        if let Some(inline) = &inline_action {
                            this.run_inline_action(
                                &scope,
                                inline_action_key(&key),
                                &inline.action,
                                &inputs,
                                cx,
                            );
                        }
                    }
                    InputEvent::Focus => {}
                }
            });
            Some(FieldHandle {
                field: InputField::Line(state),
                _subscriptions: vec![subscription],
            })
        }
        CardInputKind::Date(moment) => {
            let (min, max) = (moment.min_date(), moment.max_date());
            let initial = moment.initial_date();
            let state = cx.new(|cx| {
                let mut picker = DatePickerState::new(window, cx).date_format("%Y-%m-%d");
                if min.is_some() || max.is_some() {
                    picker = picker.disabled_matcher(Matcher::interval(min, max));
                }
                if let Some(date) = initial {
                    picker.set_date(date, window, cx);
                }
                picker
            });
            let subscription = cx.subscribe(&state, {
                move |this, _, event: &DatePickerEvent, cx| {
                    let DatePickerEvent::Change(picked) = event;
                    let text = picked.date().start().map(format_date).unwrap_or_default();
                    this.pick_card_input(&scope, &key, text, cx);
                    this.validate_card_input(&scope, &key, &validated, cx);
                }
            });
            Some(FieldHandle {
                field: InputField::Date(state),
                _subscriptions: vec![subscription],
            })
        }
        CardInputKind::Time(moment) => {
            let initial = moment.initial_time();
            let state = cx.new(|cx| {
                let mut field = TimeFieldState::new(window, cx);
                if let Some(time) = initial {
                    field.set_time(time, window, cx);
                }
                field
            });
            let focus_handle = state.focus_handle(cx);
            let changed = cx.subscribe(&state, {
                let (scope, key) = (scope.clone(), key.clone());
                move |this, _, event: &TimeFieldEvent, cx| {
                    let TimeFieldEvent::Change(time) = event;
                    this.pick_card_input(&scope, &key, format_time(*time), cx);
                }
            });
            let blurred = cx.on_blur(&focus_handle, window, move |this, _, cx| {
                this.validate_card_input(&scope, &key, &validated, cx)
            });
            Some(FieldHandle {
                field: InputField::Time(state),
                _subscriptions: vec![changed, blurred],
            })
        }
        CardInputKind::Choice(choice) if uses_select(choice) => {
            let searchable = choice.style == ChoiceStyle::Filtered || choice.query.is_some();
            let list = choice_list(choice, &scope, &key);
            let selected = (0..list.items_count(0))
                .find(|row| {
                    list.item(IndexPath::default().row(*row))
                        .is_some_and(|item| item.value == value)
                })
                .map(|row| IndexPath::default().row(row));
            let state =
                cx.new(|cx| SelectState::new(list, selected, window, cx).searchable(searchable));
            let subscription = cx.subscribe(
                &state,
                move |this, _, event: &SelectEvent<ChoiceList>, cx| {
                    let SelectEvent::Confirm(picked) = event;
                    this.pick_card_input(&scope, &key, picked.clone().unwrap_or_default(), cx);
                },
            );
            Some(FieldHandle {
                field: InputField::Select(state),
                _subscriptions: vec![subscription],
            })
        }
        CardInputKind::Choice(_) | CardInputKind::Toggle(_) | CardInputKind::Rating(_) => None,
    }
}

fn show_choice_results(
    state: &mut AppState,
    scope: &CardScope,
    key: &str,
    query: &ChoiceQuery,
    answer: SearchAnswer,
    window: &mut Window,
    cx: &mut Context<AppState>,
) {
    let choices = state.note_choice_search(scope, key, answer, cx);
    if let (Some(choices), Some(InputField::Select(select))) =
        (choices, input_field(key, cx).cloned())
    {
        let remote = RemoteSearch {
            scope: scope.clone(),
            key: key.to_owned(),
            query: query.clone(),
        };
        select.update(cx, |select, cx| {
            select.set_items(ChoiceList::found(choices, Some(remote)), window, cx);
            cx.notify();
        });
    }
}
