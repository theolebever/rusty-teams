//! Adaptive cards (text, facts, images, columns, open-URL buttons) and announcement banners.
//! Inputs, charts, tables and the other element kinds are not drawn.

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;
use teams_core::{
    AdaptiveCard, AnnouncementBanner, BannerBackground, CardAction, CardActionKind, CardElement, CardItem,
    ColumnWidth, ContainerStyle, ImageSize, TextColor, TextSize,
};

use crate::avatar::rgb;
use crate::link_preview::thumbnail;
use crate::rich_text::{flatten, remote};
use crate::types::Actions;
use crate::ui::{UiState, ts};

const BANNER_HEIGHT: f32 = 112.0;
const BANNER_SHADE: f32 = 0.3;

/// Image URLs a card needs, so the view can ask the core to fetch them.
pub fn image_urls(card: &AdaptiveCard) -> Vec<String> {
    fn items(list: &[CardItem], out: &mut Vec<String>) {
        for item in list {
            match &item.element {
                CardElement::Image(image) => out.push(image.url.clone()),
                CardElement::ImageSet(images) => out.extend(images.iter().map(|image| image.url.clone())),
                CardElement::Media(media) => out.extend(media.poster_url.clone()),
                CardElement::Columns { columns, .. } => columns.iter().for_each(|column| items(&column.items, out)),
                CardElement::Container { items: inner, .. } => items(inner, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    items(&card.items, &mut out);
    out
}

pub fn view<A: Actions + 'static>(s: &UiState<A>, card: &AdaptiveCard, key: &str) -> El {
    let palette = active_palette();
    let mut body = col().key(key.to_owned()).w_full().max_w(480.0).padding(12.0).gap(8.0).radius(6.0)
        .border(1.0, rgb(palette.border)).fill(rgb(palette.surface_raised));
    body = body.children(items(s, &card.items));
    let buttons: Vec<El> = card.actions.iter().enumerate().filter_map(|(index, action)| button::<A>(key, index, action)).collect();
    if !buttons.is_empty() {
        body = body.child(row().w_full().gap(8.0).children(buttons));
    }
    crate::media::watch(s, key, image_urls(card).iter().map(|url| crate::rich_text::remote(url)).collect(), body)
}

fn items<A: Actions + 'static>(s: &UiState<A>, list: &[CardItem]) -> Vec<El> {
    list.iter().filter(|item| item.visible).filter_map(|item| element(s, &item.element)).collect()
}

fn element<A: Actions + 'static>(s: &UiState<A>, element: &CardElement) -> Option<El> {
    let palette = active_palette();
    match element {
        CardElement::Text(block) => {
            let size = match block.size {
                TextSize::Small => 11.0, TextSize::Default => 13.0, TextSize::Medium => 15.0,
                TextSize::Large => 18.0, TextSize::ExtraLarge => 22.0,
            };
            let color = match block.color {
                TextColor::Accent => palette.accent, TextColor::Good => palette.green,
                TextColor::Warning => palette.amber, TextColor::Attention => palette.red,
                _ if block.subtle => palette.text_muted,
                _ => palette.foreground,
            };
            let flat = flatten(&block.spans, size, false);
            (!flat.is_empty()).then(|| flat.element::<A>(size, rgb(color)).font(ts(size, if block.bold { 600.0 } else { 400.0 })))
        }
        CardElement::Image(image) => {
            let side = match image.size { ImageSize::Small => 40.0, ImageSize::Medium => 80.0, ImageSize::Large => 160.0, _ => 240.0 };
            thumbnail(s, &image.url, image.width.unwrap_or(side), image.height.unwrap_or(side))
        }
        CardElement::ImageSet(images) => {
            let pictures: Vec<El> = images.iter().filter_map(|image| thumbnail(s, &image.url, 80.0, 80.0)).collect();
            (!pictures.is_empty()).then(|| row().w_full().gap(6.0).children(pictures))
        }
        CardElement::Media(media) => thumbnail(s, media.poster_url.as_deref()?, 240.0, 135.0),
        CardElement::Facts(facts) => Some(col().w_full().gap(4.0).children(facts.iter().map(|fact| {
            let value = flatten(&fact.value, 13.0, false);
            row().w_full().gap(12.0)
                .child(text(fact.title.clone()).font(ts(13.0, 600.0)).color(rgb(palette.text_soft)).w(120.0).wrap())
                .child(value.element::<A>(13.0, rgb(palette.foreground)).grow(1.0).min_w(0.0))
        }))),
        CardElement::Columns { columns, .. } => Some(row().w_full().gap(8.0).children(columns.iter().map(|column| {
            let cell = col().min_w(0.0).gap(6.0).children(items(s, &column.items));
            match column.width {
                ColumnWidth::Pixels(width) => cell.w(width),
                ColumnWidth::Weighted(weight) => cell.grow(weight.max(0.1)),
                ColumnWidth::Stretch => cell.grow(1.0),
                ColumnWidth::Auto => cell,
            }
        }))),
        CardElement::Container { layout, items: inner, .. } => {
            let mut container = col().w_full().gap(6.0).children(items(s, inner));
            let fill = match layout.style {
                ContainerStyle::Default => None,
                ContainerStyle::Emphasis => Some(palette.row_hover),
                ContainerStyle::Accent => Some(palette.accent_soft),
                ContainerStyle::Good => Some(palette.green),
                ContainerStyle::Warning => Some(palette.amber),
                ContainerStyle::Attention => Some(palette.red_soft),
            };
            if let Some(fill) = fill { container = container.padding(8.0).radius(4.0).fill(rgb(fill).alpha(0.35)); }
            Some(container)
        }
        _ => None,
    }
}

fn button<A: Actions + 'static>(key: &str, index: usize, action: &CardAction) -> Option<El> {
    let CardActionKind::OpenUrl(url) = &action.kind else { return None };
    if !action.visible || !action.enabled || !crate::rich_text::is_openable(url) { return None; }
    let palette = active_palette();
    let url = url.clone();
    Some(row().key(format!("{key}-action-{index}")).px(12.0).h(30.0).center_y().radius(4.0)
        .border(1.0, rgb(palette.accent)).hover(rgb(palette.row_hover)).label(action.title.clone())
        .on_press_tx(move |s: &mut UiState<A>, tx| crate::rich_text::open(s, tx, &url))
        .child(text(action.title.clone()).font(ts(13.0, 600.0)).color(rgb(palette.accent))))
}

/// The coloured headline a channel announcement opens with.
pub fn banner<A: Actions + 'static>(s: &UiState<A>, banner: &AnnouncementBanner, key: &str) -> El {
    let (from, _) = match &banner.background {
        BannerBackground::Color(color) => match color.as_str() {
            "indigo" => (0x2F3185, 0x5B5FC7),
            "rose" => (0xA4264F, 0xE26D8B),
            "orangeSherbet" => (0xD8590F, 0xF5A55A),
            _ => (0x4F52B2, 0x8B8EF0),
        },
        _ => (0x4F52B2, 0x8B8EF0),
    };
    let mut frame = col().key(key.to_owned()).w_full().h(BANNER_HEIGHT).radius(8.0).fill(rgb(from)).center().px(24.0);
    if let Some(image) = banner.image()
        && let Some(loaded) = s.media.image(image.key()).filter(|loaded| loaded.error().is_none())
    {
        frame = frame.child(col().absolute(0.0, 0.0).w_full().h_full().pixels_fit(loaded.pixels(), SurfaceFit::Fill))
            .child(col().absolute(0.0, 0.0).w_full().h_full().fill(rgb(0).alpha(BANNER_SHADE)));
    }
    let frame = frame.child(text(banner.title.clone()).font(ts(24.0, 700.0)).color(rgb(0xFFFFFF)).wrap().max_lines(2));
    crate::media::watch(s, key, banner.image().into_iter().collect(), frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_images_are_collected_from_nested_items() {
        let card = AdaptiveCard::parse(r#"{"type":"AdaptiveCard","version":"1.5","body":[
            {"type":"Image","url":"https://x.test/a.png"},
            {"type":"Container","items":[{"type":"Image","url":"https://x.test/b.png"}]}]}"#).unwrap();
        assert_eq!(image_urls(&card), ["https://x.test/a.png", "https://x.test/b.png"]);
        assert_eq!(remote("https://x.test/a.png").url, "https://x.test/a.png");
    }
}
