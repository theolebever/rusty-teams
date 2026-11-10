//! Initials avatars with an optional presence dot, and the overlapped pair, sized and coloured
//! as the GPUI app draws them (`crates/app/src/views/avatar.rs`).

#![allow(dead_code)] // the views that draw these land after this module

use app_backend::schemes::active_palette;
use rune::*;

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

/// Two avatars overlapped on a diagonal, as the GPUI pair: the first at top left, the second
/// at bottom right inside a ring of the surface colour. Takes the first two names.
pub fn group_avatar(names: &[&str], size: f32) -> El {
    let small = (size * PAIR_RATIO).round();
    let mut group = el().size(size, size);
    if let Some(first) = names.first() {
        group = group.child(avatar(first, small, None).absolute(0.0, 0.0));
    }
    if let Some(second) = names.get(1) {
        let ring = el()
            .size(small, small)
            .radius(small / 2.0)
            .fill(rgb(active_palette().surface))
            .center()
            .absolute(size - small, size - small)
            .child(avatar(second, small - 2.0 * PRESENCE_RING, None));
        group = group.child(ring);
    }
    group
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

fn rgb(value: u32) -> Color {
    rune::color::hex(&format!("#{value:06X}")).expect("palette colour")
}

#[cfg(test)]
mod tests {
    use super::*;

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
