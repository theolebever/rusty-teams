use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum App {
    Teams,
}

impl App {
    pub const ALL: [App; 1] = [App::Teams];

    pub fn origins(self) -> &'static [&'static str] {
        match self {
            App::Teams => &[
                "https://teams.cloud.microsoft",
                "https://teams.microsoft.com",
            ],
        }
    }

    pub fn start_url(self) -> &'static str {
        match self {
            App::Teams => "https://teams.cloud.microsoft/",
        }
    }

    /// A static page on the app origin: keeps the MSAL cache reachable without running the app.
    pub fn park_url(self) -> &'static str {
        match self {
            App::Teams => "https://teams.cloud.microsoft/robots.txt",
        }
    }

    pub fn is_park_url(self, url: &str) -> bool {
        url.starts_with(self.park_url())
    }
}

impl std::fmt::Display for App {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            App::Teams => "teams",
        })
    }
}

const LOGIN_ORIGINS: [&str; 2] = [
    "https://login.microsoftonline.com",
    "https://login.live.com",
];

#[derive(Debug, Clone, Deserialize)]
pub struct Target {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, rename = "webSocketDebuggerUrl")]
    pub websocket_url: Option<String>,
}

pub fn find_app_tab(targets: &[Target], app: App) -> Option<&Target> {
    targets.iter().find(|target| {
        target.kind == "page"
            && target.websocket_url.is_some()
            && app
                .origins()
                .iter()
                .any(|origin| target.url.starts_with(origin))
    })
}

pub fn is_login_url(url: &str) -> bool {
    LOGIN_ORIGINS.iter().any(|origin| url.starts_with(origin))
}

// Form encoding writes spaces as `+`; a literal `+` is always `%2B`, so the replace only touches spaces.
pub fn sign_in_url(login_hint: &str) -> String {
    let hint = url::form_urlencoded::byte_serialize(login_hint.as_bytes()).collect::<String>();
    format!(
        "https://login.microsoftonline.com/organizations/oauth2/v2.0/authorize?client_id=5e3ce6c0-2b1f-4285-8d4b-75ee78787346&response_type=code&redirect_uri=https%3A%2F%2Fteams.cloud.microsoft%2Fv2&scope=openid%20profile%20offline_access&login_hint={}&prompt=login&response_mode=fragment",
        hint.replace('+', "%20")
    )
}

/// True when a navigation on the app origin carries an authorization `code` in its query or fragment.
pub fn returned_from_sign_in(app: App, url: &str) -> bool {
    app.origins().iter().any(|origin| url.starts_with(origin))
        && url
            .split(['?', '#'])
            .skip(1)
            .flat_map(|part| part.split('&'))
            .any(|pair| pair.starts_with("code="))
}

pub fn has_login_tab(targets: &[Target]) -> bool {
    targets
        .iter()
        .any(|target| target.kind == "page" && is_login_url(&target.url))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets() -> Vec<Target> {
        serde_json::from_str(
            r#"[
              {"type":"service_worker","url":"https://teams.cloud.microsoft/sw.js","webSocketDebuggerUrl":"ws://x/1"},
              {"type":"page","url":"https://outlook.cloud.microsoft/mail/","webSocketDebuggerUrl":"ws://x/2"},
              {"type":"page","url":"https://teams.cloud.microsoft/v2/","webSocketDebuggerUrl":"ws://x/3"},
              {"type":"page","url":"about:blank"}
            ]"#,
        )
        .unwrap()
    }

    #[test]
    fn finds_page_tabs_per_app() {
        let targets = targets();
        assert_eq!(
            find_app_tab(&targets, App::Teams)
                .unwrap()
                .websocket_url
                .as_deref(),
            Some("ws://x/3")
        );
    }

    #[test]
    fn ignores_non_page_targets_and_missing_tabs() {
        let only_worker = &targets()[..1];
        assert!(find_app_tab(only_worker, App::Teams).is_none());
    }

    #[test]
    fn parked_tabs_are_found_by_origin_and_recognised() {
        let parked: Vec<Target> = serde_json::from_str(
            r#"[{"type":"page","url":"https://teams.cloud.microsoft/robots.txt","webSocketDebuggerUrl":"ws://x/9"},
                {"type":"page","url":"https://outlook.cloud.microsoft/owa/favicon.ico","webSocketDebuggerUrl":"ws://x/8"}]"#,
        )
        .unwrap();
        let teams = find_app_tab(&parked, App::Teams).unwrap();
        assert!(App::Teams.is_park_url(&teams.url));
        assert!(!App::Teams.is_park_url("https://teams.cloud.microsoft/v2/"));
    }

    #[test]
    fn sign_in_url_encodes_the_hint() {
        let url = sign_in_url("a+b@x.fr");
        assert!(url.starts_with("https://login.microsoftonline.com/"));
        assert!(url.contains("login_hint=a%2Bb%40x.fr&"));
        assert!(url.contains("prompt=login"));
    }

    #[test]
    fn returning_with_a_code_is_detected_on_the_app_origin_only() {
        assert!(returned_from_sign_in(
            App::Teams,
            "https://teams.cloud.microsoft/v2/#code=0.AAA&state=x"
        ));
        assert!(returned_from_sign_in(
            App::Teams,
            "https://teams.cloud.microsoft/v2/?code=abc"
        ));
        assert!(!returned_from_sign_in(
            App::Teams,
            "https://teams.cloud.microsoft/v2/#xcode=1"
        ));
        assert!(!returned_from_sign_in(
            App::Teams,
            "https://teams.cloud.microsoft/v2/"
        ));
        assert!(!returned_from_sign_in(
            App::Teams,
            "https://login.microsoftonline.com/?code=1"
        ));
    }

    #[test]
    fn detects_login_tab() {
        let mut targets = targets();
        assert!(!has_login_tab(&targets));
        targets.push(Target {
            kind: "page".into(),
            url: "https://login.microsoftonline.com/common/oauth2".into(),
            websocket_url: None,
        });
        assert!(has_login_tab(&targets));
    }
}
