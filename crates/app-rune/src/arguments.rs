//! Command-line options shared by startup and the backend bridge.

use std::path::PathBuf;

pub const HELP: &str = "Rusty Teams (Rune)\n\nOptions:\n  --database <path>  Cache database; its directory also holds the WebView2 profile\n  --no-sso           Choose an account instead of using the Windows account (default)\n  --sso              Use the Windows account for single sign-on\n  --read-only        Disable account actions from the interface\n  --endpoint <url>   Use an existing browser's CDP endpoint\n  --demo             Use sample data without connecting to an account\n  --open <id>        Open a chat\n  --open-channel <id> Open a channel\n  --switcher         Open the chat switcher\n  --join <url>       Join a meeting or call link\n  --help             Show this help\n";

#[derive(Debug, Default, PartialEq)]
pub struct Arguments {
    pub database: Option<PathBuf>,
    pub no_sso: bool,
    pub read_only: bool,
    pub endpoint: Option<String>,
    pub demo: bool,
    pub help: bool,
    /// Started by the Windows Run key; may open hidden (`settings_backend::AUTOSTART_NAME`).
    pub autostart: bool,
    /// Run in the window, or in the already running instance (`Command::link`).
    pub command: Option<Command>,
}

/// A launch option the running instance carries out, as the GPUI client's launch options do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Open(String),
    OpenChannel(String),
    Switcher,
    Join(String),
}

/// Single-instance forwarding passes only links to the running instance, so a command travels
/// as one. Ids and URLs go unescaped: the pipe hands the argument over byte for byte.
const COMMAND_PREFIX: &str = "rusty-teams-rune://command/";

impl Command {
    pub fn link(&self) -> String {
        let (verb, value) = match self {
            Self::Open(id) => ("open", id.as_str()),
            Self::OpenChannel(id) => ("open-channel", id.as_str()),
            Self::Switcher => ("switcher", ""),
            Self::Join(url) => ("join", url.as_str()),
        };
        format!("{COMMAND_PREFIX}{verb}/{value}")
    }

    pub fn from_link(link: &str) -> Option<Self> {
        let (verb, value) = link.strip_prefix(COMMAND_PREFIX)?.split_once('/')?;
        let value = (!value.is_empty()).then(|| value.to_owned());
        match verb {
            "open" => value.map(Self::Open),
            "open-channel" => value.map(Self::OpenChannel),
            "switcher" => Some(Self::Switcher),
            "join" => value.map(Self::Join),
            _ => None,
        }
    }

    pub fn run<A: crate::types::Actions>(self, s: &mut crate::ui::UiState<A>, tx: &mut rune::Tx) {
        use crate::screens::{navigate, Screen};
        match self {
            Self::Open(id) => {
                navigate(s, Screen::Chat);
                crate::ui::open_chat(s, &id, tx);
            }
            Self::OpenChannel(id) => {
                navigate(s, Screen::Teams);
                crate::ui::open_chat(s, &id, tx);
            }
            Self::Switcher => crate::switcher::open(s, tx),
            Self::Join(url) => {
                if !s.read_only { s.actions.join_call(&url, false); }
            }
        }
    }
}

impl Arguments {
    pub fn parse(input: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut parsed = Self { no_sso: true, ..Self::default() };
        let mut input = input.into_iter();
        while let Some(flag) = input.next() {
            match flag.as_str() {
                // Links (notification clicks, forwarded commands) go to `App::on_open_url`.
                url if url.starts_with(crate::notifications::ACTIVATION_PREFIX) || url.starts_with(COMMAND_PREFIX) => {},
                "--switcher" => parsed.command = Some(Command::Switcher),
                "--no-sso" => parsed.no_sso = true,
                "--sso" => parsed.no_sso = false,
                "--read-only" => parsed.read_only = true,
                "--demo" => parsed.demo = true,
                app_backend::autostart::AUTOSTART_FLAG => parsed.autostart = true,
                "--help" | "-h" => parsed.help = true,
                "--database" | "--endpoint" | "--open" | "--open-channel" | "--join" => {
                    let value = input
                        .next()
                        .filter(|value| !value.is_empty() && !value.starts_with("--"))
                        .ok_or_else(|| format!("{flag} requires a value"))?;
                    match flag.as_str() {
                        "--database" => parsed.database = Some(PathBuf::from(value)),
                        "--endpoint" => parsed.endpoint = Some(value),
                        "--open" => parsed.command = Some(Command::Open(value)),
                        "--open-channel" => parsed.command = Some(Command::OpenChannel(value)),
                        _ => parsed.command = Some(Command::Join(value)),
                    }
                }
                _ => return Err(format!("Unknown option: {flag}")),
            }
        }
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> Result<Arguments, String> {
        Arguments::parse(values.iter().map(|value| (*value).to_owned()))
    }

    #[test]
    fn notification_activation_is_left_for_runes_url_handler() {
        assert_eq!(parse(&["rusty-teams-rune://notification/message?conversation_id=chat"]).unwrap(), parse(&[]).unwrap());
        assert!(parse(&["unknown://notification/message"]).is_err());
    }

    #[test]
    fn launch_commands_round_trip_through_a_link() {
        let open = parse(&["--open", "19:abc@thread.v2"]).unwrap().command.unwrap();
        assert_eq!(open, Command::Open("19:abc@thread.v2".into()));
        let join = Command::Join("https://teams.microsoft.com/l/meetup-join/x?context=%7B%7D".into());
        for command in [open, join, Command::Switcher, Command::OpenChannel("19:c@thread.tacv2".into())] {
            assert_eq!(Command::from_link(&command.link()), Some(command.clone()));
            assert_eq!(parse(&[&command.link()]).unwrap(), parse(&[]).unwrap(), "a forwarded link is left for on_open_url");
        }
        assert_eq!(Command::from_link("rusty-teams-rune://command/open/"), None);
        assert!(parse(&["--join"]).is_err());
    }

    #[test]
    fn account_choice_is_the_default_and_sso_is_explicit() {
        assert!(parse(&[]).unwrap().no_sso);
        assert!(parse(&["--database", "other/cache.db"]).unwrap().no_sso);
        assert!(!parse(&["--sso"]).unwrap().no_sso);
        assert!(parse(&["--sso", "--no-sso"]).unwrap().no_sso);
        assert!(!parse(&["--no-sso", "--sso"]).unwrap().no_sso);
    }

    #[test]
    fn account_options_reach_startup() {
        let args = parse(&[
            "--database", "other account/cache.db", "--no-sso", "--read-only",
            "--endpoint", "http://127.0.0.1:9222",
        ]).unwrap();
        assert_eq!(args.database, Some(PathBuf::from("other account/cache.db")));
        assert!(args.no_sso && args.read_only);
        assert!(parse(&["--autostart"]).unwrap().autostart);
        assert_eq!(args.endpoint.as_deref(), Some("http://127.0.0.1:9222"));
    }

    #[test]
    fn invalid_options_cannot_silently_use_the_default_account() {
        for values in [&["--database"][..], &["--database", "--no-sso"], &["--endpoint", ""], &["--databse", "cache.db"]] {
            assert!(parse(values).is_err(), "accepted invalid arguments {values:?}");
        }
    }
}
