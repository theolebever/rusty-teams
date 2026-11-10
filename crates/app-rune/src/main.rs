//! Rune shell next to the GPUI app: one window, one label, real backend crates linked.

use rune::*;

struct State {
    label: String,
}

impl State {
    fn new() -> State {
        State { label: label() }
    }
}

/// Runs a few calls through store, teams-core, app-state and session so the binary links them.
fn label() -> String {
    let store = store::Store::open_in_memory().expect("in-memory store");
    let ids = teams_core::default_reactions(&store);
    let glyphs = state::default_reactions::quick_reaction_glyphs(&ids, teams_core::skin_tone(&store));
    let login = session::is_login_url("https://login.microsoftonline.com/");
    format!("Rusty Teams on Rune {} (login url check: {login})", glyphs.concat())
}

fn view(s: &State, _cx: &mut Cx<State>) -> El {
    col()
        .key("root")
        .w_full()
        .h_full()
        .justify_center()
        .items_center()
        .child(text(s.label.clone()).key("label"))
}

fn main() {
    App::new(AppSpec::new("Rusty Teams", (800.0, 600.0), State::new, view)).run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    #[test]
    fn headless_frame_shows_the_label() {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(State::new(), view, (800.0, 600.0), env);
        rt.frame(0.0);
        assert!(rt.find("label").is_some());
        assert!(rt.state.label.starts_with("Rusty Teams on Rune "));
        assert!(rt.state.label.ends_with("(login url check: true)"));
    }
}
