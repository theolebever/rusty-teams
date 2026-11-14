//! Card form values, validation, action responses and task dialogs.

use std::collections::{HashMap, HashSet};
use rune::*;
use teams_core::{AdaptiveCard, CardAction, CardActionKind, CardActionOutcome, CardInput, CardInputKind, TaskDialog, TaskDialogKind};
use crate::types::Actions;
use crate::ui::{UiState, text, ts};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Scope { pub conversation: String, pub message: String, pub key: String, pub task: bool }

#[derive(Default)]
pub struct State {
    pub inputs: HashMap<(Scope, String), TextEdit>,
    pub parents: HashMap<Scope, Vec<(Scope, CardInput)>>,
    pub errors: HashMap<Scope, String>,
    pub busy: HashSet<Scope>,
    pub replacements: HashMap<Scope, AdaptiveCard>,
    pub shown: HashSet<(Scope, usize)>,
    pub visibility: HashMap<(Scope, String), bool>,
    pub refreshed: HashSet<Scope>,
    pub dialog: Option<(Scope, TaskDialog)>,
}

pub fn input<A: Actions + 'static>(s: &UiState<A>, scope: &Scope, input: &CardInput) -> El {
    let key = (scope.clone(), input.id.clone());
    let initial = TextEdit::new(input.initial_value());
    let current = s.cards.inputs.get(&key).unwrap_or(&initial);
    let mut body = col().key(format!("{}-input-{}", scope.key, input.id)).w_full().gap(4.0)
        .children(input.label.as_ref().map(|label| text(format!("{label}{}", if input.required { " *" } else { "" })).font(ts(13.0, 600.0))));
    let choices: Option<Vec<(String, String, bool)>> = match &input.kind {
        CardInputKind::Choice(choice) => Some(choice.choices.iter().map(|item| (item.title.clone(), choice.toggled(&current.text, &item.value), choice.is_selected(&current.text, &item.value))).collect()),
        CardInputKind::Toggle(toggle) => Some(vec![(toggle.title.clone(), toggle.flipped(&current.text), toggle.is_on(&current.text))]),
        CardInputKind::Rating(rating) => Some((1..=rating.max).map(|number| ("★".repeat(number as usize), rating.value_text(f64::from(number)), rating.rating(&current.text) == f64::from(number))).collect()),
        _ => None,
    };
    if let Some(choices) = choices {
        for (index, (label, value, on)) in choices.into_iter().enumerate() {
            let key = key.clone();
            let choice = crate::screens::button(&format!("{} {label}", if on { "☑" } else { "☐" }), &format!("{}-choice-{}-{index}", scope.key, input.id), move |s: &mut UiState<A>, _| {
                if !s.read_only && !s.cards.busy.contains(&key.0) { s.cards.inputs.insert(key.clone(), TextEdit::new(&value)); }
            });
            body = body.child(choice);
        }
    } else {
        let input_copy = input.clone();
        let placeholder = match &input.kind {
            CardInputKind::Text(text) => text.placeholder.as_deref().unwrap_or_default(),
            CardInputKind::Number(number) => number.placeholder.as_deref().unwrap_or_default(),
            CardInputKind::Date(_) => "YYYY-MM-DD", CardInputKind::Time(_) => "HH:MM", _ => "",
        };
        let secure = matches!(&input.kind, CardInputKind::Text(text) if text.style == teams_core::TextStyle::Password);
        let mut field = text_input(current, ts(14.0, 400.0)).key(format!("{}-field-{}", scope.key, input.id))
            .label(input.label.clone().unwrap_or_else(|| input.id.clone())).placeholder(placeholder, crate::ui::c(crate::ui::MUTED)).w_full()
            .on_edit(move |s: &mut UiState<A>| s.cards.inputs.entry(key.clone()).or_insert_with(|| {
                let edit = TextEdit::new(input_copy.initial_value());
                if secure { edit.secure() } else { edit }
            }));
        if secure { field = field.secure(); }
        body = body.child(field);
    }
    body
}

pub fn run<A: Actions + 'static>(s: &mut UiState<A>, scope: Scope, card: &AdaptiveCard, index: usize, action: CardAction, tx: &mut Tx) {
    if !action.enabled || !action.visible { return; }
    match &action.kind {
        CardActionKind::OpenUrl(url) => { crate::rich_text::open(s, tx, url); return; }
        CardActionKind::ShowCard(_) => {
            let child = Scope { key: format!("{}-show-{index}", scope.key), ..scope.clone() };
            let mut parents = s.cards.parents.get(&scope).cloned().unwrap_or_default();
            parents.extend(card.own_inputs().into_iter().map(|input| (scope.clone(), input)));
            s.cards.parents.insert(child, parents);
            let key = (scope, index);
            if !s.cards.shown.remove(&key) { s.cards.shown.insert(key); }
            return;
        }
        CardActionKind::ToggleVisibility(targets) => {
            let initial = card.element_visibility();
            for target in targets {
                let key = (scope.clone(), target.element_id.clone());
                let current = s.cards.visibility.get(&key).copied().or_else(|| initial.get(&target.element_id).copied()).unwrap_or(true);
                s.cards.visibility.insert(key, target.visible.unwrap_or(!current));
            }
            return;
        }
        _ => {}
    }
    if s.read_only || s.cards.busy.contains(&scope) { return; }
    let mut owners = s.cards.parents.get(&scope).cloned().unwrap_or_default();
    owners.extend(card.own_inputs().into_iter().map(|input| (scope.clone(), input)));
    let inputs: Vec<_> = owners.iter().map(|(_, input)| input.clone()).collect();
    let values = if action.collects_inputs {
        match teams_core::collect_input_values(&inputs, |input| {
            let owner = owners.iter().rev().find(|(_, candidate)| candidate.id == input.id).map(|(owner, _)| owner).unwrap_or(&scope);
            s.cards.inputs.get(&(owner.clone(), input.id.clone())).map(|edit| edit.text.clone()).unwrap_or_else(|| input.initial_value())
        }) {
            Ok(values) => values,
            Err(errors) => { s.cards.errors.insert(scope, errors.iter().map(|error| format!("{}: {}", error.id, error.message)).collect::<Vec<_>>().join("\n")); return; }
        }
    } else { serde_json::Map::new() };
    s.cards.errors.remove(&scope);
    s.cards.busy.insert(scope.clone());
    s.actions.card_action(scope, action.with_inputs(&values));
}

pub fn apply<A: Actions>(s: &mut UiState<A>, scope: Scope, result: Result<CardActionOutcome, String>) {
    s.cards.busy.remove(&scope);
    match result {
        Ok(CardActionOutcome::Sent) => s.toast.show("Sent"),
        Ok(CardActionOutcome::ReplaceCard(content)) => match AdaptiveCard::parse(&content) {
            Some(card) => { s.cards.replacements.insert(scope.clone(), card); s.cards.inputs.retain(|(owner, _), _| owner != &scope); }
            None => { s.cards.errors.insert(scope, "The app returned an unreadable card".into()); }
        },
        Ok(CardActionOutcome::Dialog(dialog)) => s.cards.dialog = Some((scope, dialog)),
        Ok(CardActionOutcome::Message(message)) => s.toast.show(message),
        Ok(CardActionOutcome::Failed(error)) | Err(error) => { s.cards.errors.insert(scope, error); }
    }
}

pub fn dialog<A: Actions + 'static>(s: &UiState<A>) -> Option<El> {
    let (scope, dialog) = s.cards.dialog.as_ref()?;
    let mut body = col().w_full().gap(8.0).padding(16.0)
        .child(text(dialog.title.clone().unwrap_or_else(|| "Task".into())).font(ts(18.0, 600.0)));
    match &dialog.kind {
        TaskDialogKind::Card(content) => {
            if let Some(card) = AdaptiveCard::parse(content) {
                let scope = Scope { key: format!("{}-task", scope.key), task: true, ..scope.clone() };
                body = body.child(crate::cards::scoped(s, &card, &scope));
            } else { body = body.child(text("The app returned an unreadable task card")); }
        }
        TaskDialogKind::Url { url, .. } => {
            let url = url.clone();
            body = body.child(crate::screens::button("Open task", "card-task-open", move |s: &mut UiState<A>, tx| crate::rich_text::open(s, tx, &url)));
        }
    }
    Some(crate::shortcuts::card("card-task-dialog", dialog.width.clamp(320, 900) as f32, body, |s: &mut UiState<A>| s.cards.dialog = None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct Fake(RefCell<Vec<CardAction>>);
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn card_action(&self, _: Scope, action: CardAction) { self.0.borrow_mut().push(action); }
    }
    fn scope() -> Scope { Scope { conversation: "chat".into(), message: "message".into(), key: "card".into(), task: false } }

    #[test]
    fn submit_validates_required_fields_merges_data_and_prevents_double_submission() {
        let card = AdaptiveCard::parse(r#"{"type":"AdaptiveCard","body":[{"type":"Input.Text","id":"name","isRequired":true}],"actions":[{"type":"Action.Submit","title":"Send","data":{"fixed":1}}]}"#).expect("card");
        let mut s = UiState::new(Fake::default());
        run(&mut s, scope(), &card, 0, card.actions[0].clone(), &mut Tx::default());
        assert!(s.actions.0.borrow().is_empty());
        assert!(s.cards.errors[&scope()].contains("Required"));
        s.cards.inputs.insert((scope(), "name".into()), TextEdit::new("Ada"));
        run(&mut s, scope(), &card, 0, card.actions[0].clone(), &mut Tx::default());
        run(&mut s, scope(), &card, 0, card.actions[0].clone(), &mut Tx::default());
        let calls = s.actions.0.borrow();
        assert_eq!(calls.len(), 1);
        let CardActionKind::Submit(submit) = &calls[0].kind else { panic!("submit action"); };
        assert_eq!(submit.data, serde_json::json!({"fixed":1,"name":"Ada"}));
    }

    #[test]
    fn showing_a_child_card_preserves_parent_inputs_and_read_only_blocks_server_actions() {
        let card = AdaptiveCard::parse(r#"{"type":"AdaptiveCard","body":[{"type":"Input.Text","id":"parent","value":"Ada","isRequired":true}],"actions":[{"type":"Action.ShowCard","title":"More","card":{"type":"AdaptiveCard","body":[{"type":"Input.Text","id":"child","value":"hello"}],"actions":[{"type":"Action.Execute","title":"Go","verb":"go"}]}}]}"#).expect("card");
        let mut s = UiState::new(Fake::default());
        run(&mut s, scope(), &card, 0, card.actions[0].clone(), &mut Tx::default());
        let CardActionKind::ShowCard(child) = &card.actions[0].kind else { panic!("child card"); };
        let child_scope = Scope { key: "card-show-0".into(), ..scope() };
        s.read_only = true;
        run(&mut s, child_scope.clone(), child, 0, child.actions[0].clone(), &mut Tx::default());
        assert!(s.actions.0.borrow().is_empty());
        s.read_only = false;
        run(&mut s, child_scope, child, 0, child.actions[0].clone(), &mut Tx::default());
        let calls = s.actions.0.borrow();
        let CardActionKind::Execute(execute) = &calls[0].kind else { panic!("execute action"); };
        assert_eq!(execute.data, serde_json::json!({"parent":"Ada","child":"hello"}));
    }

    #[test]
    fn rejected_actions_keep_form_values_for_retry() {
        let mut s = UiState::new(Fake::default());
        s.cards.inputs.insert((scope(), "name".into()), TextEdit::new("Ada"));
        s.cards.busy.insert(scope());
        apply(&mut s, scope(), Err("offline".into()));
        assert!(!s.cards.busy.contains(&scope()));
        assert_eq!(s.cards.inputs[&(scope(), "name".into())].text, "Ada");
        assert_eq!(s.cards.errors[&scope()], "offline");
    }

    #[test]
    fn rendered_card_fields_submit_the_value_entered_in_rune() {
        fn view(s: &UiState<Fake>, _: &mut Cx<UiState<Fake>>) -> El {
            let card = AdaptiveCard::parse(r#"{"type":"AdaptiveCard","body":[{"type":"Input.Text","id":"name","label":"Name","isRequired":true}],"actions":[{"type":"Action.Submit","title":"Send"}]}"#).expect("card");
            crate::cards::view(s, &card, "form", "message")
        }
        let mut state = UiState::new(Fake::default());
        state.selected = Some("chat".into());
        let mut rt = Runtime::new(state, view, (800.0, 600.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        let (x, y) = rt.find("form-field-name").expect("input field");
        rt.pointer_down(x, y); rt.pointer_up(x, y); rt.frame(1.0 / 60.0);
        rt.key(KeyEvent { key: Key::Text("Ada".into()), mods: Mods::default() }); rt.frame(1.0 / 60.0);
        let (x, y) = rt.find("form-action-0").expect("submit button");
        rt.pointer_down(x, y); rt.pointer_up(x, y); rt.frame(1.0 / 60.0);
        let calls = rt.state.actions.0.borrow();
        assert_eq!(calls.len(), 1);
        let CardActionKind::Submit(submit) = &calls[0].kind else { panic!("submit action"); };
        assert_eq!(submit.data, serde_json::json!({"name":"Ada"}));
    }
}
