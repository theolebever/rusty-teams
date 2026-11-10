//! Call-bar glyphs drawn as strokes and dots on a 24 x 24 grid (`Shape::view_box`), so a glyph
//! scales with its element and needs no icon font or image asset.

use std::f32::consts::{PI, TAU};

use rune::color::hex;
use rune::{Color, El, LineCap, Shape, canvas};

#[derive(Clone, Copy)]
pub enum Icon {
    Chat,
    People,
    Hand,
    React,
    View,
    More,
    Camera,
    Mic,
    Share,
    Chevron,
}

const STROKE: f32 = 1.8;

/// The glyph at `size` points; `slashed` adds the red diagonal that marks camera or mic off.
pub fn icon(kind: Icon, size: f32, color: Color, slashed: bool) -> El {
    let mut shape = glyph(kind, Shape::new().view_box(24.0, 24.0), color);
    if slashed {
        shape = shape.stroke_with(vec![[3.0, 3.0], [21.0, 21.0]], false, 2.4, LineCap::Round, hex("#C4314B").expect("red"));
    }
    canvas(shape).size(size, size)
}

fn glyph(kind: Icon, s: Shape, c: Color) -> Shape {
    match kind {
        Icon::Chat => {
            let s = loop_(s, rrect(3.0, 4.0, 18.0, 13.0, 3.0), c);
            line(s, vec![[7.0, 17.0], [7.0, 21.0], [12.0, 17.0]], c)
        }
        Icon::People => {
            let s = loop_(s, circle(9.0, 8.0, 3.5), c);
            let s = line(s, arc(9.0, 21.0, 7.0, PI, TAU), c);
            line(s, arc(17.0, 9.0, 2.6, -PI * 0.5, PI * 0.5), c)
        }
        Icon::Hand => {
            let s = loop_(s, rrect(6.0, 11.0, 12.0, 10.0, 3.0), c);
            let s = line(s, vec![[9.0, 11.0], [9.0, 4.0]], c);
            let s = line(s, vec![[12.0, 11.0], [12.0, 3.0]], c);
            line(s, vec![[15.0, 11.0], [15.0, 5.0]], c)
        }
        Icon::React => {
            let s = loop_(s, circle(12.0, 12.0, 9.0), c);
            let s = dot(s, 8.5, 10.0, 1.4, c);
            let s = dot(s, 15.5, 10.0, 1.4, c);
            line(s, arc(12.0, 12.0, 4.8, PI * 0.25, PI * 0.75), c)
        }
        Icon::View => {
            let s = loop_(s, rrect(3.0, 3.0, 8.0, 8.0, 2.0), c);
            let s = loop_(s, rrect(13.0, 3.0, 8.0, 8.0, 2.0), c);
            let s = loop_(s, rrect(3.0, 13.0, 8.0, 8.0, 2.0), c);
            loop_(s, rrect(13.0, 13.0, 8.0, 8.0, 2.0), c)
        }
        Icon::More => {
            let s = dot(s, 5.0, 12.0, 1.8, c);
            let s = dot(s, 12.0, 12.0, 1.8, c);
            dot(s, 19.0, 12.0, 1.8, c)
        }
        Icon::Camera => {
            let s = loop_(s, rrect(2.0, 6.5, 14.0, 11.0, 2.5), c);
            loop_(s, vec![[16.0, 10.5], [22.0, 7.0], [22.0, 17.0], [16.0, 13.5]], c)
        }
        Icon::Mic => {
            let s = loop_(s, rrect(9.0, 2.5, 6.0, 11.0, 3.0), c);
            let s = line(s, arc(12.0, 10.5, 6.5, 0.0, PI), c);
            let s = line(s, vec![[12.0, 17.0], [12.0, 21.0]], c);
            line(s, vec![[8.0, 21.0], [16.0, 21.0]], c)
        }
        Icon::Share => {
            let s = loop_(s, rrect(2.0, 5.0, 20.0, 14.0, 2.5), c);
            let s = line(s, vec![[12.0, 16.0], [12.0, 9.0]], c);
            line(s, vec![[8.5, 12.5], [12.0, 9.0], [15.5, 12.5]], c)
        }
        Icon::Chevron => line(s, vec![[6.5, 9.5], [12.0, 15.0], [17.5, 9.5]], c),
    }
}

fn line(s: Shape, points: Vec<[f32; 2]>, c: Color) -> Shape {
    s.stroke_with(points, false, STROKE, LineCap::Round, c)
}

fn loop_(s: Shape, points: Vec<[f32; 2]>, c: Color) -> Shape {
    s.stroke_with(points, true, STROKE, LineCap::Round, c)
}

fn dot(s: Shape, cx: f32, cy: f32, r: f32, c: Color) -> Shape {
    s.fill(circle(cx, cy, r), c)
}

fn circle(cx: f32, cy: f32, r: f32) -> Vec<[f32; 2]> {
    (0..24).map(|i| {
        let a = i as f32 / 24.0 * TAU;
        [cx + r * a.cos(), cy + r * a.sin()]
    }).collect()
}

/// Points on a circle from angle `a0` to `a1` (radians, y down), both ends included.
fn arc(cx: f32, cy: f32, r: f32, a0: f32, a1: f32) -> Vec<[f32; 2]> {
    const STEPS: usize = 12;
    (0..=STEPS).map(|i| {
        let a = a0 + (a1 - a0) * i as f32 / STEPS as f32;
        [cx + r * a.cos(), cy + r * a.sin()]
    }).collect()
}

fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<[f32; 2]> {
    let mut points = Vec::new();
    for (cx, cy, a0) in [
        (x + w - r, y + r, -PI * 0.5),
        (x + w - r, y + h - r, 0.0),
        (x + r, y + h - r, PI * 0.5),
        (x + r, y + r, PI),
    ] {
        points.extend(arc(cx, cy, r, a0, a0 + PI * 0.5));
    }
    points
}
