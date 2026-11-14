//! What leaves the composer: the HTML sent on the wire and the draft stored per conversation.

use chatsvc::Gif;
use store::DraftRecord;
use teams_core::{Draft, DraftLine};

const GIF_ITEMTYPE: &str = "http://schema.skype.com/Giphy";
const GIF_MAX_HEIGHT: u32 = 250;
const PREVIEW_CHARS: usize = 200;

pub fn composition_record(id: &str, composed: &crate::types::Composed, files: &[teams_core::FileReference]) -> Result<Option<DraftRecord>, String> {
    let outgoing = outgoing(composed, files);
    if outgoing.draft.is_blank() && !outgoing.has_attachments() && outgoing.subject.is_none() && outgoing.banner.is_none() { return Ok(None); }
    let stored = state::stored_outgoing::encode(&outgoing).ok_or_else(|| "Cannot encode draft".to_owned())?;
    let mut value: serde_json::Value = serde_json::from_str(&stored.payload).map_err(|e| e.to_string())?;
    let mut markers: Vec<_> = composed.inline.iter().flat_map(|(marker, _)| composed.text.match_indices(marker).map(move |(at, _)| (at, marker.clone()))).collect();
    markers.sort_by_key(|(at, _)| *at);
    value["rune_text"] = serde_json::Value::String(composed.text.clone());
    value["rune_markers"] = serde_json::json!(markers.into_iter().map(|(_, marker)| marker).collect::<Vec<_>>());
    Ok(Some(DraftRecord { conversation_id: id.into(), payload: value.to_string(), images: stored.images,
        preview: composed.text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(PREVIEW_CHARS).collect(), updated_at: chrono::Utc::now() }))
}

pub fn composition(record: &DraftRecord) -> Option<(crate::types::Composed, Vec<teams_core::FileReference>)> {
    use state::outgoing::OutgoingImage;
    let value: serde_json::Value = serde_json::from_str(&record.payload).ok()?;
    let outgoing = state::stored_outgoing::decode(&record.payload, &record.images)?;
    // Other clients' drafts have no Rune source text; their stored objects still need real markers.
    let raw = value.get("rune_text").and_then(|value| value.as_str());
    let markers: Vec<String> = value.get("rune_markers").cloned().and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default();
    let mut markers = markers.into_iter();
    let mut inline = Vec::new();
    let mut gifs = Vec::new();
    let mut replacements = Vec::new();
    for image in outgoing.images {
        match image {
            OutgoingImage::Inline(image) => {
                let marker = markers.next().unwrap_or_else(crate::inline_compose::marker);
                replacements.push(marker.clone());
                if !inline.iter().any(|(present, _)| present == &marker) { inline.push((marker, image)); }
            }
            OutgoingImage::Remote(image) => {
                replacements.push(String::new());
                gifs.push(Gif { preview_url: image.url.clone(), url: image.url, title: image.title, width: image.width, height: image.height, preview_width: image.width, preview_height: image.height });
            }
        }
    }
    let text = raw.map(str::to_owned).unwrap_or_else(|| {
        let mut objects = replacements.into_iter();
        outgoing.draft.text().split(teams_core::OBJECT_MARK).enumerate().fold(String::new(), |mut text, (index, piece)| {
            if index > 0 { text.push_str(&objects.next().unwrap_or_default()); }
            text.push_str(piece); text
        })
    });
    Some((crate::types::Composed { text, inline, gifs, mentions: outgoing.mentions, importance: outgoing.importance, subject: outgoing.subject, banner: outgoing.banner,
        reply: outgoing.reply.map(|reply| crate::types::ReplyReference { message_id: reply.message_id, sender: reply.author, text: reply.excerpt }) }, outgoing.files.into_iter().map(|file| file.reference).collect()))
}

pub fn outgoing(composed: &crate::types::Composed, files: &[teams_core::FileReference]) -> state::outgoing::Outgoing {
    use state::outgoing::{Outgoing, OutgoingFile, OutgoingImage, ReplyPreview};
    use state::remote_image::{RemoteImage, RemoteKind};
    let mut positions: Vec<_> = composed.inline.iter().flat_map(|(marker, image)| composed.text.match_indices(marker).map(move |(at, _)| (at, marker.len(), image.clone()))).collect();
    positions.sort_by_key(|(at, _, _)| *at);
    let mut source = String::new();
    let mut images = Vec::new();
    let mut cursor = 0;
    for (at, length, image) in positions {
        if at < cursor { continue; }
        source.push_str(&composed.text[cursor..at]);
        source.push(teams_core::OBJECT_MARK);
        images.push(OutgoingImage::Inline(image));
        cursor = at + length;
    }
    source.push_str(&composed.text[cursor..]);
    let mut body = html(&source, &[]);
    body.extend(std::iter::repeat_n(teams_core::OBJECT_MARK, composed.gifs.len()));
    Outgoing {
        draft: Draft::from_spans(&teams_core::html_to_spans(&body)),
        subject: composed.subject.clone(),
        mentions: composed.mentions.clone(),
        reply: composed.reply.as_ref().map(|reply| ReplyPreview { message_id: reply.message_id.clone(), author: reply.sender.clone(), excerpt: reply.text.clone() }),
        edit: None,
        images: images.into_iter().chain(composed.gifs.iter().map(|gif| OutgoingImage::Remote(RemoteImage {
            kind: RemoteKind::Gif, url: gif.url.clone(), title: gif.title.clone(), width: gif.width, height: gif.height,
        }))).collect(),
        files: files.iter().map(|file| OutgoingFile { name: file.name.clone(), size: 0, kind: teams_core::FileKind::from_name(&file.name), reference: file.clone(), uploaded: None }).collect(),
        link_preview: None,
        importance: composed.importance,
        banner: composed.banner.clone(),
    }
}

/// Teams HTML for the markdown-ish composer text (`**bold**`, `*italic*`, `~~strike~~`, `` `code` ``,
/// `- ` lists, and `<u>underline</u>`), followed by the chosen GIFs.
pub fn html(text: &str, gifs: &[Gif]) -> String {
    let mut html = if text.trim().is_empty() {
        String::new()
    } else {
        // The markdown reader has no underline marker; the typed tag is escaped, so turn it back.
        let mut html = Draft::from_markdown(text).trimmed().to_html();
        for tag in crate::formatting::tags() { html = html.replace(&teams_core::escape_html(&tag), &tag); }
        html
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

    #[test]
    fn markdown_tables_and_code_blocks_are_sent_as_structured_html() {
        let html = html("| A | B |\n| --- | --- |\n| one | two |\n\n```rust\nlet x = 1;\n```", &[]);
        assert!(html.contains("<table"), "{html}");
        assert!(html.contains("<td"), "{html}");
        assert!(html.contains("<pre"), "{html}");
        assert!(html.contains("let x = 1;"), "{html}");
    }

    #[test]
    fn persistent_outgoing_keeps_formatting_gifs_mentions_reply_and_files() {
        let composed = crate::types::Composed {
            inline: vec![],
            importance: store::MessageImportance::Urgent,
            subject: Some("Subject".into()),
            banner: Some(teams_core::AnnouncementBanner { title: "Headline".into(), background: teams_core::BannerBackground::Color("indigo".into()) }),
            text: "**hello** @Ada".into(),
            mentions: vec![teams_core::MentionInput::user("ada", "Ada")],
            gifs: vec![gif(100)],
            reply: Some(crate::types::ReplyReference { message_id: "original".into(), sender: "Ada".into(), text: "earlier".into() }),
        };
        let files = vec![teams_core::FileReference { attachment_id: "attachment".into(), content_url: "https://x.test/file".into(), name: "plan.pdf".into() }];
        let outgoing = outgoing(&composed, &files);
        let record = state::outbox::outbox_record("queued", "chat", store::OutboxTarget::Flat, None, &outgoing, chrono::Utc::now()).expect("outgoing can be encoded");
        let restored = state::outbox::outgoing_of(&record).expect("outgoing can be restored");
        assert!(restored.html().contains("<b>hello</b>"));
        assert!(restored.html().contains("https://g.test/a.gif?x=1&amp;y=2"), "{}", restored.html());
        assert_eq!(restored.mentions, composed.mentions);
        assert_eq!(restored.reply.as_ref().map(|reply| reply.message_id.as_str()), Some("original"));
        assert_eq!(restored.extras().files, files);
        assert_eq!(restored.importance, store::MessageImportance::Urgent);
        assert_eq!(restored.subject, composed.subject);
        assert_eq!(restored.banner, composed.banner);
        let draft_record = composition_record("chat", &composed, &files).unwrap().unwrap();
        let (draft, restored_files) = composition(&draft_record).unwrap();
        assert_eq!(draft.text, composed.text);
        assert_eq!(draft.mentions, composed.mentions);
        assert_eq!(draft.reply, composed.reply);
        assert_eq!(draft.subject, composed.subject);
        assert_eq!(draft.banner, composed.banner);
        assert_eq!(draft.importance, composed.importance);
        assert_eq!(draft.gifs[0].url, composed.gifs[0].url);
        assert_eq!(restored_files, files);
    }

    #[test]
    fn a_disk_draft_keeps_inline_bytes_and_their_source_positions() {
        let image = state::outgoing::InlineImage { name: "photo.png".into(), image: app_base::inline_image::ImageData::from_bytes(app_base::inline_image::ImageFormat::Png, vec![1, 2, 3]), dimensions: Some((4, 5)) };
        let composed = crate::types::Composed { text: "before ![image](rune-attachment:test) after".into(), inline: vec![("![image](rune-attachment:test)".into(), image)], ..Default::default() };
        let record = composition_record("chat", &composed, &[]).unwrap().unwrap();
        let store = store::Store::open_in_memory().unwrap();
        store.save_draft(&record).unwrap();
        let record = store.draft("chat").unwrap().unwrap();
        let (restored, _) = composition(&record).unwrap();
        assert_eq!(restored.text, composed.text);
        assert_eq!(restored.inline, composed.inline);
        assert_eq!(outgoing(&restored, &[]).html(), outgoing(&composed, &[]).html());
    }
}
