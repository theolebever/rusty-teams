use session::{GRAPH, Request, Scope};

use crate::client::{Graph, ensure_graph_url};
use crate::error::{Error, Result};
use crate::models::Photo;
use crate::people::decode_binary;

const CHAT_PATH: &str = "/v1.0/chats/";
const CHANNEL_PATH: &str = "/v1.0/teams/";
const SHARES_PATH: &str = "/v1.0/shares/";
const SHARES_SCOPE: &str = "Files.ReadWrite.All";
const MAX_PUBLIC_IMAGE_BYTES: usize = 32 * 1024 * 1024;

fn allowed_public_image(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|port| port == 443)
        && url.host_str().is_some_and(|host| {
            host == "giphy.com"
                || host.ends_with(".giphy.com")
                || host == "statics.teams.cdn.office.net"
        })
}

impl Graph {
    /// Binary GET of a Graph hosted content URL (`.../hostedContents/{id}/$value`).
    pub async fn download_hosted_content(&self, url: &str) -> Result<Photo> {
        ensure_graph_url(url)?;
        let scope = hosted_content_scope(url).ok_or(Error::ForeignNextLink)?;
        let request = Request::binary_get(url);
        let answer = self
            .session()
            .batch(std::slice::from_ref(&request), &scope)
            .await?
            .remove(0);
        decode_binary(url, answer)
    }

    /// Anonymous native GET: WebView fetch is blocked by the Teams page's CDN/CORS policy.
    pub async fn download_external_image(&self, url: &str) -> Result<Photo> {
        let parsed = reqwest::Url::parse(url).map_err(|error| Error::Download(error.to_string()))?;
        if !allowed_public_image(&parsed) {
            // Bot partners can host icons elsewhere; retain the browser's network boundary there.
            let answer = self.session().send(Request::anonymous_binary_get(url, Vec::new()), &Scope::graph("Chat.Read")).await?;
            return decode_binary(url, answer);
        }
        let failed = |error: reqwest::Error| Error::Download(error.without_url().to_string());
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(failed)?;
        let mut answer = client
            .get(parsed)
            .send()
            .await
            .map_err(failed)?
            .error_for_status()
            .map_err(failed)?;
        if !answer.status().is_success() {
            return Err(Error::Download(
                "Public image redirects are not allowed".into(),
            ));
        }
        if answer
            .content_length()
            .is_some_and(|size| size > MAX_PUBLIC_IMAGE_BYTES as u64)
        {
            return Err(Error::Download("Public image exceeds 32 MiB".into()));
        }
        let content_type = answer
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_owned();
        let mut bytes = Vec::new();
        while let Some(chunk) = answer.chunk().await.map_err(failed)? {
            if chunk.len() > MAX_PUBLIC_IMAGE_BYTES - bytes.len() {
                return Err(Error::Download("Public image exceeds 32 MiB".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Photo {
            bytes,
            content_type,
        })
    }
}

pub fn hosted_content_scope(url: &str) -> Option<Scope> {
    let path = url.strip_prefix(GRAPH)?;
    if path.starts_with(CHAT_PATH) {
        Some(Scope::graph("Chat.Read"))
    } else if path.starts_with(CHANNEL_PATH) {
        Some(Scope::graph("ChannelMessage.Read.All"))
    } else if path.starts_with(SHARES_PATH) {
        Some(Scope::graph(SHARES_SCOPE))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_images_never_accept_foreign_hosts_credentials_or_insecure_urls() {
        for url in [
            "https://media3.giphy.com/media/a/giphy.gif",
            "https://statics.teams.cdn.office.net/sticker.gif",
        ] {
            assert!(allowed_public_image(&reqwest::Url::parse(url).unwrap()));
        }
        for url in [
            "http://media3.giphy.com/a.gif",
            "https://giphy.com.evil.example/a.gif",
            "https://evilgiphy.com/a.gif",
            "https://user:secret@media3.giphy.com/a.gif",
            "https://media3.giphy.com:8443/a.gif",
            "https://127.0.0.1/a.gif",
        ] {
            assert!(
                !allowed_public_image(&reqwest::Url::parse(url).unwrap()),
                "accepted {url}"
            );
        }
    }

    #[test]
    fn scope_follows_the_conversation_kind() {
        let chat =
            "https://graph.microsoft.com/v1.0/chats/19%3Aa/messages/1/hostedContents/2/$value";
        let channel = "https://graph.microsoft.com/v1.0/teams/t/channels/c/messages/1/hostedContents/2/$value";
        assert_eq!(hosted_content_scope(chat).unwrap().name, "Chat.Read");
        assert_eq!(
            hosted_content_scope(channel).unwrap().name,
            "ChannelMessage.Read.All"
        );
        let thumbnail =
            "https://graph.microsoft.com/v1.0/shares/u!aHR0cA/driveItem/thumbnails/0/large/content";
        assert_eq!(
            hosted_content_scope(thumbnail).unwrap().name,
            "Files.ReadWrite.All"
        );
        assert!(hosted_content_scope("https://graph.microsoft.com/v1.0/me").is_none());
    }
}
