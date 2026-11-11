//! Initials avatars with an optional presence dot, and the overlapped pair, sized and coloured
//! as the GPUI app draws them (`crates/app/src/views/avatar.rs`).

#![allow(dead_code)] // the views that draw these land after this module

use app_backend::schemes::active_palette;
use rune::*;
use crate::ui::text;

use crate::icons::sym;
use crate::ui::ts;

const INITIALS_RATIO: f32 = 0.36;
const PAIR_RATIO: f32 = 24.0 / 36.0;
const PRESENCE_RATIO: f32 = 0.38;
const PRESENCE_MIN: f32 = 10.0;
const PRESENCE_MAX: f32 = 15.0;
const PRESENCE_RING: f32 = 2.0;
const GLYPH_DISC: f32 = 0.75;
const WHITE: u32 = 0xFFFFFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Available,
    Away,
    Busy,
    DoNotDisturb,
    Offline,
}

pub fn chat_presence(details: Option<&crate::types::ChatDetails>) -> Option<Presence> {
    use teams_core::Availability as A;
    Some(match details?.presence?.availability {
        A::Available => Presence::Available,
        A::Away => Presence::Away,
        A::Busy => Presence::Busy,
        A::DoNotDisturb => Presence::DoNotDisturb,
        A::Offline => Presence::Offline,
        A::Unknown => return None,
    })
}

/// A circle of `name`'s initials on a colour from its name, with a presence dot at bottom right.
pub fn avatar(name: &str, size: f32, presence: Option<Presence>) -> El {
    let palette = active_palette();
    let background = palette.avatar_palette[palette_index(name, palette.avatar_palette.len())];
    let face = el()
        .size(size, size)
        .radius(size / 2.0)
        .fill(rgb(background))
        .center()
        .child(text(initials(name)).font(ts((size * INITIALS_RATIO).round(), 600.0)).color(rgb(WHITE)));
    let mut root = el().size(size, size).child(face);
    if let Some(presence) = presence {
        root = root.child(presence_dot(presence, size));
    }
    root
}

fn photo<'a, A: crate::types::Actions + 'static>(s: &'a crate::ui::UiState<A>, id: Option<&str>) -> Option<&'a Image> {
    id.and_then(|id| s.media.images.get(&format!("avatar:{id}"))).filter(|image| image.is_ready() && image.error().is_none())
}

pub fn person<A: crate::types::Actions + 'static>(s: &crate::ui::UiState<A>, name: &str, id: Option<&str>, size: f32, presence: Option<Presence>) -> El {
    let Some(image) = photo(s, id) else {
        return avatar(name, size, presence);
    };
    let mut root = el().size(size, size).child(el().size(size, size).radius(size / 2.0).pixels_fit(image.pixels(), SurfaceFit::Fill).label(name));
    if let Some(presence) = presence { root = root.child(presence_dot(presence, size)); }
    root
}

/// A chat's avatar: the other person, the group's own picture, else two of its members as a pair.
pub fn chat<A: crate::types::Actions + 'static>(s: &crate::ui::UiState<A>, title: &str, chat_id: &str, size: f32) -> El {
    let details = s.details.get(chat_id);
    let avatar_id = details.and_then(|d| d.avatar_id.as_deref());
    let group = details.map_or(&[][..], |d| d.group.as_slice());
    match group {
        [first, second, ..] if photo(s, avatar_id).is_none() => group_avatar(s, [first, second], size),
        [only] if photo(s, avatar_id).is_none() => person(s, &only.display_name, only.user_id.as_deref(), size, None),
        _ => person(s, title, avatar_id, size, chat_presence(details)),
    }
}

/// Two avatars overlapped on a diagonal, as the GPUI pair: the first at top left, the second
/// at bottom right inside a ring of the surface colour. Photos when loaded, else initials.
fn group_avatar<A: crate::types::Actions + 'static>(s: &crate::ui::UiState<A>, members: [&store::MemberRecord; 2], size: f32) -> El {
    let small = (size * PAIR_RATIO).round();
    let [first, second] = members;
    let ring = el()
        .size(small, small)
        .radius(small / 2.0)
        .fill(rgb(active_palette().surface))
        .center()
        .absolute(size - small, size - small)
        .child(person(s, &second.display_name, second.user_id.as_deref(), small - 2.0 * PRESENCE_RING, None));
    el().size(size, size)
        .child(person(s, &first.display_name, first.user_id.as_deref(), small, None).absolute(0.0, 0.0))
        .child(ring)
}

fn presence_dot(presence: Presence, avatar_size: f32) -> El {
    let palette = active_palette();
    let (glyph, color) = match presence {
        Presence::Available => ("status_available", palette.green),
        Presence::Away => ("status_away", palette.amber),
        Presence::Busy => ("status_dot", palette.red),
        Presence::DoNotDisturb => ("status_dnd", palette.red),
        Presence::Offline => ("status_offline", palette.text_muted),
    };
    let outer = presence_size(avatar_size);
    let inner = outer - 2.0 * PRESENCE_RING;
    el()
        .size(outer, outer)
        .radius(outer / 2.0)
        .fill(rgb(palette.surface))
        .center()
        .absolute(avatar_size - outer, avatar_size - outer)
        .child(sym(glyph, inner / GLYPH_DISC).color(rgb(color)))
}

fn presence_size(avatar_size: f32) -> f32 {
    (avatar_size * PRESENCE_RATIO).round().clamp(PRESENCE_MIN, PRESENCE_MAX)
}

/// First letters of the first and last word, or the first letter of a single word; `?` for none.
fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty()).collect();
    let letters: Vec<char> = match words.as_slice() {
        [] => return "?".to_owned(),
        [single] => single.chars().take(1).collect(),
        [first, .., last] => first.chars().take(1).chain(last.chars().take(1)).collect(),
    };
    letters.into_iter().flat_map(char::to_uppercase).collect()
}

/// FNV-1a over the name, so one person keeps one colour everywhere.
fn palette_index(key: &str, len: usize) -> usize {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in key.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash as usize % len
}

pub(crate) fn rgb(value: u32) -> Color {
    let channel = |shift: u32| rune::color::srgb_decode(((value >> shift) & 0xFF) as f32 / 255.0);
    Color { r: channel(16), g: channel(8), b: channel(0), a: 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_colours_match_hex_decoding() {
        for value in [0, 0xFFFFFF, 0x123456, 0x5B5FC7, 0xFF0000] {
            assert_eq!(rgb(value), rune::color::hex(&format!("#{value:06X}")).unwrap());
        }
    }

    #[test]
    fn initials_use_first_and_last_word() {
        assert_eq!(initials("Mara Lindqvist"), "ML");
        assert_eq!(initials("wiki"), "W");
        assert_eq!(initials("  "), "?");
    }

    #[test]
    fn colour_follows_the_name_hash_inside_the_palette() {
        let len = active_palette().avatar_palette.len();
        assert_eq!(palette_index("Mara Lindqvist", len), palette_index("Mara Lindqvist", len));
        assert!(palette_index("Mara Lindqvist", len) < len);
    }

    #[test]
    fn presence_dot_scales_with_the_avatar_and_stays_in_bounds() {
        assert_eq!(presence_size(20.0), 10.0);
        assert_eq!(presence_size(36.0), 14.0);
        assert_eq!(presence_size(100.0), PRESENCE_MAX);
    }
}
