//! The Agents settings pane, as GPUI's (`crates/app/src/agents/settings_section.rs`): the agent
//! and action lists, one form per item, presets and validation from the `agents` crate.
//! ponytail: the form saves on Done rather than on each keystroke, and there is no Test probe
//! nor placeholder chips; add them when the form is used often enough to miss them.

use agents::{AgentProfile, AgentSettings, Environment, OutputFormat, environment_choices, output_choices, same_environment_kind};
use rune::*;

use crate::screens::button;
use crate::settings::{caption, choice, color, toggle};
use crate::types::Actions;
use crate::ui::{UiState, text, ts};

const NOTE: &str = "Agents get the message, its sender and up to 20 earlier messages. Nothing is sent to Teams without you.";
const DEFAULT_DISTRO: &str = "Ubuntu";
const AGENT_FIELDS: [&str; 5] = ["Name", "Distro name", "Command", "Open session", "MCP config flag"];
const ACTION_FIELDS: [&str; 4] = ["Label", "Working dir", "Prompt", "Extra args"];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Agent(String),
    Action(String),
}

/// The open agent or action, edited here and written back on Done.
#[derive(Default)]
pub struct Form {
    target: Option<Target>,
    fields: [TextEdit; 5],
    environment: Environment,
    output: OutputFormat,
    agent_id: String,
    ask_extra: bool,
}

fn open_agent<A: Actions>(s: &mut UiState<A>, id: &str) {
    let Some(agent) = s.agent.settings.agents.iter().find(|agent| agent.id == id) else { return };
    let distro = match &agent.environment { Environment::Wsl { distro: Some(distro) } => distro.as_str(), _ => "" };
    let values = [agent.name.as_str(), distro, &agent.command, &agent.open_session_command, &agent.mcp_config_flag];
    s.settings.agent_form = Form {
        target: Some(Target::Agent(agent.id.clone())),
        fields: values.map(TextEdit::new),
        environment: agent.environment.clone(),
        output: agent.output,
        ..Form::default()
    };
}

fn open_action<A: Actions>(s: &mut UiState<A>, id: &str) {
    let Some(action) = s.agent.settings.actions.iter().find(|action| action.id == id) else { return };
    let values = [action.label.as_str(), &action.working_dir, &action.prompt_template, &action.extra_args, ""];
    s.settings.agent_form = Form {
        target: Some(Target::Action(action.id.clone())),
        fields: values.map(TextEdit::new),
        agent_id: action.agent_id.clone(),
        ask_extra: action.ask_extra_instructions,
        ..Form::default()
    };
}

fn save<A: Actions>(s: &mut UiState<A>, edit: impl FnOnce(&mut AgentSettings)) {
    edit(&mut s.agent.settings);
    s.actions.save_agent_settings(s.agent.settings.clone());
}

/// Writes the form back into its agent or action and closes it.
fn done<A: Actions>(s: &mut UiState<A>) {
    let form = std::mem::take(&mut s.settings.agent_form);
    let field = |index: usize| form.fields[index].text.clone();
    match form.target {
        Some(Target::Agent(id)) => save(s, |settings| {
            let Some(agent) = settings.agents.iter_mut().find(|agent| agent.id == id) else { return };
            agent.name = field(0);
            agent.command = field(2);
            agent.open_session_command = field(3);
            agent.mcp_config_flag = field(4).trim().to_owned();
            agent.output = form.output;
            agent.environment = match form.environment {
                Environment::Wsl { distro: Some(_) } => {
                    let distro = field(1).trim().to_owned();
                    Environment::Wsl { distro: Some(if distro.is_empty() { DEFAULT_DISTRO.to_owned() } else { distro }) }
                }
                other => other,
            };
        }),
        Some(Target::Action(id)) => save(s, |settings| {
            let Some(action) = settings.actions.iter_mut().find(|action| action.id == id) else { return };
            action.label = field(0);
            action.working_dir = field(1).trim().to_owned();
            action.prompt_template = field(2);
            action.extra_args = field(3);
            action.agent_id = form.agent_id;
            action.ask_extra_instructions = form.ask_extra;
        }),
        None => {}
    }
}

fn delete<A: Actions>(s: &mut UiState<A>) {
    let form = std::mem::take(&mut s.settings.agent_form);
    match form.target {
        Some(Target::Agent(id)) => save(s, |settings| settings.agents.retain(|agent| agent.id != id)),
        Some(Target::Action(id)) => save(s, |settings| settings.actions.retain(|action| action.id != id)),
        None => {}
    }
}

fn field<A: Actions + 'static>(s: &UiState<A>, label: &str, index: usize) -> El {
    let input = text_input(&s.settings.agent_form.fields[index], ts(14.0, 400.0))
        .key(format!("agent-field-{index}")).label(label.to_owned()).color(color("#242424")).w_full()
        .px(10.0).py(6.0).radius(6.0).border(1.0, color("#E0E0E0"))
        .on_edit(move |s: &mut UiState<A>| &mut s.settings.agent_form.fields[index]);
    col().w_full().gap(4.0).px(12.0).pt(8.0)
        .child(text(label.to_owned()).font(ts(13.0, 600.0)).color(color("#242424")))
        .child(input)
}

fn form_buttons<A: Actions + 'static>() -> El {
    row().gap(8.0).px(12.0).pt(16.0)
        .child(button::<A>("Done", "agent-form-done", |s, _| done(s)))
        .child(button::<A>("Delete", "agent-form-delete", |s, _| delete(s)))
}

fn agent_form<A: Actions + 'static>(s: &UiState<A>) -> El {
    let form = &s.settings.agent_form;
    let mut pane = col().w_full().child(field(s, AGENT_FIELDS[0], 0)).child(caption("RUNS IN"));
    for (label, environment) in environment_choices() {
        let selected = same_environment_kind(&environment, &form.environment);
        pane = pane.child(choice(format!("agent-environment-{label}"), label, selected, move |s: &mut UiState<A>| {
            s.settings.agent_form.environment = environment.clone();
        }));
    }
    if matches!(form.environment, Environment::Wsl { distro: Some(_) }) {
        pane = pane.child(field(s, AGENT_FIELDS[1], 1));
    }
    pane = pane.child(field(s, AGENT_FIELDS[2], 2)).child(caption("OUTPUT"));
    for (label, output) in output_choices() {
        pane = pane.child(choice(format!("agent-output-{output:?}"), label, form.output == output, move |s: &mut UiState<A>| {
            s.settings.agent_form.output = output;
        }));
    }
    pane.child(field(s, AGENT_FIELDS[4], 4)).child(field(s, AGENT_FIELDS[3], 3)).child(form_buttons::<A>())
}

fn action_form<A: Actions + 'static>(s: &UiState<A>) -> El {
    let form = &s.settings.agent_form;
    let mut pane = col().w_full().child(field(s, ACTION_FIELDS[0], 0)).child(caption("AGENT"));
    for agent in &s.agent.settings.agents {
        let id = agent.id.clone();
        pane = pane.child(choice(format!("action-agent-{id}"), &agent.name, form.agent_id == id, move |s: &mut UiState<A>| {
            s.settings.agent_form.agent_id = id.clone();
        }));
    }
    let ask = form.ask_extra;
    pane.child(field(s, ACTION_FIELDS[1], 1))
        .child(field(s, ACTION_FIELDS[2], 2))
        .child(toggle("action-ask-extra", "Ask for extra instructions", ask, true, move |s: &mut UiState<A>| s.settings.agent_form.ask_extra = !ask))
        .child(field(s, ACTION_FIELDS[3], 3))
        .child(form_buttons::<A>())
}

fn item<A: Actions + 'static>(key: String, label: &str, press: impl Fn(&mut UiState<A>) + 'static) -> El {
    row().key(key).w_full().h(36.0).px(12.0).center_y().radius(6.0).hover(color("#EBEBEB")).on_press(press)
        .child(text(label.to_owned()).font(ts(14.0, 400.0)).color(color("#242424")).truncate().grow(1.0).min_w(0.0))
}

pub fn pane<A: Actions + 'static>(s: &UiState<A>) -> El {
    match &s.settings.agent_form.target {
        Some(Target::Agent(_)) => return agent_form(s),
        Some(Target::Action(_)) => return action_form(s),
        None => {}
    }
    let settings = &s.agent.settings;
    let resume = settings.resume_interrupted_runs;
    let mut pane = col().w_full()
        .child(text(NOTE).font(ts(13.0, 400.0)).color(color("#616161")).wrap().px(12.0))
        .child(toggle("agents-resume", "Resume interrupted runs", resume, true, move |s: &mut UiState<A>| save(s, |settings| settings.resume_interrupted_runs = !resume)))
        .child(caption("AGENTS"));
    for agent in &settings.agents {
        let id = agent.id.clone();
        pane = pane.child(item(format!("agent-row-{id}"), &agent.name, move |s: &mut UiState<A>| open_agent(s, &id)));
    }
    let mut presets = row().wrap().gap(6.0).px(12.0).pt(4.0);
    for (label, preset) in AgentProfile::presets() {
        presets = presets.child(button::<A>(&format!("+ {label}"), &format!("agent-add-{}", preset.id), move |s, _| {
            let mut id = String::new();
            save(s, |settings| id = settings.add_agent(preset.clone()));
            open_agent(s, &id);
        }));
    }
    pane = pane.child(presets).child(caption("ACTIONS"));
    for (index, action) in settings.actions.iter().enumerate() {
        let id = action.id.clone();
        pane = pane.child(row().w_full().gap(4.0).center_y()
            .child(item(format!("action-row-{id}"), &action.label, move |s: &mut UiState<A>| open_action(s, &id)).grow(1.0).min_w(0.0))
            .child(button::<A>("Up", &format!("action-up-{index}"), move |s, _| save(s, |settings| { agents::move_item(&mut settings.actions, index, true); })))
            .child(button::<A>("Down", &format!("action-down-{index}"), move |s, _| save(s, |settings| { agents::move_item(&mut settings.actions, index, false); }))));
    }
    pane = pane.child(row().px(12.0).pt(4.0).child(button::<A>("+ Add action", "action-add", |s, _| {
        let mut id = String::new();
        save(s, |settings| id = settings.add_action());
        open_action(s, &id);
    })));
    for problem in settings.validate() {
        pane = pane.child(text(problem.to_string()).font(ts(13.0, 400.0)).color(color("#C4314B")).wrap().px(12.0).pt(4.0));
    }
    pane
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake(RefCell<Vec<AgentSettings>>);
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn save_agent_settings(&self, settings: AgentSettings) { self.0.borrow_mut().push(settings); }
    }

    #[test]
    fn an_added_agent_is_edited_saved_and_used_by_a_new_action() {
        let mut state = UiState::new(Fake::default());
        state.screens.screen = crate::screens::Screen::Settings;
        state.settings.section = crate::settings::Section::Agents;
        let mut rt = Runtime::new(state, crate::ui::view::<Fake>, (1200.0, 1600.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(1.0);
        let click = |rt: &mut Runtime<UiState<Fake>>, key: &str| {
            let (x, y) = rt.find(key).unwrap_or_else(|| panic!("{key} is shown"));
            rt.pointer_down(x, y); rt.pointer_up(x, y); rt.frame(1.0 / 60.0); rt.frame(1.0);
        };
        click(&mut rt, "agent-add-custom");
        assert!(rt.find("agent-field-0").is_some(), "the new agent opens");
        rt.update(|s, _| {
            s.settings.agent_form.fields[0] = TextEdit::new("Local agent");
            s.settings.agent_form.fields[2] = TextEdit::new("my-agent --print");
        });
        rt.frame(1.0);
        click(&mut rt, "agent-environment-WSL - named distro");
        assert!(rt.find("agent-field-1").is_some(), "a named distro asks for its name");
        click(&mut rt, "agent-form-done");
        let saved = rt.state.actions.0.borrow().last().cloned().expect("saved");
        let agent = saved.agents.iter().find(|agent| agent.id == "custom").expect("custom agent");
        assert_eq!((agent.name.as_str(), agent.command.as_str()), ("Local agent", "my-agent --print"));
        assert_eq!(agent.environment, Environment::Wsl { distro: Some(DEFAULT_DISTRO.into()) });
        click(&mut rt, "action-add");
        click(&mut rt, "action-agent-custom");
        click(&mut rt, "agent-form-done");
        let saved = rt.state.actions.0.borrow().last().cloned().expect("saved");
        assert_eq!(saved.actions.last().map(|action| action.agent_id.as_str()), Some("custom"));
        click(&mut rt, "action-up-2");
        assert_eq!(rt.state.agent.settings.actions[1].id, "action");
    }
}
