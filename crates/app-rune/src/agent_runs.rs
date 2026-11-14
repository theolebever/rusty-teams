//! Durable Rune runs; the GPUI history is kept in its own schema.
use crate::{bridge::Shared, types::Update};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, PoisonError};

const KEY: &str = "rune.agent.runs";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    session: String,
    agent: String,
    name: String,
    action: agents::AgentAction,
    prompt: String,
    status: String,
    text: String,
}

fn load(store: &store::Store) -> Result<Vec<Run>, String> {
    store
        .meta(KEY)
        .map_err(|error| error.to_string())?
        .map_or(Ok(Vec::new()), |json| {
            serde_json::from_str(&json)
                .map_err(|error| format!("Agent history is unreadable: {error}"))
        })
}

fn save(store: &store::Store, run: &Run) -> Result<(), String> {
    let mut runs = load(store)?;
    if let Some(previous) = runs
        .iter_mut()
        .find(|previous| previous.session == run.session)
    {
        *previous = run.clone();
    } else {
        runs.push(run.clone());
    }
    store
        .set_meta(
            KEY,
            &serde_json::to_string(&runs).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())
}

pub fn history(store: &store::Store) -> Result<Vec<crate::agent_panel::Run>, String> {
    Ok(load(store)?
        .into_iter()
        .rev()
        .map(|run| crate::agent_panel::Run {
            title: format!("{} · {}", run.name, run.prompt),
            status: run.status,
            text: run.text,
        })
        .collect())
}

pub fn start(
    shared: Arc<Shared>,
    id: &str,
    prompt: &str,
    working_dir: &str,
    action_id: Option<(&str, &str)>,
) {
    if shared.read_only || prompt.trim().is_empty() {
        return;
    }
    let result = (|| {
        let settings = shared
            .store
            .meta(agents::SETTINGS_KEY)
            .map_err(|error| error.to_string())?
            .map(|json| agents::AgentSettings::from_json(&json).map_err(|error| error.to_string()))
            .transpose()?
            .unwrap_or_default();
        let mut action = match action_id {
            Some((id, _)) => settings
                .actions
                .iter()
                .find(|action| action.id == id)
                .cloned()
                .ok_or("Action is no longer configured")?,
            None => agents::AgentAction {
                agent_id: id.into(),
                ..Default::default()
            },
        };
        let profile = settings
            .agents
            .iter()
            .find(|profile| profile.id == action.agent_id)
            .ok_or("Agent is no longer configured")?;
        if !working_dir.is_empty() {
            action.working_dir = working_dir.into();
        }
        if action.working_dir.is_empty() {
            action.working_dir = "~".into();
        }
        Ok::<_, String>(Run {
            session: action_id
                .map_or_else(agents::new_session_id, |(_, session)| session.to_owned()),
            agent: profile.id.clone(),
            name: profile.name.clone(),
            action,
            prompt: prompt.into(),
            status: "Running".into(),
            text: String::new(),
        })
    })();
    match result {
        Ok(run) => launch(shared, run, false),
        Err(error) => shared.publish(Update::AgentResult(error)),
    }
}

fn launch(shared: Arc<Shared>, mut run: Run, resume: bool) {
    let result = (|| {
        let mut settings = shared
            .store
            .meta(agents::SETTINGS_KEY)
            .map_err(|error| error.to_string())?
            .map(|json| agents::AgentSettings::from_json(&json).map_err(|error| error.to_string()))
            .transpose()?
            .unwrap_or_default();
        let profile = settings
            .agents
            .iter()
            .find(|profile| profile.id == run.agent)
            .cloned()
            .ok_or("Agent is no longer configured")?;
        let invocation = if resume {
            agents::resume_invocation
        } else {
            agents::build_invocation
        };
        let plan = invocation(&profile, &run.action, &run.prompt, &run.session, None, None)
            .map_err(|error| error.to_string())?;
        settings.remember_working_dir(&run.agent, &run.action.working_dir);
        shared
            .store
            .set_meta(agents::SETTINGS_KEY, &settings.to_json())
            .map_err(|error| error.to_string())?;
        shared.publish(Update::AgentSettings(settings));
        Ok::<_, String>((plan, profile.output))
    })();
    let (plan, output) = match result {
        Ok(value) => value,
        Err(error) => {
            run.status = "Failed".into();
            run.text = error.clone();
            if let Err(save_error) = save(&shared.store, &run) {
                shared.publish(Update::Status(save_error));
            }
            shared.publish(Update::AgentResult(error));
            return;
        }
    };
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let mut running = shared
        .agent_stop
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if running.is_some() {
        shared.publish(Update::AgentResult("An agent is already running".into()));
        return;
    }
    if let Err(error) = save(&shared.store, &run) {
        shared.publish(Update::AgentResult(error));
        return;
    }
    *running = Some(stop);
    drop(running);
    app_backend::runtime::handle().spawn(async move {
        let (events, mut updates) = tokio::sync::mpsc::unbounded_channel();
        let progress = shared.clone();
        let drain = tokio::spawn(async move {
            while let Some(update) = updates.recv().await {
                if let agents::RunUpdate::Event(agents::RunEvent::Step(step)) = update {
                    progress.publish(Update::AgentProgress(step));
                }
            }
        });
        let outcome = agents::run(plan, output, events, stopped).await;
        let _ = drain.await;
        (run.status, run.text) = match outcome.status {
            agents::RunStatus::Completed { final_text } => ("Done".into(), final_text),
            agents::RunStatus::Failed { message, .. } => ("Failed".into(), message),
            agents::RunStatus::Stopped => ("Stopped".into(), "Stopped".into()),
        };
        if let Err(error) = save(&shared.store, &run) {
            shared.publish(Update::Status(format!("Agent history not saved: {error}")));
        }
        shared
            .agent_stop
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        shared.publish(Update::AgentResult(run.text));
        match history(&shared.store) {
            Ok(mut runs) => {
                runs.extend(crate::agent_panel::stored_runs(&shared.store));
                shared.publish(Update::AgentRuns(runs));
            }
            Err(error) => shared.publish(Update::Status(error)),
        }
    });
}

pub fn resume(shared: Arc<Shared>) {
    if shared.read_only {
        return;
    }
    let result = load(&shared.store);
    let runs = match result {
        Ok(runs) => runs,
        Err(error) => {
            shared.publish(Update::Status(error));
            return;
        }
    };
    let settings = shared
        .store
        .meta(agents::SETTINGS_KEY)
        .ok()
        .flatten()
        .and_then(|json| agents::AgentSettings::from_json(&json).ok())
        .unwrap_or_default();
    for mut run in runs.into_iter().filter(|run| run.status == "Running") {
        if settings.resume_interrupted_runs
            && settings
                .agents
                .iter()
                .find(|profile| profile.id == run.agent)
                .is_some_and(agents::supports_resume)
        {
            shared.publish(Update::AgentProgress(format!("Resuming {}", run.name)));
            launch(shared.clone(), run, true);
        } else {
            run.status = "Interrupted".into();
            run.text =
                "The app closed during this run; automatic resume is disabled or unsupported."
                    .into();
            if let Err(error) = save(&shared.store, &run) {
                shared.publish(Update::Status(error));
            }
        }
    }
    match history(&shared.store) {
        Ok(mut runs) => {
            runs.extend(crate::agent_panel::stored_runs(&shared.store));
            shared.publish(Update::AgentRuns(runs));
        }
        Err(error) => shared.publish(Update::Status(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_survives_reopening_and_completion_updates_the_same_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.db");
        let store = store::Store::open(&path).unwrap();
        let mut run = Run {
            session: "session".into(),
            agent: "agent".into(),
            name: "Agent".into(),
            action: Default::default(),
            prompt: "Explain".into(),
            status: "Running".into(),
            text: String::new(),
        };
        save(&store, &run).unwrap();
        drop(store);
        let store = store::Store::open(&path).unwrap();
        assert_eq!(load(&store).unwrap()[0].session, "session");
        run.status = "Done".into();
        run.text = "Answer".into();
        save(&store, &run).unwrap();
        let history = history(&store).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text, "Answer");
    }
}
