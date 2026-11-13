//! Configured local agents, using the same invocation and runner as the GPUI application.
use rune::*;
use crate::ui::text;
use crate::types::Actions;
use crate::ui::{UiState, c, ts, INK, MUTED};

/// One finished run in the history list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Run {
    pub title: String,
    pub status: String,
    pub text: String,
}

#[derive(Default)]
pub struct State {
    pub settings: agents::AgentSettings,
    pub prompt: TextEdit,
    /// Empty runs in `~`, as GPUI's default actions do.
    pub working_dir: TextEdit,
    pub output: String,
    pub running: bool,
    /// The last `SHOWN_STEPS` tool steps of the current run, as the runner reports them.
    pub steps: Vec<String>,
    /// Newest first: this session's runs (no status), then the GPUI client's stored ones.
    pub runs: Vec<Run>,
    current: Run,
}

/// How many tool steps the panel lists.
const SHOWN_STEPS: usize = 5;

impl State {
    pub fn step(&mut self, step: String) {
        if self.steps.len() == SHOWN_STEPS { self.steps.remove(0); }
        self.steps.push(step);
    }

    pub fn finish(&mut self, result: String) {
        self.running = false;
        self.steps.clear();
        let mut run = std::mem::take(&mut self.current);
        run.text.clone_from(&result);
        self.output = result;
        if !run.title.is_empty() { self.runs.insert(0, run); }
    }
}

fn start<A: Actions>(s: &mut UiState<A>, id: &str, name: &str) {
    let prompt = s.agent.prompt.text.trim().to_owned();
    if prompt.is_empty() { return; }
    s.agent.running = true;
    s.agent.output = "Running…".into();
    s.agent.current = Run { title: format!("{name} · {prompt}"), ..Run::default() };
    s.actions.run_agent(id, &prompt, s.agent.working_dir.text.trim());
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut content = col().w_full().padding(20.0).gap(12.0)
        .child(text("Agents").font(ts(20.0, 600.0)).color(c(INK)))
        .child(auto_grow(text_input(&s.agent.prompt, ts(14.0, 400.0)).key("agent-prompt").label("Instructions for the agent")
            .placeholder("Describe what you want the agent to do", c(MUTED))
            .color(c(INK)).w_full().on_edit(|s: &mut UiState<A>| &mut s.agent.prompt), 2, 8)
            .w_full().min_h(64.0).padding(12.0).radius(8.0).fill(c(crate::ui::HOVER)))
        .child(text_input(&s.agent.working_dir, ts(14.0, 400.0)).key("agent-working-dir").label("Working folder")
            .placeholder("Working folder (~ when empty)", c(MUTED)).color(c(INK)).w_full()
            .on_edit(|s: &mut UiState<A>| &mut s.agent.working_dir).h(36.0).px(12.0).radius(8.0).fill(c(crate::ui::HOVER)));
    let mut recent: Vec<&String> = s.agent.settings.recent_working_dirs.values().flatten().collect();
    recent.sort();
    recent.dedup();
    if !recent.is_empty() {
        let mut chips = row().w_full().wrap().gap(6.0);
        for dir in recent {
            let pick = dir.clone();
            chips = chips.child(crate::screens::button(dir, &format!("agent-dir-{dir}"), move |s: &mut UiState<A>, _| s.agent.working_dir = TextEdit::new(&pick)));
        }
        content = content.child(chips);
    }
    if !s.read_only && !s.agent.running {
        for agent in &s.agent.settings.agents {
            let (id, name) = (agent.id.clone(), agent.name.clone());
            content = content.child(crate::screens::button(&format!("Run {name}"), &format!("agent-{id}"), move |s: &mut UiState<A>, _| start(s, &id, &name)));
        }
    }
    if s.agent.running {
        content = content.child(crate::screens::button("Stop", "agent-stop", |s: &mut UiState<A>, _| s.actions.stop_agent()));
        for step in &s.agent.steps {
            content = content.child(text(step.clone()).font(ts(13.0, 400.0)).color(c(MUTED)).truncate());
        }
    }
    if !s.agent.output.is_empty() {
        content = content.child(text(s.agent.output.clone()).key("agent-output").font(ts(14.0, 400.0)).color(c(INK)).wrap());
    }
    if !s.agent.runs.is_empty() {
        content = content.child(text("Agent runs").font(ts(16.0, 600.0)).color(c(INK)).pt(12.0));
        for (index, run) in s.agent.runs.iter().enumerate() {
            content = content.child(col().key(format!("agent-run-{index}")).w_full().gap(2.0).padding(10.0).radius(8.0).fill(c(crate::ui::HOVER))
                .child(text(if run.status.is_empty() { run.title.clone() } else { format!("{} · {}", run.title, run.status) }).font(ts(13.0, 600.0)).color(c(INK)).truncate())
                .child(text(run.text.clone()).font(ts(13.0, 400.0)).color(c(MUTED)).max_lines(3)));
        }
    }
    col().key("agents").grow(1.0).min_h(0.0).w_full().scroll(ScrollSpec { stick_bottom: false }).child(content)
}

/// The GPUI client's run history (`agents.runs`), newest first. Read only: its records carry
/// fields Rune has no source for, and an incomplete record would make GPUI drop the history.
pub fn stored_runs(store: &store::Store) -> Vec<Run> {
    let Some(json) = store.meta("agents.runs").ok().flatten() else { return Vec::new() };
    let Ok(serde_json::Value::Array(runs)) = serde_json::from_str(&json) else { return Vec::new() };
    let field = |run: &serde_json::Value, key: &str| run[key].as_str().unwrap_or_default().to_owned();
    runs.iter().rev().map(|run| Run {
        title: [field(run, "agent_name"), field(run, "action_label"), field(run, "chat_title")]
            .into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · "),
        status: match &run["state"] {
            serde_json::Value::String(state) => state.clone(),
            _ => "Failed".into(),
        },
        text: field(run, "final_text"),
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake { calls: RefCell<Vec<String>> }
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn run_agent(&self, id: &str, prompt: &str, working_dir: &str) { self.calls.borrow_mut().push(format!("{id} in {working_dir}: {prompt}")); }
        fn stop_agent(&self) { self.calls.borrow_mut().push("stop".into()); }
    }

    #[test]
    fn running_agents_require_a_prompt_show_progress_and_land_in_the_history() {
        let mut state = UiState::new(Fake::default());
        state.screens.screen = crate::screens::Screen::Agents;
        state.agent.settings.remember_working_dir("claude", "/repo");
        let mut rt = Runtime::new(state, crate::ui::view::<Fake>, (1000.0, 700.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        let click = |rt: &mut Runtime<UiState<Fake>>, key: &str| {
            let (x, y) = rt.find(key).expect("agent control");
            rt.pointer_down(x, y); rt.pointer_up(x, y); rt.frame(1.0);
        };
        click(&mut rt, "agent-claude");
        assert!(rt.state.actions.calls.borrow().is_empty());
        rt.update(|s, _| s.agent.prompt = TextEdit::new("Explain this"));
        rt.frame(1.0);
        click(&mut rt, "agent-dir-/repo");
        click(&mut rt, "agent-claude");
        assert!(rt.state.agent.running);
        assert!(rt.find("agent-claude").is_none());
        rt.update(|s, _| s.apply(crate::types::Update::AgentProgress("Read src/main.rs".into())));
        assert_eq!(rt.state.agent.steps, ["Read src/main.rs"]);
        click(&mut rt, "agent-stop");
        assert_eq!(*rt.state.actions.calls.borrow(), ["claude in /repo: Explain this", "stop"]);
        rt.update(|s, _| s.apply(crate::types::Update::AgentResult("Stopped".into())));
        rt.frame(1.0);
        assert!(!rt.state.agent.running && rt.state.agent.steps.is_empty());
        assert!(rt.find("agent-claude").is_some());
        assert_eq!(rt.state.agent.runs[0].text, "Stopped");
        assert!(rt.find("agent-run-0").is_some());
    }

    #[test]
    fn the_gpui_history_reads_newest_first() {
        let store = store::Store::open_in_memory().expect("in-memory store");
        store.set_meta("agents.runs", r#"[{"agent_name":"Claude","action_label":"Answer this","chat_title":"Ada","state":"Done","final_text":"ok"},{"agent_name":"Copilot","state":{"Failed":{"message":"x","log":[]}}}]"#).expect("meta");
        let runs = stored_runs(&store);
        assert_eq!(runs[0], Run { title: "Copilot".into(), status: "Failed".into(), text: String::new() });
        assert_eq!(runs[1].title, "Claude · Answer this · Ada");
    }
}
