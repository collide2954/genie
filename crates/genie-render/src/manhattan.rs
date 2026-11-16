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

/// The dots of the associations in view, at most one in each pixel, and the line of
/// significance. `faint` gives the dots under the line a lighter color, as for every second
/// chromosome of the whole-genome view.
pub fn scene(
    viewport: Viewport,
    associations: &[Association],
    height: f32,
    (hue, faint): (Hue, bool),
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
    let mut seen = HashSet::new();
    let mut strong = Vec::new();
    for association in associations {
        if !visible.contains(association.position) {
            continue;
        }
        let x = viewport.to_px(association.position as f64 + 0.5) as f32;
        let y = height_of(association.value, top, height);
        if !seen.insert((x.floor() as i32, y.floor() as i32)) {
            continue;
        }
        let dot = |paint| Primitive::Rect {
            x: x.floor() - DOT / 2.0,
            y: y.floor() - DOT / 2.0,
            width: DOT,
            height: DOT,
            paint,
        };
        if association.value >= SIGNIFICANT {
            strong.push(dot(Paint::Hue(Hue::Vermillion)));
        } else if faint {
            primitives.push(dot(Paint::Tint(hue)));
        } else {
            primitives.push(dot(Paint::Hue(hue)));
        }
    }
    primitives.extend(strong);
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

    use super::{SIGNIFICANT, ceiling, hit, scene};
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
        let drawn = rects(&scene(view(), &found, 100.0, (Hue::Blue, false)));
        assert_eq!(drawn.len(), 3, "{drawn:?}");
        assert_eq!(drawn[0].1, Paint::Line);
        assert_eq!(drawn[1], ([9.0, 98.0, 2.0, 2.0], Paint::Hue(Hue::Blue)));
        assert_eq!(
            drawn[2],
            ([49.0, 3.0, 2.0, 2.0], Paint::Hue(Hue::Vermillion))
        );
        let faint = rects(&scene(view(), &found, 100.0, (Hue::Blue, true)));
        assert_eq!(faint[1].1, Paint::Tint(Hue::Blue));
    }

    #[test]
    fn hits_find_the_nearest_dot() {
        let found = [at(100, 0.0), at(500, 16.6)];
        assert_eq!(hit(view(), &found, 100.0, 50.0, 5.0), Some(1));
        assert_eq!(hit(view(), &found, 100.0, 10.0, 98.0), Some(0));
        assert_eq!(hit(view(), &found, 100.0, 30.0, 50.0), None);
    }
}
