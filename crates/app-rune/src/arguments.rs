//! Command-line options shared by startup and the backend bridge.

use std::path::PathBuf;

pub const HELP: &str = "Rusty Teams (Rune)\n\nOptions:\n  --database <path>  Cache database; its directory also holds the WebView2 profile\n  --no-sso           Choose an account instead of using the Windows account (default)\n  --sso              Use the Windows account for single sign-on\n  --read-only        Disable account actions from the interface\n  --endpoint <url>   Use an existing browser's CDP endpoint\n  --demo             Use sample data without connecting to an account\n  --help             Show this help\n";

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
}

impl Arguments {
    pub fn parse(input: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut parsed = Self { no_sso: true, ..Self::default() };
        let mut input = input.into_iter();
        while let Some(flag) = input.next() {
            match flag.as_str() {
                url if url.starts_with(crate::notifications::ACTIVATION_PREFIX) => {},
                "--no-sso" => parsed.no_sso = true,
                "--sso" => parsed.no_sso = false,
                "--read-only" => parsed.read_only = true,
                "--demo" => parsed.demo = true,
                app_backend::autostart::AUTOSTART_FLAG => parsed.autostart = true,
                "--help" | "-h" => parsed.help = true,
                "--database" | "--endpoint" => {
                    let value = input
                        .next()
                        .filter(|value| !value.is_empty() && !value.starts_with("--"))
                        .ok_or_else(|| format!("{flag} requires a value"))?;
                    if flag == "--database" {
                        parsed.database = Some(PathBuf::from(value));
                    } else {
                        parsed.endpoint = Some(value);
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
