//! The lane of a Hi-C track: the contacts of the bins in view as a triangle over the diagonal,
//! with the contacts of near bins at the bottom. Each contact is a brick one bin wide and half a
//! bin high. Each row of bricks is half a bin higher and half a bin to the right of the row
//! under it, so the bricks stand for the diamonds of the rotated map.

use genie_core::Viewport;
use genie_core::contact::ContactMap;

use crate::scene::{Paint, Primitive};

pub const HEIGHT: f32 = 160.0;
/// The part of the counts in view that is under the count that gets the full color.
const SHARE: f64 = 0.95;
/// The least part of the full color that the middle count in view gets.
const MIDDLE: f32 = 0.2;
/// The flattest curve of the colors, for the most skewed counts.
const FLATTEST: f32 = 0.35;
/// The log2 of the ratio of observed to expected contacts that gets the full color.
const RATIO: f32 = 2.0;

/// The width of a bin in pixels.
fn bin_width(viewport: Viewport, map: &ContactMap) -> Option<f32> {
    if map.bin == 0 {
        return None;
    }
    let width = (viewport.to_px(map.bin as f64) - viewport.to_px(0.0)) as f32;
    (width > 0.0).then_some(width)
}

/// The rects of the contacts in view.
pub fn scene(viewport: Viewport, map: &ContactMap, height: f32) -> Vec<Primitive> {
    let Some(width) = bin_width(viewport, map) else {
        return Vec::new();
    };
    let rise = width / 2.0;
    let visible = viewport.visible();
    let bin = map.bin as f64;
    let mut shown = Vec::new();
    for &(x, y, value) in &map.contacts {
        let top = height - (y - x + 1) as f32 * rise;
        let left = (f64::from(x) + f64::from(y)) * bin / 2.0;
        let seen = left + bin >= visible.start() as f64 && left <= visible.end() as f64;
        if top + rise > 0.0 && seen {
            shown.push((left, top, value));
        }
    }
    let (scale, curve) = if map.ratio {
        (1.0, 1.0)
    } else {
        let mut counts: Vec<f32> = shown.iter().map(|(.., value)| *value).collect();
        colors(&mut counts)
    };
    shown
        .into_iter()
        .filter_map(|(left, top, value)| {
            let level = if map.ratio {
                ratio_level(value)?
            } else if scale > 0.0 {
                ((value / scale).clamp(0.0, 1.0).powf(curve) * 100.0).round() as i8
            } else {
                0
            };
            let y = top.max(0.0);
            Some(Primitive::Rect {
                x: viewport.to_px(left) as f32,
                y,
                width,
                height: top + rise - y,
                paint: Paint::Change(level),
            })
        })
        .collect()
}

/// The count that gets the full color, and the power of the curve of the colors. The full color
/// starts at the count that is higher than 95 % of the counts in view. The colors follow a
/// straight line, unless the middle count would get less than a fifth of the full color: then
/// they follow a curve that gives it a fifth. In a view of a whole chromosome, the contacts of
/// near bins are much higher than the others, and a straight line leaves the map pale.
fn colors(counts: &mut [f32]) -> (f32, f32) {
    counts.sort_by(f32::total_cmp);
    let at = ((counts.len().saturating_sub(1)) as f64 * SHARE).round() as usize;
    let top = counts.get(at).copied().unwrap_or(1.0);
    let middle = counts.get(counts.len() / 2).copied().unwrap_or(0.0);
    let curve = if middle > 0.0 && top > middle {
        (MIDDLE.ln() / (middle / top).ln()).clamp(FLATTEST, 1.0)
    } else {
        1.0
    };
    (top, curve)
}

/// The level of a ratio of observed to expected contacts: -100 for a quarter, 100 for 4.
fn ratio_level(value: f32) -> Option<i8> {
    if value <= 0.0 || !value.is_finite() {
        return None;
    }
    Some((value.log2().clamp(-RATIO, RATIO) / RATIO * 100.0).round() as i8)
}

/// The two bins of the contact under a point of the lane.
pub fn hit(
    viewport: Viewport,
    map: &ContactMap,
    height: f32,
    x: f32,
    y: f32,
) -> Option<(u32, u32)> {
    let rise = bin_width(viewport, map)? / 2.0;
    if y > height {
        return None;
    }
    let depth = ((height - y) / rise).floor();
    let first = viewport.to_bp(f64::from(x)) / map.bin as f64 - f64::from(depth) / 2.0;
    if first < 0.0 {
        return None;
    }
    let first = first.floor() as u32;
    Some((first, first + depth as u32))
}

#[cfg(test)]
mod tests {
    use genie_core::contact::ContactMap;
    use genie_core::{Region, Viewport};

    use super::{hit, scene};
    use crate::scene::{Paint, Primitive};

    fn view(start: u64, end: u64) -> Viewport {
        Viewport::new(1_000_000, 100.0).show(Region::between(start, end))
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
    fn contacts_are_bricks_that_rise_with_the_distance_of_their_bins() {
        let map = ContactMap {
            bin: 100,
            contacts: vec![(0, 0, 10.0), (0, 1, 5.0), (2, 4, 20.0), (30, 90, 7.0)],
            ratio: false,
        };
        assert_eq!(
            rects(&scene(view(0, 1_000), &map, 40.0)),
            [
                ([0.0, 35.0, 10.0, 5.0], Paint::Change(50)),
                ([5.0, 30.0, 10.0, 5.0], Paint::Change(25)),
                ([30.0, 25.0, 10.0, 5.0], Paint::Change(100)),
            ]
        );
    }

    #[test]
    fn ratios_go_from_blue_to_vermillion() {
        let map = ContactMap {
            bin: 100,
            contacts: vec![(0, 0, 4.0), (1, 1, 1.0), (2, 2, 0.5), (3, 3, 0.0)],
            ratio: true,
        };
        let paints: Vec<Paint> = rects(&scene(view(0, 1_000), &map, 40.0))
            .into_iter()
            .map(|(_, paint)| paint)
            .collect();
        assert_eq!(
            paints,
            [Paint::Change(100), Paint::Change(0), Paint::Change(-50)]
        );
    }

    /// In a view of a whole chromosome, the contacts of near bins are much higher than the
    /// others. The colors then follow a curve that gives the middle count a fifth of the color.
    #[test]
    fn skewed_counts_bend_the_colors_so_the_middle_count_shows() {
        let mut contacts = Vec::new();
        for first in 0..10 {
            contacts.push((first, first, if first < 2 { 100.0 } else { 10.0 }));
        }
        for first in 0..9 {
            contacts.push((first, first + 1, if first == 0 { 50.0 } else { 10.0 }));
        }
        contacts.extend([(0, 2, 10.0), (1, 3, 10.0)]);
        let map = ContactMap {
            bin: 100,
            contacts: contacts.clone(),
            ratio: false,
        };
        let bricks = rects(&scene(view(0, 1_000), &map, 40.0));
        assert_eq!(bricks.len(), contacts.len());
        for ((_, paint), (.., value)) in bricks.iter().zip(&contacts) {
            let wanted = match *value {
                100.0 => 100,
                50.0 => 62,
                _ => 20,
            };
            assert_eq!(*paint, Paint::Change(wanted), "{value}");
        }
    }

    #[test]
    fn rows_above_the_lane_are_cut() {
        let map = ContactMap {
            bin: 100,
            contacts: vec![(0, 7, 1.0), (0, 8, 1.0)],
            ratio: false,
        };
        assert_eq!(
            rects(&scene(view(0, 1_000), &map, 40.0)),
            [([35.0, 0.0, 10.0, 5.0], Paint::Change(100))]
        );
    }

    #[test]
    fn hits_find_the_pair_of_bins_under_the_point() {
        let map = ContactMap {
            bin: 100,
            contacts: vec![(2, 4, 20.0)],
            ratio: false,
        };
        let viewport = view(0, 1_000);
        assert_eq!(hit(viewport, &map, 40.0, 31.0, 27.0), Some((2, 4)));
        assert_eq!(hit(viewport, &map, 40.0, 1.0, 38.0), Some((0, 0)));
        assert_eq!(hit(viewport, &map, 40.0, 1.0, 33.0), None);
        assert_eq!(hit(viewport, &map, 40.0, 50.0, 41.0), None);
    }
}
