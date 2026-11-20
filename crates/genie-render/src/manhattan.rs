//! The lane of a GWAS track: a dot for each variant at the height of its -log10 p, and a line at
//! the genome-wide significance of 5×10⁻⁸.

use std::collections::HashSet;

use genie_core::Viewport;
use genie_core::association::Association;

use crate::scene::{Hue, Paint, Primitive};

pub const HEIGHT: f32 = 100.0;
/// -log10 of 5×10⁻⁸.
pub const SIGNIFICANT: f32 = 7.301_03;
const DOT: f32 = 2.0;
/// The space over the highest dot.
const TOP: f32 = 4.0;
/// The distance in pixels within which a point finds a dot.
const REACH: f32 = 4.0;

/// The highest -log10 p that the lane shows: the highest in view, and at least one above the
/// line of significance.
pub fn ceiling(viewport: Viewport, associations: &[Association]) -> f32 {
    let visible = viewport.visible();
    associations
        .iter()
        .filter(|association| visible.contains(association.position))
        .map(|association| association.value)
        .fold(SIGNIFICANT + 1.0, f32::max)
}

fn height_of(value: f32, ceiling: f32, height: f32) -> f32 {
    height - 1.0 - value / ceiling * (height - TOP - 1.0)
}

/// The color of the lead variant of the LocusZoom colors.
pub const LEAD: Paint = Paint::Rgb([150, 50, 184]);
/// The half width of the diamond of the lead variant.
const DIAMOND: f32 = 5.0;

/// The r² of each association with the lead variant, in the order of the associations, and the
/// place of the lead, for the LocusZoom colors.
#[derive(Debug, Clone, Copy)]
pub struct Zoom<'a> {
    pub r2: &'a [Option<f32>],
    pub lead: usize,
}

/// The step of the LocusZoom colors of an r² with the lead: 5 from 0.8, 4 from 0.6, 3 from
/// 0.4, 2 from 0.2, 1 under 0.2, and 0 without an r².
fn step(r2: Option<f32>) -> u8 {
    match r2 {
        Some(value) if value >= 0.8 => 5,
        Some(value) if value >= 0.6 => 4,
        Some(value) if value >= 0.4 => 3,
        Some(value) if value >= 0.2 => 2,
        Some(_) => 1,
        None => 0,
    }
}

/// The LocusZoom color of an r² with the lead: red from 0.8, orange from 0.6, green from 0.4,
/// light blue from 0.2, dark blue under 0.2, and grey without an r².
pub fn locuszoom(r2: Option<f32>) -> Paint {
    match step(r2) {
        5 => Paint::Rgb([212, 63, 58]),
        4 => Paint::Rgb([238, 162, 54]),
        3 => Paint::Rgb([92, 184, 92]),
        2 => Paint::Rgb([70, 184, 218]),
        1 => Paint::Rgb([53, 126, 189]),
        _ => Paint::Muted,
    }
}

/// The dots of the associations in view, at most one in each pixel, and the line of
/// significance. `faint` gives the dots under the line a lighter color, as for every second
/// chromosome of the whole-genome view. With a `zoom`, the dots take the LocusZoom colors of
/// their r² with the lead, the higher r² on top, and the lead is a purple diamond.
pub fn scene(
    viewport: Viewport,
    associations: &[Association],
    height: f32,
    (hue, faint): (Hue, bool),
    zoom: Option<Zoom<'_>>,
) -> Vec<Primitive> {
    let top = ceiling(viewport, associations);
    let visible = viewport.visible();
    let line = height_of(SIGNIFICANT, top, height);
    let mut primitives = vec![Primitive::Rect {
        x: 0.0,
        y: line.round(),
        width: viewport.width() as f32,
        height: 1.0,
        paint: Paint::Line,
    }];
    let mut dots: Vec<(u8, f32, f32, Paint)> = Vec::new();
    let mut lead = None;
    for (index, association) in associations.iter().enumerate() {
        if !visible.contains(association.position) {
            continue;
        }
        let x = viewport.to_px(association.position as f64 + 0.5) as f32;
        let y = height_of(association.value, top, height);
        let (rank, paint) = match zoom {
            Some(zoom) if zoom.lead == index => {
                lead = Some((x, y));
                continue;
            }
            Some(zoom) => {
                let r2 = zoom.r2.get(index).copied().flatten();
                (step(r2), locuszoom(r2))
            }
            None if association.value >= SIGNIFICANT => (2, Paint::Hue(Hue::Vermillion)),
            None if faint => (1, Paint::Tint(hue)),
            None => (1, Paint::Hue(hue)),
        };
        dots.push((rank, x, y, paint));
    }
    dots.sort_by_key(|(rank, ..)| std::cmp::Reverse(*rank));
    let mut seen = HashSet::new();
    dots.retain(|(_, x, y, _)| seen.insert((x.floor() as i32, y.floor() as i32)));
    primitives.extend(
        dots.into_iter()
            .rev()
            .map(|(_, x, y, paint)| Primitive::Rect {
                x: x.floor() - DOT / 2.0,
                y: y.floor() - DOT / 2.0,
                width: DOT,
                height: DOT,
                paint,
            }),
    );
    if let Some((x, y)) = lead {
        primitives.push(Primitive::Ribbon {
            corners: [
                (x, y - DIAMOND),
                (x + DIAMOND, y),
                (x, y + DIAMOND),
                (x - DIAMOND, y),
            ],
            paint: LEAD,
        });
    }
    primitives
}

/// The association nearest to a point of the lane, if a dot is near it.
pub fn hit(
    viewport: Viewport,
    associations: &[Association],
    height: f32,
    x: f32,
    y: f32,
) -> Option<usize> {
    let top = ceiling(viewport, associations);
    let visible = viewport.visible();
    associations
        .iter()
        .enumerate()
        .filter(|(_, association)| visible.contains(association.position))
        .map(|(index, association)| {
            let dx = viewport.to_px(association.position as f64 + 0.5) as f32 - x;
            let dy = height_of(association.value, top, height) - y;
            (index, dx.hypot(dy))
        })
        .filter(|(_, distance)| *distance <= REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use genie_core::association::Association;
    use genie_core::{Region, Viewport};

    use super::{LEAD, SIGNIFICANT, Zoom, ceiling, hit, locuszoom, scene};
    use crate::scene::{Hue, Paint, Primitive};

    fn view() -> Viewport {
        Viewport::new(1_000_000, 100.0).show(Region::between(0, 1_000))
    }

    fn at(position: u64, value: f32) -> Association {
        Association {
            position,
            value,
            row: None,
        }
    }

    fn rects(primitives: &[Primitive]) -> Vec<([f32; 4], Paint)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect {
                    x,
                    y,
                    width,
                    height,
                    paint,
                } => Some(([*x, *y, *width, *height], *paint)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn dots_rise_with_their_value_under_a_line_of_significance() {
        let found = [at(100, 0.0), at(101, 0.0), at(500, 16.6), at(5_000, 30.0)];
        assert!((ceiling(view(), &found) - 16.6).abs() < 1e-4);
        assert!((ceiling(view(), &found[..2]) - (SIGNIFICANT + 1.0)).abs() < 1e-4);
        let drawn = rects(&scene(view(), &found, 100.0, (Hue::Blue, false), None));
        assert_eq!(drawn.len(), 3, "{drawn:?}");
        assert_eq!(drawn[0].1, Paint::Line);
        assert_eq!(drawn[1], ([9.0, 98.0, 2.0, 2.0], Paint::Hue(Hue::Blue)));
        assert_eq!(
            drawn[2],
            ([49.0, 3.0, 2.0, 2.0], Paint::Hue(Hue::Vermillion))
        );
        let faint = rects(&scene(view(), &found, 100.0, (Hue::Blue, true), None));
        assert_eq!(faint[1].1, Paint::Tint(Hue::Blue));
    }

    /// With a lead, the dots take the LocusZoom color of their r² with it, the higher r² on top,
    /// and the lead is a purple diamond.
    #[test]
    fn locuszoom_colors_follow_the_r2_with_the_lead() {
        let found = [at(100, 2.0), at(300, 4.0), at(500, 9.0), at(700, 1.0)];
        let r2 = [Some(0.9), Some(0.3), None, None];
        let zoom = Zoom { r2: &r2, lead: 2 };
        let primitives = scene(view(), &found, 100.0, (Hue::Blue, false), Some(zoom));
        let paints: Vec<Paint> = rects(&primitives).iter().map(|(_, paint)| *paint).collect();
        assert_eq!(
            paints,
            [
                Paint::Line,
                locuszoom(None),
                locuszoom(Some(0.3)),
                locuszoom(Some(0.9))
            ]
        );
        let lead = primitives.iter().any(
            |primitive| matches!(primitive, Primitive::Ribbon { paint, .. } if *paint == LEAD),
        );
        assert!(lead);
        let colors = [0.85, 0.65, 0.45, 0.25, 0.05].map(|r2| locuszoom(Some(r2)));
        assert_eq!(
            colors,
            [
                Paint::Rgb([212, 63, 58]),
                Paint::Rgb([238, 162, 54]),
                Paint::Rgb([92, 184, 92]),
                Paint::Rgb([70, 184, 218]),
                Paint::Rgb([53, 126, 189]),
            ]
        );
    }

    #[test]
    fn hits_find_the_nearest_dot() {
        let found = [at(100, 0.0), at(500, 16.6)];
        assert_eq!(hit(view(), &found, 100.0, 50.0, 5.0), Some(1));
        assert_eq!(hit(view(), &found, 100.0, 10.0, 98.0), Some(0));
        assert_eq!(hit(view(), &found, 100.0, 30.0, 50.0), None);
    }
}
