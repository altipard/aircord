//! The icons: monochrome outlines on a 24 × 24 grid, one 1.5 pt pen at every size, no fills. The mechanism (and the
//! Search, Close, Copy, Delete, Open, Save, Link and Check drawings) come from BIMCraft's `icons.rs`; the rest are
//! Aircord's own in the same hand, the Sun and Moon of the theme switch after PhotoCraft's title bar.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use std::f32::consts::TAU;

pub const STROKE_WIDTH: f32 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Scan,
    Stop,
    Clear,
    Sessions,
    Playbooks,
    Firmware,
    Connect,
    Disconnect,
    Send,
    Copy,
    Save,
    Delete,
    Search,
    Close,
    Play,
    Edit,
    Import,
    Export,
    Sun,
    Moon,
    History,
    Check,
    Warning,
}

impl Icon {
    pub const ALL: &'static [Icon] = &[
        Icon::Scan,
        Icon::Stop,
        Icon::Clear,
        Icon::Sessions,
        Icon::Playbooks,
        Icon::Firmware,
        Icon::Connect,
        Icon::Disconnect,
        Icon::Send,
        Icon::Copy,
        Icon::Save,
        Icon::Delete,
        Icon::Search,
        Icon::Close,
        Icon::Play,
        Icon::Edit,
        Icon::Import,
        Icon::Export,
        Icon::Sun,
        Icon::Moon,
        Icon::History,
        Icon::Check,
        Icon::Warning,
    ];
}

/// A single unfilled contour in grid coordinates.
#[derive(Clone, Debug)]
pub struct Outline {
    pub points: Vec<Pos2>,
    pub closed: bool,
}

#[derive(Default)]
struct Glyph(Vec<Outline>);

impl Glyph {
    fn line(&mut self, points: &[(f32, f32)]) {
        self.0.push(Outline { points: points.iter().map(|&(x, y)| pos2(x, y)).collect(), closed: false });
    }

    fn closed(&mut self, points: &[(f32, f32)]) {
        self.0.push(Outline { points: points.iter().map(|&(x, y)| pos2(x, y)).collect(), closed: true });
    }

    fn arc(&mut self, center: (f32, f32), radius: f32, from: f32, to: f32) {
        let start = from.to_radians();
        let sweep = (to - from).to_radians();
        let steps = (sweep.abs() / TAU * 128.0).ceil().max(2.0) as usize;
        let points = (0..=steps)
            .map(|i| {
                let angle = start + sweep * i as f32 / steps as f32;
                pos2(center.0 + radius * angle.cos(), center.1 + radius * angle.sin())
            })
            .collect();
        self.0.push(Outline { points, closed: false });
    }

    fn circle(&mut self, center: (f32, f32), radius: f32) {
        let points = (0..128)
            .map(|i| {
                let angle = TAU * i as f32 / 128.0;
                pos2(center.0 + radius * angle.cos(), center.1 + radius * angle.sin())
            })
            .collect();
        self.0.push(Outline { points, closed: true });
    }
}

/// All contours of an icon, in the 24 pt design space.
pub fn outlines(icon: Icon) -> Vec<Outline> {
    let mut g = Glyph::default();
    match icon {
        Icon::Scan => {
            // A radio source and the waves it hears.
            g.circle((12.0, 16.0), 1.6);
            g.arc((12.0, 16.0), 5.0, -140.0, -40.0);
            g.arc((12.0, 16.0), 9.0, -140.0, -40.0);
            g.line(&[(12.0, 17.6), (12.0, 21.0)]);
        }
        Icon::Stop => g.closed(&[(6.0, 6.0), (18.0, 6.0), (18.0, 18.0), (6.0, 18.0)]),
        Icon::Clear => {
            // A list being wiped: three rows and a sweep across them.
            g.line(&[(4.0, 7.0), (14.0, 7.0)]);
            g.line(&[(4.0, 12.0), (11.0, 12.0)]);
            g.line(&[(4.0, 17.0), (9.0, 17.0)]);
            g.line(&[(14.0, 13.0), (20.0, 19.0)]);
            g.line(&[(20.0, 13.0), (14.0, 19.0)]);
        }
        Icon::Sessions => {
            g.line(&[(3.0, 19.0), (3.0, 5.0), (9.0, 5.0), (12.0, 8.0), (21.0, 8.0)]);
            g.closed(&[(3.0, 19.0), (6.0, 10.0), (22.0, 10.0), (19.0, 19.0)]);
        }
        Icon::Playbooks => {
            // Steps in order, the first one playing.
            g.closed(&[(3.5, 4.5), (8.5, 7.0), (3.5, 9.5)]);
            g.line(&[(11.0, 7.0), (21.0, 7.0)]);
            g.line(&[(4.0, 12.5), (6.0, 12.5)]);
            g.line(&[(11.0, 12.5), (21.0, 12.5)]);
            g.line(&[(4.0, 18.0), (6.0, 18.0)]);
            g.line(&[(11.0, 18.0), (21.0, 18.0)]);
        }
        Icon::Firmware => {
            // A chip with pins, and an arrow going into it.
            g.closed(&[(6.0, 6.0), (18.0, 6.0), (18.0, 18.0), (6.0, 18.0)]);
            for x in [9.0, 12.0, 15.0] {
                g.line(&[(x, 3.0), (x, 6.0)]);
                g.line(&[(x, 18.0), (x, 21.0)]);
            }
            g.line(&[(12.0, 9.0), (12.0, 15.0)]);
            g.line(&[(9.5, 11.5), (12.0, 9.0), (14.5, 11.5)]);
        }
        Icon::Connect => {
            g.closed(&[(5.0, 8.0), (13.0, 8.0), (15.0, 10.0), (15.0, 14.0), (13.0, 16.0), (5.0, 16.0), (3.0, 14.0), (3.0, 10.0)]);
            g.closed(&[(11.0, 8.0), (19.0, 8.0), (21.0, 10.0), (21.0, 14.0), (19.0, 16.0), (11.0, 16.0), (9.0, 14.0), (9.0, 10.0)]);
        }
        Icon::Disconnect => {
            // The two links pulled apart, a gap between them.
            g.line(&[(9.0, 8.0), (5.0, 8.0), (3.0, 10.0), (3.0, 14.0), (5.0, 16.0), (9.0, 16.0)]);
            g.line(&[(15.0, 8.0), (19.0, 8.0), (21.0, 10.0), (21.0, 14.0), (19.0, 16.0), (15.0, 16.0)]);
            g.line(&[(12.0, 3.0), (12.0, 6.0)]);
            g.line(&[(12.0, 18.0), (12.0, 21.0)]);
        }
        Icon::Send => {
            g.closed(&[(3.0, 4.0), (21.0, 12.0), (3.0, 20.0), (6.5, 12.0)]);
            g.line(&[(6.5, 12.0), (13.0, 12.0)]);
        }
        Icon::Copy => {
            g.closed(&[(8.0, 3.0), (20.0, 3.0), (20.0, 15.0), (8.0, 15.0)]);
            g.line(&[(8.0, 9.0), (4.0, 9.0), (4.0, 21.0), (16.0, 21.0), (16.0, 15.0)]);
        }
        Icon::Save | Icon::Import => {
            g.line(&[(12.0, 3.0), (12.0, 14.0)]);
            g.line(&[(8.0, 10.0), (12.0, 14.0), (16.0, 10.0)]);
            g.line(&[(4.0, 15.0), (4.0, 20.0), (20.0, 20.0), (20.0, 15.0)]);
        }
        Icon::Export => {
            g.line(&[(12.0, 14.0), (12.0, 3.0)]);
            g.line(&[(8.0, 7.0), (12.0, 3.0), (16.0, 7.0)]);
            g.line(&[(4.0, 15.0), (4.0, 20.0), (20.0, 20.0), (20.0, 15.0)]);
        }
        Icon::Delete => {
            g.line(&[(4.0, 6.0), (20.0, 6.0)]);
            g.line(&[(9.0, 6.0), (9.0, 3.0), (15.0, 3.0), (15.0, 6.0)]);
            g.line(&[(6.0, 6.0), (7.0, 21.0), (17.0, 21.0), (18.0, 6.0)]);
            g.line(&[(10.0, 10.0), (10.0, 17.0)]);
            g.line(&[(14.0, 10.0), (14.0, 17.0)]);
        }
        Icon::Search => {
            g.line(&[(15.0, 15.0), (21.0, 21.0)]);
            g.circle((10.0, 10.0), 7.0);
        }
        Icon::Close => {
            g.line(&[(5.0, 5.0), (19.0, 19.0)]);
            g.line(&[(19.0, 5.0), (5.0, 19.0)]);
        }
        Icon::Play => g.closed(&[(7.0, 4.5), (19.0, 12.0), (7.0, 19.5)]),
        Icon::Edit => {
            g.closed(&[(4.0, 20.0), (5.0, 15.5), (16.0, 4.5), (19.5, 8.0), (8.5, 19.0)]);
            g.line(&[(13.5, 7.0), (17.0, 10.5)]);
        }
        Icon::Sun => {
            // The theme switch in dark mode: back to light.
            g.circle((12.0, 12.0), 4.0);
            for k in 0..8 {
                let a = k as f32 * TAU / 8.0;
                let (s, c) = a.sin_cos();
                g.line(&[(12.0 + 6.5 * c, 12.0 + 6.5 * s), (12.0 + 9.0 * c, 12.0 + 9.0 * s)]);
            }
        }
        Icon::Moon => {
            // The theme switch in light mode: a crescent, the outer circle cut by an inner one at its upper right.
            g.arc((12.0, 12.0), 8.5, 10.4, 259.6);
            g.arc((15.5, 8.5), 7.0, 224.0, 46.0);
        }
        Icon::History => {
            g.arc((12.0, 12.0), 8.5, -150.0, 180.0);
            g.line(&[(1.5, 9.0), (3.5, 12.0), (6.5, 9.5)]);
            g.line(&[(12.0, 7.0), (12.0, 12.0), (15.5, 14.0)]);
        }
        Icon::Check => {
            g.line(&[(7.0, 12.0), (10.5, 15.5), (17.0, 9.0)]);
            g.circle((12.0, 12.0), 9.0);
        }
        Icon::Warning => {
            g.closed(&[(12.0, 3.0), (21.5, 20.0), (2.5, 20.0)]);
            g.line(&[(12.0, 9.0), (12.0, 14.0)]);
            g.line(&[(12.0, 16.5), (12.0, 17.5)]);
        }
    }
    g.0
}

/// An icon fitted into `rect` without distortion, one pen weight for every stroke.
pub fn shapes(icon: Icon, rect: Rect, ink: Color32) -> Vec<Shape> {
    let side = rect.width().min(rect.height());
    if !side.is_finite() || side <= 0.0 || !rect.min.is_finite() || !rect.max.is_finite() {
        return Vec::new();
    }
    let scale = side / 24.0;
    let origin = rect.center() - vec2(side, side) * 0.5;
    let stroke = Stroke::new(STROKE_WIDTH, ink);
    outlines(icon)
        .into_iter()
        .map(|o| {
            let points = o.points.into_iter().map(|p| origin + p.to_vec2() * scale).collect();
            if o.closed { Shape::closed_line(points, stroke) } else { Shape::line(points, stroke) }
        })
        .collect()
}

pub fn paint(painter: &Painter, icon: Icon, rect: Rect, ink: Color32) {
    painter.extend(shapes(icon, rect, ink));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_stays_inside_its_grid_with_room_for_the_pen() {
        for &icon in Icon::ALL {
            let paths = outlines(icon);
            assert!(!paths.is_empty(), "{icon:?}");
            for p in paths.iter().flat_map(|o| &o.points) {
                assert!(p.is_finite(), "{icon:?}");
                assert!((1.5..=22.5).contains(&p.x) && (1.5..=22.5).contains(&p.y), "{icon:?}: {p:?}");
            }
        }
    }

    #[test]
    fn a_degenerate_rect_draws_nothing() {
        assert!(shapes(Icon::Scan, Rect::NOTHING, Color32::WHITE).is_empty());
        assert!(!shapes(Icon::Scan, Rect::from_min_size(pos2(0.0, 0.0), vec2(18.0, 18.0)), Color32::WHITE).is_empty());
    }
}
