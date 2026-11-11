//! Configured local agents, using the same invocation and runner as the GPUI application.
use rune::*;
use crate::ui::text;
use crate::types::Actions;
use crate::ui::{UiState, c, ts, INK};

#[derive(Default)]
pub struct State {
    pub settings: agents::AgentSettings,
    pub prompt: TextEdit,
    pub output: String,
    pub running: bool,
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    let mut content = col().w_full().padding(20.0).gap(12.0)
        .child(text("Agents").font(ts(20.0, 600.0)).color(c(INK)))
        .child(auto_grow(text_input(&s.agent.prompt, ts(14.0, 400.0)).key("agent-prompt").label("Instructions for the agent")
            .placeholder("Describe what you want the agent to do", c(crate::ui::MUTED))
            .color(c(INK)).w_full().on_edit(|s: &mut UiState<A>| &mut s.agent.prompt), 2, 8)
            .w_full().min_h(64.0).padding(12.0).radius(8.0).fill(c(crate::ui::HOVER)));
    if !s.read_only && !s.agent.running {
        for agent in &s.agent.settings.agents {
            let id = agent.id.clone();
            content = content.child(crate::screens::button(&format!("Run {}", agent.name), &format!("agent-{}", agent.id), move |s: &mut UiState<A>, _| {
                if !s.agent.prompt.text.trim().is_empty() {
                    s.agent.running = true;
                    s.agent.output = "Running…".into();
                    s.actions.run_agent(&id, &s.agent.prompt.text);
                }
            }));
        }
    }
    if s.agent.running {
        content = content.child(crate::screens::button("Stop", "agent-stop", |s: &mut UiState<A>, _| s.actions.stop_agent()));
    }
    if !s.agent.output.is_empty() {
        content = content.child(text(s.agent.output.clone()).key("agent-output").font(ts(14.0, 400.0)).color(c(INK)).wrap());
    }
    col().key("agents").grow(1.0).min_h(0.0).w_full().scroll(ScrollSpec { stick_bottom: false }).child(content)
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
        fn run_agent(&self, id: &str, prompt: &str) { self.calls.borrow_mut().push(format!("{id}: {prompt}")); }
        fn stop_agent(&self) { self.calls.borrow_mut().push("stop".into()); }
    }

    #[test]
    fn running_agents_require_a_prompt_and_can_be_stopped() {
        let mut state = UiState::new(Fake::default());
        state.screens.screen = crate::screens::Screen::Agents;
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
        click(&mut rt, "agent-claude");
        assert!(rt.state.agent.running);
        assert!(rt.find("agent-claude").is_none());
        click(&mut rt, "agent-stop");
        assert_eq!(*rt.state.actions.calls.borrow(), ["claude: Explain this", "stop"]);
        rt.update(|s, _| s.apply(crate::types::Update::AgentResult("Stopped".into())));
        rt.frame(1.0);
        assert!(!rt.state.agent.running);
        assert!(rt.find("agent-claude").is_some());
    }
}

