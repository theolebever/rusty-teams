//! What leaves the composer: the HTML sent on the wire and the draft stored per conversation.

use chatsvc::Gif;
use store::DraftRecord;
use teams_core::{Draft, DraftLine};

const GIF_ITEMTYPE: &str = "http://schema.skype.com/Giphy";
const GIF_MAX_HEIGHT: u32 = 250;
const PREVIEW_CHARS: usize = 200;

/// Teams HTML for the markdown-ish composer text (`**bold**`, `*italic*`, `~~strike~~`, `` `code` ``,
/// `- ` lists, and `<u>underline</u>`), followed by the chosen GIFs.
pub fn html(text: &str, gifs: &[Gif]) -> String {
    let mut html = if text.trim().is_empty() {
        String::new()
    } else {
        // The markdown reader has no underline marker; the typed tag is escaped, so turn it back.
        Draft::from_markdown(text).trimmed().to_html().replace("&lt;u&gt;", "<u>").replace("&lt;/u&gt;", "</u>")
    };
    for gif in gifs {
        html.push_str(&gif_html(gif));
    }
    html
}

fn gif_html(gif: &Gif) -> String {
    let (width, height) = if gif.height > GIF_MAX_HEIGHT {
        (gif.width * GIF_MAX_HEIGHT / gif.height, GIF_MAX_HEIGHT)
    } else {
        (gif.width, gif.height)
    };
    format!(
        "<img src=\"{}\" width=\"{width}\" height=\"{height}\" alt=\"{}\" itemtype=\"{GIF_ITEMTYPE}\">",
        teams_core::escape_html(&gif.url),
        teams_core::escape_html(&gif.title),
    )
}

/// A draft the GPUI app can read too: its payload with plain lines and nothing attached.
pub fn draft_record(conversation_id: &str, text: &str) -> Option<DraftRecord> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let payload = serde_json::json!({
        "lines": Draft::plain(text).to_lines(),
        "mentions": [], "reply": null, "files": [], "objects": [],
    });
    let preview: String = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(PREVIEW_CHARS).collect();
    Some(DraftRecord {
        conversation_id: conversation_id.to_owned(),
        payload: payload.to_string(),
        preview,
        images: Vec::new(),
        updated_at: chrono::Utc::now(),
    })
}

pub fn draft_text(payload: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    let lines: Vec<DraftLine> = serde_json::from_value(value.get("lines")?.clone()).ok()?;
    let text = Draft::from_lines(&lines).text().to_owned();
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gif(height: u32) -> Gif {
        Gif {
            url: "https://g.test/a.gif?x=1&y=2".into(),
            preview_url: String::new(),
            title: "wave".into(),
            width: 400,
            height,
            preview_width: 0,
            preview_height: 0,
        }
    }

    #[test]
    fn markers_become_html_tags_and_gifs_follow_the_text() {
        let out = html("**b** *i* ~~s~~ <u>u</u> `c`", &[gif(500)]);
        for tag in ["<b>b</b>", "<i>i</i>", "<s>s</s>", "<u>u</u>", "<code>c</code>"] {
            assert!(out.contains(tag), "{tag} missing from {out}");
        }
        assert!(out.ends_with("width=\"200\" height=\"250\" alt=\"wave\" itemtype=\"http://schema.skype.com/Giphy\">"), "{out}");
        assert!(out.contains("x=1&amp;y=2"));
        assert_eq!(html("  ", &[]), "");
    }

    #[test]
    fn a_draft_survives_the_store_payload() {
        let record = draft_record("c", "line one\nline two").expect("record");
        assert_eq!(record.preview, "line one line two");
        assert_eq!(draft_text(&record.payload).as_deref(), Some("line one\nline two"));
        assert!(draft_record("c", " \n").is_none());
    }
}
