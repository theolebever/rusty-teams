//! Adaptive cards (text, facts, images, columns, open-URL buttons) and announcement banners.
//! Form inputs, client actions and server actions share their validation with GPUI.

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;
use teams_core::{
    AdaptiveCard, AnnouncementBanner, BannerBackground, CardAction, CardActionKind, CardElement, CardItem,
    ColumnWidth, ContainerStyle, ImageSize, TextColor, TextSize,
};

use crate::avatar::rgb;
use crate::link_preview::thumbnail;
use crate::rich_text::flatten;
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

pub fn view<A: Actions + 'static>(s: &UiState<A>, card: &AdaptiveCard, key: &str, message: &str) -> El {
    let scope = crate::card_state::Scope { conversation: s.selected.clone().unwrap_or_default(), message: message.into(), key: key.into(), task: false };
    scoped(s, card, &scope)
}

pub fn scoped<A: Actions + 'static>(s: &UiState<A>, card: &AdaptiveCard, scope: &crate::card_state::Scope) -> El {
    let key = &scope.key;
    let card = s.cards.replacements.get(scope).unwrap_or(card);
    let palette = active_palette();
    let mut body = col().key(key.to_owned()).w_full().max_w(480.0).padding(12.0).gap(8.0).radius(6.0)
        .border(1.0, rgb(palette.border)).fill(rgb(palette.surface_raised));
    body = body.children(items(s, &card.items, scope, card, &mut card.actions.len()));
    let buttons: Vec<El> = card.actions.iter().enumerate().filter_map(|(index, action)| button::<A>(s, scope, card, index, action)).collect();
    if !buttons.is_empty() {
        body = body.child(row().w_full().gap(8.0).children(buttons));
    }
    if let Some(error) = s.cards.errors.get(scope) { body = body.child(text(error.clone()).key(format!("{}-error", scope.key)).font(ts(12.0, 400.0)).wrap()); }
    if let Some(refresh) = &card.refresh {
        let automatic = refresh.runs_automatically(s.details.get(&scope.conversation).map(|details| details.members.len()), s.own.me.as_ref().map(|me| me.user_id.as_str()));
        let (auto_scope, auto_action) = (scope.clone(), refresh.action.clone());
        body = body.child(el().key(format!("{}-refresh-watch", key)).size(1.0, 1.0).on_visible(move |s: &mut UiState<A>, visible, _| {
            if visible && automatic && !s.read_only && !s.cards.busy.contains(&auto_scope) && s.cards.refreshed.insert(auto_scope.clone()) {
                s.cards.busy.insert(auto_scope.clone());
                s.actions.refresh_card(auto_scope.clone(), auto_action.clone(), teams_core::ExecuteTrigger::Automatic);
            }
        }));
        let (scope, action) = (scope.clone(), refresh.action.clone());
        body = body.child(crate::controls::button::<A>(&format!("{}-refresh", key), "Refresh", crate::controls::Variant::Secondary, crate::controls::Size::Sm, move |s: &mut UiState<A>, _| {
            if !s.read_only && !s.cards.busy.contains(&scope) { s.cards.busy.insert(scope.clone()); s.actions.refresh_card(scope.clone(), action.clone(), teams_core::ExecuteTrigger::Manual); }
        }));
    }
    crate::media::watch(s, key, image_urls(card).iter().map(|url| crate::rich_text::remote(url)).collect(), body)
}

fn items<A: Actions + 'static>(s: &UiState<A>, list: &[CardItem], scope: &crate::card_state::Scope, card: &AdaptiveCard, next_action: &mut usize) -> Vec<El> {
    list.iter().filter_map(|item| {
        // Traverse hidden elements too so toggling visibility cannot change action identities.
        let content = element(s, &item.element, scope, card, next_action);
        if item.id.as_ref().and_then(|id| s.cards.visibility.get(&(scope.clone(), id.clone()))).copied().unwrap_or(item.visible) { content } else { None }
    }).collect()
}

fn element<A: Actions + 'static>(s: &UiState<A>, element: &CardElement, scope: &crate::card_state::Scope, card: &AdaptiveCard, next_action: &mut usize) -> Option<El> {
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
            let cell = col().min_w(0.0).gap(6.0).children(items(s, &column.items, scope, card, next_action));
            match column.width {
                ColumnWidth::Pixels(width) => cell.w(width),
                ColumnWidth::Weighted(weight) => cell.grow(weight.max(0.1)),
                ColumnWidth::Stretch => cell.grow(1.0),
                ColumnWidth::Auto => cell,
            }
        }))),
        CardElement::Container { layout, items: inner, .. } => {
            let mut container = col().w_full().gap(6.0).children(items(s, inner, scope, card, next_action));
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
        CardElement::Input(input) => Some(crate::card_state::input(s, scope, input)),
        CardElement::Actions(actions) => Some(col().w_full().gap(6.0).children(actions.iter().filter_map(|action| {
            let index = *next_action;
            *next_action += 1;
            button(s, scope, card, index, action)
        }))),
        _ => None,
    }
}

fn button<A: Actions + 'static>(s: &UiState<A>, scope: &crate::card_state::Scope, card: &AdaptiveCard, index: usize, action: &CardAction) -> Option<El> {
    if !action.visible { return None; }
    let (scope_copy, card_copy, action_copy) = (scope.clone(), card.clone(), action.clone());
    let enabled = action.enabled && !s.cards.busy.contains(scope);
    let destructive = is_destructive(&action.title);
    let variant = match (enabled, destructive) {
        (false, _) => crate::controls::Variant::Quiet,
        (true, true) => crate::controls::Variant::Danger,
        (true, false) => crate::controls::Variant::Primary,
    };
    let mut button = crate::controls::button::<A>(&format!("{}-action-{index}", scope.key), &action.title, variant, crate::controls::Size::Md, move |s, tx| {
        crate::card_state::run(s, scope_copy.clone(), &card_copy, index, action_copy.clone(), tx);
    });
    // A Quiet button attaches no press handler, so swallow the press here or it falls through to the scrim.
    if !enabled {
        button = button.on_press(|_: &mut UiState<A>| {});
    }
    if let CardActionKind::ShowCard(child) = &action.kind
        && s.cards.shown.contains(&(scope.clone(), index)) {
            let child_scope = crate::card_state::Scope { key: format!("{}-show-{index}", scope.key), ..scope.clone() };
            button = col().w_full().child(button).child(scoped(s, child, &child_scope));
        }
    Some(button)
}

/// Whether a card action title starts a destructive verb. Matches whole words, so "Removed" does
/// not count. The Adaptive Card model has no `style` field, so the title is the only signal.
fn is_destructive(title: &str) -> bool {
    title
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| matches!(word.to_lowercase().as_str(), "delete" | "remove" | "discard" | "quit" | "leave"))
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
    let frame = frame.child(text(banner.title.clone()).font(ts(24.0, 600.0)).color(rgb(0xFFFFFF)).wrap().max_lines(2));
    crate::media::watch(s, key, banner.image().into_iter().collect(), frame)
}

#[cfg(test)]
mod tests {
    use crate::rich_text::remote;
    use super::*;
    #[derive(Default)]
    struct Fake;
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
    }

    #[test]
    fn action_sets_have_distinct_stable_keys_even_when_hidden() {
        let mut rt = Runtime::new(UiState::new(Fake), |s, _| {
            let card = AdaptiveCard::parse(r#"{"type":"AdaptiveCard","body":[{"type":"ActionSet","id":"hidden","isVisible":false,"actions":[{"type":"Action.ShowCard","title":"Hidden","card":{"type":"AdaptiveCard"}}]},{"type":"Container","items":[{"type":"ActionSet","actions":[{"type":"Action.ShowCard","title":"Nested","card":{"type":"AdaptiveCard"}}]}]}],"actions":[{"type":"Action.ShowCard","title":"Root","card":{"type":"AdaptiveCard"}}]}"#).unwrap();
            view(s, &card, "test-card", "message")
        }, (600.0, 500.0), Env { scale: 1.0, system_dark: false });
        rt.frame(0.0);
        assert!(rt.find("test-card-action-0").is_some());
        assert!(rt.find("test-card-action-1").is_none());
        assert!(rt.find("test-card-action-2").is_some());
    }

    #[test]
    fn card_images_are_collected_from_nested_items() {
        let card = AdaptiveCard::parse(r#"{"type":"AdaptiveCard","version":"1.5","body":[
            {"type":"Image","url":"https://x.test/a.png"},
            {"type":"Container","items":[{"type":"Image","url":"https://x.test/b.png"}]}]}"#).unwrap();
        assert_eq!(image_urls(&card), ["https://x.test/a.png", "https://x.test/b.png"]);
        assert_eq!(remote("https://x.test/a.png").url, "https://x.test/a.png");
    }

    #[test]
    fn destructive_titles_match_whole_words_only() {
        assert!(is_destructive("Delete"));
        assert!(is_destructive("Remove all members"));
        assert!(!is_destructive("Removed by admin"));
        assert!(!is_destructive("Quitting soon"));
        assert!(!is_destructive("Save"));
    }

    #[test]
    fn destructive_card_actions_use_the_danger_fill() {
        use app_backend::schemes::active_palette;
        use rune::ElementInfo;
        fn view(s: &UiState<Fake>, _: &mut Cx<UiState<Fake>>) -> El {
            let card = AdaptiveCard::parse(r#"{"type":"AdaptiveCard","body":[],"actions":[{"type":"Action.Submit","title":"Delete item"},{"type":"Action.Submit","title":"Save"}]}"#).unwrap();
            crate::cards::view(s, &card, "danger-card", "message")
        }
        let mut rt = Runtime::new(UiState::new(Fake), view, (600.0, 500.0), Env { scale: 1.0, system_dark: false });
        rt.frame(0.0);
        let elements = rt.elements();
        let by_key = |key: &str| -> &ElementInfo {
            elements.iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no element {key}"))
        };
        let red = rune::color::hex(&format!("#{:06X}", active_palette().red)).expect("red colour");
        let accent = rune::color::hex(&format!("#{:06X}", active_palette().accent)).expect("accent colour");
        assert_eq!(by_key("danger-card-action-0").fill, red, "delete is destructive");
        assert_eq!(by_key("danger-card-action-1").fill, accent, "save is primary");
    }
}
