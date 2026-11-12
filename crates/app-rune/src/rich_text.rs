//! Message bodies with inline formatting, built from the core's span model.

use std::ops::Range;

use app_backend::schemes::active_palette;
use rune::*;
use store::MessageRecord;
use teams_core::{AdaptiveCard, AnnouncementBanner, ImageRef, LinkPreview, Span as Md};

use crate::avatar::rgb;
use crate::types::{Actions, MessageRow};
use crate::ui::{UiState, ts};

/// What a message shows beyond its plain text. Empty when the body has no formatting.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rich {
    pub spans: Vec<Md>,
    /// The own body before subject and card previews are added for display.
    pub(crate) edit_draft: Option<teams_core::Draft>,
    pub preview: Option<LinkPreview>,
    pub cards: Vec<AdaptiveCard>,
    pub banner: Option<AnnouncementBanner>,
}

impl Rich {
    pub fn from_record(record: &MessageRecord) -> Self {
        if record.deleted {
            return Self::default();
        }
        let attachments = teams_core::attachments(record);
        let cards = teams_core::adaptive_cards(record);
        let mut spans = teams_core::message_spans(record);
        let own_spans: Vec<_> = spans.iter().filter(|span| !matches!(span, Md::Quote { .. })).cloned().collect();
        let edit_draft = Some(teams_core::Draft::from_spans(&own_spans).trimmed());
        // Plain bodies keep using `MessageRow.text`, which also carries subject and card text.
        if spans.iter().all(|span| matches!(span, Md::Text(_))) {
            spans.clear();
        } else {
            if let Some(subject) = record.subject.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                spans.splice(0..0, [Md::Bold(vec![Md::Text(subject.to_owned())]), Md::LineBreak]);
            }
            if cards.is_empty() {
                for text in teams_core::card_texts(record) {
                    spans.extend([Md::LineBreak, Md::Text(text)]);
                }
            }
        }
        Self {
            spans,
            edit_draft,
            preview: teams_core::link_preview(record),
            cards,
            banner: AnnouncementBanner::from_attachments(&attachments),
        }
    }

    /// Everything the view draws from the image cache.
    pub fn images(&self) -> Vec<ImageRef> {
        let mut images: Vec<ImageRef> = self.preview.iter().filter_map(LinkPreview::image)
            .chain(self.banner.iter().filter_map(AnnouncementBanner::image)).collect();
        images.extend(self.cards.iter().flat_map(crate::cards::image_urls).map(|url| remote(&url)));
        images
    }
}

pub fn remote(url: &str) -> ImageRef {
    ImageRef { id: url.to_owned(), url: url.to_owned(), width: None, height: None, alt: None }
}

pub fn is_openable(url: &str) -> bool {
    ["https://", "http://", "mailto:"].iter().any(|scheme| url.starts_with(scheme))
}

pub fn open<A: Actions + 'static>(s: &mut UiState<A>, tx: &mut Tx, url: &str) {
    if is_openable(url) {
        if let Err(error) = tx.open_url(url) { s.status = error.to_string(); }
    }
}

/// The message body: formatted when the message has spans, else its plain text.
pub fn body<A: Actions + 'static>(m: &MessageRow, size: f32, color: Color) -> El {
    if m.media.rich.spans.is_empty() {
        return crate::ui::text(m.text.clone()).font(ts(size, 400.0)).color(color).wrap();
    }
    flatten(&m.media.rich.spans, size, m.mine).element::<A>(size, color)
}

#[derive(Clone, Copy, Default)]
struct Style {
    bold: bool,
    italic: bool,
    mono: bool,
    strike: bool,
    underline: bool,
    link: bool,
    mention: bool,
    quote: bool,
    color: Option<Color>,
    bg: Option<Color>,
    size: Option<f32>,
}

/// Text plus the byte ranges that style it, ready for `rune::rich`.
#[derive(Default)]
pub struct Flat {
    pub text: String,
    spans: Vec<rune::Span>,
    strikes: Vec<(usize, usize, Option<Color>)>,
    links: Vec<(Range<usize>, String)>,
    base: f32,
    own: bool,
}

pub fn flatten(spans: &[Md], base: f32, own: bool) -> Flat {
    let mut flat = Flat { base, own, ..Default::default() };
    flat.walk(spans, Style::default(), None);
    while flat.text.ends_with('\n') { flat.text.pop(); }
    flat
}

impl Flat {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub fn element<A: Actions + 'static>(self, size: f32, color: Color) -> El {
        let mut element = rune::rich(self.text, self.spans).font(ts(size, 400.0)).color(color)
            .strikes(self.strikes);
        if !self.links.is_empty() {
            for (range, url) in self.links { element = element.link(Link::new(range, url)); }
            element = element.on_link(|s: &mut UiState<A>, url: &str, tx: &mut Tx| open(s, tx, url));
        }
        element
    }

    fn newline(&mut self) {
        if !self.text.is_empty() && !self.text.ends_with('\n') { self.text.push('\n'); }
    }

    fn leaf(&mut self, text: &str, st: Style, link: Option<&str>) {
        let palette = active_palette();
        let start = self.text.len();
        self.text.push_str(&teams_core::emoji_presentation(text));
        let end = self.text.len();
        if start == end { return; }
        let mut span = rune::Span::new(start, end);
        let mut styled = false;
        if st.bold || st.mention { span = span.bold(); styled = true; }
        if st.italic || st.quote { span = span.italic(); styled = true; }
        if st.mono { span = span.mono().bg(rgb(palette.inline_code_fill), 2.0); styled = true; }
        if let Some(size) = st.size { span = span.size(size); styled = true; }
        let color = if st.link || st.mention { Some(rgb(palette.accent)) } else if st.quote { Some(rgb(palette.text_muted)) } else { st.color };
        if let Some(color) = color { span = span.color(color); styled = true; }
        let bg = if st.mention { Some(rgb(if self.own { palette.mention_background_own } else { palette.mention_background })) } else { st.bg };
        if let Some(bg) = bg { span = span.bg(bg, 1.0); styled = true; }
        if st.underline || st.link { span = span.underline(color.unwrap_or(rgb(palette.foreground))); styled = true; }
        if styled { self.spans.push(span); }
        if st.strike { self.strikes.push((start, end, None)); }
        if let Some(url) = link { self.links.push((start..end, url.to_owned())); }
    }

    fn walk(&mut self, spans: &[Md], st: Style, link: Option<&str>) {
        for span in spans {
            match span {
                Md::Text(text) => self.leaf(text, st, link),
                Md::Bold(c) => self.walk(c, Style { bold: true, ..st }, link),
                Md::Italic(c) => self.walk(c, Style { italic: true, ..st }, link),
                Md::Strike(c) => self.walk(c, Style { strike: true, ..st }, link),
                Md::Underline(c) => self.walk(c, Style { underline: true, ..st }, link),
                Md::Superscript(c) | Md::Subscript(c) => self.walk(c, Style { size: Some(self.base * 0.8), ..st }, link),
                Md::Code(code) => self.leaf(code, Style { mono: true, ..st }, link),
                Md::CodeBlock { code, .. } => {
                    self.newline();
                    self.leaf(code.trim_end(), Style { mono: true, ..st }, link);
                    self.newline();
                }
                Md::Link { url, children } => self.walk(children, Style { link: true, ..st }, Some(url)),
                Md::Mention { name, .. } => {
                    self.leaf(&format!("@{}", name.trim_start_matches('@')), Style { mention: true, ..st }, link);
                }
                Md::Sized(teams_core::FontSize::Hidden, _) => {}
                Md::Sized(size, c) => {
                    let size = match size {
                        teams_core::FontSize::Small => self.base * 0.85,
                        teams_core::FontSize::Large => self.base * 1.3,
                        teams_core::FontSize::Pixels(px) => f32::from(*px),
                        teams_core::FontSize::Hidden => self.base,
                    };
                    self.walk(c, Style { size: Some(size), ..st }, link);
                }
                Md::Colored { color, background, children } => {
                    let hex = |value: u32| rgb(value & 0xff_ffff);
                    self.walk(children, Style { color: color.map(hex).or(st.color), bg: background.map(hex).or(st.bg), ..st }, link);
                }
                Md::Heading { level, children } => {
                    self.newline();
                    let size = self.base + f32::from(4_u8.saturating_sub(*level));
                    self.walk(children, Style { bold: true, size: Some(size), ..st }, link);
                    self.newline();
                }
                Md::LineBreak => self.text.push('\n'),
                Md::Rule => { self.newline(); self.leaf("────────", st, None); self.newline(); }
                Md::List { ordered, start, items } => {
                    self.newline();
                    for (index, item) in items.iter().enumerate() {
                        let marker = if *ordered { format!("{}. ", *start as usize + index) } else { "• ".to_owned() };
                        self.leaf(&marker, st, None);
                        self.walk(item, st, link);
                        self.newline();
                    }
                }
                Md::Table { rows, .. } => {
                    self.newline();
                    for row in rows {
                        for (index, cell) in row.iter().enumerate() {
                            if index > 0 { self.leaf("  |  ", st, None); }
                            self.walk(cell, st, link);
                        }
                        self.newline();
                    }
                }
                Md::Image { .. } => self.leaf("[image]", Style { italic: true, ..st }, None),
                Md::CustomEmoji { name, .. } => self.leaf(&format!(":{name}:"), st, None),
                Md::Quote { children, .. } | Md::BlockQuote(children) => {
                    self.newline();
                    self.leaf("▎ ", Style { quote: true, ..st }, None);
                    self.walk(children, Style { quote: true, ..st }, link);
                    self.newline();
                }
                Md::Indent { children, .. } => self.walk(children, st, link),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str) -> Md { Md::Text(text.into()) }

    #[test]
    fn formatting_becomes_styled_ranges_and_links_keep_their_url() {
        let spans = vec![
            Md::Bold(vec![t("hi")]), t(" "),
            Md::Link { url: "https://example.com".into(), children: vec![t("site")] },
            Md::LineBreak,
            Md::List { ordered: false, start: 1, items: vec![vec![t("one")], vec![t("two")]] },
            Md::Mention { name: "Ada".into(), id: None },
        ];
        let flat = flatten(&spans, 14.0, false);
        assert_eq!(flat.text, "hi site\n• one\n• two\n@Ada");
        assert_eq!(flat.links, vec![(3..7, "https://example.com".to_owned())]);
        assert_eq!(flat.spans[0].weight, Some(600.0));
        assert!(flat.spans.iter().any(|span| span.end as usize == flat.text.len() && span.weight.is_some()), "mention is bold");
    }

    #[test]
    fn only_web_and_mail_links_open() {
        assert!(is_openable("https://a.b") && is_openable("mailto:a@b.c"));
        assert!(!is_openable("file:///C:/x.exe") && !is_openable("javascript:alert(1)"));
    }

    #[test]
    fn plain_bodies_stay_plain_and_formatted_ones_carry_the_subject() {
        let mut record = MessageRecord { body_html: "<p>hello</p>".into(), ..Default::default() };
        assert!(Rich::from_record(&record).spans.is_empty());
        record.body_html = "<p>hello <b>bold</b></p>".into();
        record.subject = Some("Title".into());
        let rich = Rich::from_record(&record);
        assert_eq!(rich.spans[0], Md::Bold(vec![t("Title")]));
        assert_eq!(flatten(&rich.spans, 14.0, false).text, "Title\nhello bold");
    }
}
