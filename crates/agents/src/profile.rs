use serde::{Deserialize, Serialize};

const CLAUDE_COMMAND: &str =
    "claude -p --output-format stream-json --verbose --session-id {session_id}";
const COPILOT_COMMAND: &str = "copilot -p {prompt} --output-format json --session-id {session_id}";
pub(crate) const LEGACY_CLAUDE_WSL_OPEN_SESSION: &str =
    "wt.exe wsl.exe --cd {cwd} -e claude --resume {session_id}";
pub(crate) const LEGACY_COPILOT_WSL_OPEN_SESSION: &str =
    "wt.exe wsl.exe --cd {cwd} -e copilot --resume {session_id}";
const CLAUDE_WSL_OPEN_SESSION: &str =
    "wt.exe wsl.exe --cd {cwd} -e sh -lc \"exec claude --resume {session_id}\"";
const COPILOT_WSL_OPEN_SESSION: &str =
    "wt.exe wsl.exe --cd {cwd} -e sh -lc \"exec copilot --resume {session_id}\"";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Environment {
    #[default]
    Host,
    Wsl {
        distro: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    #[default]
    ClaudeStreamJson,
    CopilotJsonl,
    Text,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub environment: Environment,
    pub command: String,
    pub output: OutputFormat,
    pub open_session_command: String,
    pub mcp_config_flag: String,
}

impl AgentProfile {
    pub(crate) fn migrate_open_session_command(&mut self) {
        let replacement = match self.open_session_command.as_str() {
            LEGACY_CLAUDE_WSL_OPEN_SESSION => CLAUDE_WSL_OPEN_SESSION,
            LEGACY_COPILOT_WSL_OPEN_SESSION => COPILOT_WSL_OPEN_SESSION,
            _ => return,
        };
        self.open_session_command = replacement.to_string();
    }

    pub fn claude_wsl() -> Self {
        Self {
            id: "claude".to_string(),
            name: "Claude Code (WSL)".to_string(),
            environment: Environment::Wsl { distro: None },
            command: CLAUDE_COMMAND.to_string(),
            output: OutputFormat::ClaudeStreamJson,
            open_session_command: CLAUDE_WSL_OPEN_SESSION.to_string(),
            mcp_config_flag: "--mcp-config".to_string(),
        }
    }

    pub fn claude_host() -> Self {
        Self {
            id: "claude-host".to_string(),
            name: "Claude Code".to_string(),
            environment: Environment::Host,
            command: CLAUDE_COMMAND.to_string(),
            output: OutputFormat::ClaudeStreamJson,
            open_session_command: "wt.exe -d {cwd} claude --resume {session_id}".to_string(),
            mcp_config_flag: "--mcp-config".to_string(),
        }
    }

    pub fn copilot_wsl() -> Self {
        Self {
            id: "copilot".to_string(),
            name: "Copilot CLI (WSL)".to_string(),
            environment: Environment::Wsl { distro: None },
            command: COPILOT_COMMAND.to_string(),
            output: OutputFormat::CopilotJsonl,
            open_session_command: COPILOT_WSL_OPEN_SESSION.to_string(),
            mcp_config_flag: "--additional-mcp-config".to_string(),
        }
    }

    /// The agents the settings offer to add, with their menu labels.
    pub fn presets() -> [(&'static str, AgentProfile); 4] {
        let custom = AgentProfile {
            id: "custom".to_owned(),
            name: "Custom agent".to_owned(),
            environment: Environment::Host,
            command: String::new(),
            output: OutputFormat::Text,
            open_session_command: String::new(),
            mcp_config_flag: String::new(),
        };
        [
            ("Claude Code (WSL)", AgentProfile::claude_wsl()),
            ("Claude Code (Windows)", AgentProfile::claude_host()),
            ("Copilot CLI (WSL)", AgentProfile::copilot_wsl()),
            ("Custom", custom),
        ]
    }
}

/// The environments the settings offer, with their labels. A named WSL distro is
/// `Wsl { distro: Some(_) }` whatever the name.
pub fn environment_choices() -> [(&'static str, Environment); 3] {
    [
        ("Windows", Environment::Host),
        ("WSL - default distro", Environment::Wsl { distro: None }),
        (
            "WSL - named distro",
            Environment::Wsl {
                distro: Some(String::new()),
            },
        ),
    ]
}

fn environment_kind(environment: &Environment) -> u8 {
    match environment {
        Environment::Host => 0,
        Environment::Wsl { distro: None } => 1,
        Environment::Wsl { distro: Some(_) } => 2,
    }
}

/// Whether two environments are the same choice of `environment_choices`.
pub fn same_environment_kind(left: &Environment, right: &Environment) -> bool {
    environment_kind(left) == environment_kind(right)
}

/// The output formats the settings offer, with their labels.
pub fn output_choices() -> [(&'static str, OutputFormat); 3] {
    [
        (
            "Claude stream-json - live steps",
            OutputFormat::ClaudeStreamJson,
        ),
        ("Copilot JSON - live steps", OutputFormat::CopilotJsonl),
        ("Plain text - time and last line", OutputFormat::Text),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environments_compare_by_kind_not_distro_name() {
        let named = |name: &str| Environment::Wsl {
            distro: Some(name.to_owned()),
        };
        assert!(same_environment_kind(&named("Ubuntu"), &named("Debian")));
        assert!(!same_environment_kind(
            &named("Ubuntu"),
            &Environment::Wsl { distro: None }
        ));
        assert!(!same_environment_kind(&Environment::Host, &named("Ubuntu")));
        assert!(same_environment_kind(
            &Environment::Host,
            &Environment::Host
        ));
    }
}
