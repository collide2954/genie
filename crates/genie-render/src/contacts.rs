//! The lane of a Hi-C track: the contacts of the bins in view as a triangle over the diagonal,
//! with the contacts of near bins at the bottom. Each contact is a brick one bin wide and half a
//! bin high. Each row of bricks is half a bin higher and half a bin to the right of the row
//! under it, so the bricks stand for the diamonds of the rotated map.

use genie_core::contact::ContactMap;
use genie_core::signal::Summary;
use genie_core::{Region, Viewport};

use crate::scene::{Hue, Paint, Primitive};
use crate::signal::{self, Column, Style};

pub const HEIGHT: f32 = 160.0;
/// The part of the counts in view that is under the count that gets the full color.
const SHARE: f64 = 0.95;
/// The least part of the full color that the middle count in view gets.
const MIDDLE: f32 = 0.2;
/// The flattest curve of the colors, for the most skewed counts.
const FLATTEST: f32 = 0.35;
/// The log2 of the ratio of observed to expected contacts that gets the full color.
const RATIO: f32 = 2.0;
/// The contacts this many bins or more from the diagonal set the colors, so that the high
/// contacts of near bins do not leave the rest of the map pale.
const OFF_DIAGONAL: u32 = 2;

/// A contact in view: the place of its brick, its value, and the bins between its two bins.
struct Brick {
    left: f64,
    top: f32,
    value: f32,
    distance: u32,
}

/// The width of a bin in pixels.
fn bin_width(viewport: Viewport, map: &ContactMap) -> Option<f32> {
    if map.bin == 0 {
        return None;
    }
    let width = (viewport.to_px(map.bin as f64) - viewport.to_px(0.0)) as f32;
    (width > 0.0).then_some(width)
}

/// The contacts in view, as bricks of a lane of a height.
fn bricks(viewport: Viewport, map: &ContactMap, height: f32, rise: f32) -> Vec<Brick> {
    let visible = viewport.visible();
    let bin = map.bin as f64;
    map.contacts
        .iter()
        .filter_map(|&(x, y, value)| {
            let distance = y.saturating_sub(x);
            let top = height - (distance + 1) as f32 * rise;
            let left = (f64::from(x) + f64::from(y)) * bin / 2.0;
            let seen = left + bin >= visible.start() as f64 && left <= visible.end() as f64;
            (top + rise > 0.0 && seen).then_some(Brick {
                left,
                top,
                value,
                distance,
            })
        })
        .collect()
}

/// The count that gets the full color and the power of the curve, from the contacts in view
/// that are `OFF_DIAGONAL` bins or more from the diagonal, else from all contacts in view.
fn automatic(shown: &[Brick]) -> (f32, f32) {
    let far = shown.iter().filter(|brick| brick.distance >= OFF_DIAGONAL);
    let mut counts: Vec<f32> = far.map(|brick| brick.value).collect();
    if counts.is_empty() {
        counts = shown.iter().map(|brick| brick.value).collect();
    }
    colors(&mut counts)
}

/// The count that gets the full color in view when the track has no typed count, or nothing
/// for observed/expected ratios.
pub fn full_color(viewport: Viewport, map: &ContactMap, height: f32) -> Option<f32> {
    let width = bin_width(viewport, map)?;
    let shown = bricks(viewport, map, height, width / 2.0);
    (!map.ratio && !shown.is_empty()).then(|| automatic(&shown).0)
}

/// The rects of the contacts in view. A typed count gets the full color, with a straight line
/// of colors to it; else the contacts in view set the colors.
pub fn scene(
    viewport: Viewport,
    map: &ContactMap,
    height: f32,
    full: Option<f32>,
) -> Vec<Primitive> {
    let Some(width) = bin_width(viewport, map) else {
        return Vec::new();
    };
    let rise = width / 2.0;
    let shown = bricks(viewport, map, height, rise);
    let (scale, curve) = match full.filter(|count| *count > 0.0) {
        _ if map.ratio => (1.0, 1.0),
        Some(count) => (count, 1.0),
        None => automatic(&shown),
    };
    shown
        .into_iter()
        .filter_map(|brick| {
            let level = if map.ratio {
                ratio_level(brick.value)?
            } else if scale > 0.0 {
                ((brick.value / scale).clamp(0.0, 1.0).powf(curve) * 100.0).round() as i8
            } else {
                0
            };
            let y = brick.top.max(0.0);
            Some(Primitive::Rect {
                x: viewport.to_px(brick.left) as f32,
                y,
                width,
                height: brick.top + rise - y,
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

/// The height of the lane of a virtual 4C under the map.
pub const VIEWPOINT_HEIGHT: f32 = signal::HEIGHT;

/// The contacts of the bin of a viewpoint with each other bin of the map, as values along the
/// chromosome.
pub fn viewpoint_values(map: &ContactMap, viewpoint: u64) -> Vec<Summary> {
    if map.bin == 0 {
        return Vec::new();
    }
    let bin = u32::try_from(viewpoint / map.bin).unwrap_or(u32::MAX);
    map.row(bin)
        .into_iter()
        .filter(|(_, value)| value.is_finite())
        .map(|(other, value)| {
            let start = u64::from(other) * map.bin;
            Summary::single(Region::between(start, start + map.bin), value)
        })
        .collect()
}

fn viewpoint_columns(viewport: Viewport, map: &ContactMap, viewpoint: u64) -> Vec<Column> {
    signal::columns(viewport, &viewpoint_values(map, viewpoint))
}

/// The lane of a virtual 4C: a bar for the contacts of the viewpoint with the bins under each
/// pixel, and a line at the viewpoint.
pub fn viewpoint_lane(
    viewport: Viewport,
    map: &ContactMap,
    viewpoint: u64,
    hue: Hue,
) -> Vec<Primitive> {
    let columns = viewpoint_columns(viewport, map, viewpoint);
    let width = viewport.width() as f32;
    let mut primitives = signal::range(&columns).map_or_else(Vec::new, |scale| {
        signal::scene(&columns, scale, Style::Bars, hue, VIEWPOINT_HEIGHT, width)
    });
    let x = viewport.to_px(viewpoint as f64) as f32;
    primitives.push(Primitive::Rect {
        x: x - 0.5,
        y: 0.0,
        width: 1.0,
        height: VIEWPOINT_HEIGHT,
        paint: Paint::Accent,
    });
    primitives
}

/// The column of the lane of a virtual 4C under a point.
pub fn viewpoint_hit(
    viewport: Viewport,
    map: &ContactMap,
    viewpoint: u64,
    x: f32,
) -> Option<Column> {
    let columns = viewpoint_columns(viewport, map, viewpoint);
    signal::hit(&columns, x)
}

#[cfg(test)]
mod tests {
    use genie_core::contact::ContactMap;
    use genie_core::{Region, Viewport};

    use super::{
        VIEWPOINT_HEIGHT, full_color, hit, scene, viewpoint_hit, viewpoint_lane, viewpoint_values,
    };
    use crate::scene::{Hue, Paint, Primitive};

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
            rects(&scene(view(0, 1_000), &map, 40.0, None)),
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
        let paints: Vec<Paint> = rects(&scene(view(0, 1_000), &map, 40.0, None))
            .into_iter()
            .map(|(_, paint)| paint)
            .collect();
        assert_eq!(
            paints,
            [Paint::Change(100), Paint::Change(0), Paint::Change(-50)]
        );
    }

    /// The contacts of a bin with itself and with the next bin are much higher than the others.
    /// The contacts 2 bins or more from the diagonal set the full color, and the diagonal gets
    /// the full color.
    #[test]
    fn the_diagonal_does_not_set_the_full_color() {
        let mut contacts = Vec::new();
        for first in 0..10 {
            contacts.push((first, first, 1_000.0));
        }
        for first in 0..9 {
            contacts.push((first, first + 1, 500.0));
        }
        contacts.extend([(0, 2, 10.0), (1, 3, 20.0), (2, 4, 20.0), (3, 6, 20.0)]);
        let map = ContactMap {
            bin: 100,
            contacts: contacts.clone(),
            ratio: false,
        };
        let bricks = rects(&scene(view(0, 1_000), &map, 40.0, None));
        assert_eq!(bricks.len(), contacts.len());
        for ((_, paint), (.., value)) in bricks.iter().zip(&contacts) {
            let wanted = if *value < 15.0 { 50 } else { 100 };
            assert_eq!(*paint, Paint::Change(wanted), "{value}");
        }
        assert_eq!(full_color(view(0, 1_000), &map, 40.0), Some(20.0));
    }

    /// In a view of a whole chromosome, near bins have many more contacts than far bins, also off
    /// the diagonal. The colors then follow a curve that gives the middle count a fifth of the
    /// color.
    #[test]
    fn skewed_counts_bend_the_colors_so_the_middle_count_shows() {
        let mut contacts = Vec::new();
        for first in 0..10 {
            contacts.push((first, first, 1_000.0));
        }
        for first in 0..8 {
            contacts.push((first, first + 2, if first < 2 { 100.0 } else { 10.0 }));
        }
        contacts.extend([(0, 3, 50.0), (1, 4, 10.0), (2, 5, 10.0)]);
        let map = ContactMap {
            bin: 100,
            contacts: contacts.clone(),
            ratio: false,
        };
        let bricks = rects(&scene(view(0, 1_000), &map, 40.0, None));
        assert_eq!(bricks.len(), contacts.len());
        for ((_, paint), (.., value)) in bricks.iter().zip(&contacts) {
            let wanted = match *value {
                1_000.0 | 100.0 => 100,
                50.0 => 62,
                _ => 20,
            };
            assert_eq!(*paint, Paint::Change(wanted), "{value}");
        }
    }

    /// A typed count gets the full color, and the colors follow a straight line to it. Ratios
    /// keep their own range.
    #[test]
    fn a_typed_count_gives_straight_colors() {
        let map = ContactMap {
            bin: 100,
            contacts: vec![(0, 0, 1_000.0), (0, 2, 10.0), (1, 3, 20.0)],
            ratio: false,
        };
        let paints: Vec<Paint> = rects(&scene(view(0, 1_000), &map, 40.0, Some(40.0)))
            .into_iter()
            .map(|(_, paint)| paint)
            .collect();
        assert_eq!(
            paints,
            [Paint::Change(100), Paint::Change(25), Paint::Change(50)]
        );
        let ratios = ContactMap { ratio: true, ..map };
        assert_eq!(
            rects(&scene(view(0, 1_000), &ratios, 40.0, Some(40.0))),
            rects(&scene(view(0, 1_000), &ratios, 40.0, None))
        );
        assert_eq!(full_color(view(0, 1_000), &ratios, 40.0), None);
    }

    /// A virtual 4C: the contacts of the bin of a viewpoint with each other bin, as bars, with a
    /// line at the viewpoint.
    #[test]
    fn viewpoints_show_their_contacts_with_each_bin() {
        let map = ContactMap {
            bin: 100,
            contacts: vec![
                (2, 2, 10.0),
                (2, 5, 4.0),
                (5, 5, 8.0),
                (5, 7, 6.0),
                (7, 7, 9.0),
            ],
            ratio: false,
        };
        let values: Vec<(Region, f64)> = viewpoint_values(&map, 550)
            .iter()
            .map(|summary| (summary.span, summary.sum / f64::from(summary.count)))
            .collect();
        assert_eq!(
            values,
            [
                (Region::between(200, 300), 4.0),
                (Region::between(500, 600), 8.0),
                (Region::between(700, 800), 6.0)
            ]
        );
        let lane = viewpoint_lane(view(0, 1_000), &map, 550, Hue::Green);
        let found = rects(&lane);
        assert!(found.contains(&([54.5, 0.0, 1.0, VIEWPOINT_HEIGHT], Paint::Accent)));
        assert!(found.len() > 3, "{found:?}");
        let column = viewpoint_hit(view(0, 1_000), &map, 550, 75.0).unwrap();
        assert!((column.bin.mean() - 6.0).abs() < 1e-6);
        assert_eq!(viewpoint_hit(view(0, 1_000), &map, 550, 5.0), None);
    }

    #[test]
    fn rows_above_the_lane_are_cut() {
        let map = ContactMap {
            bin: 100,
            contacts: vec![(0, 7, 1.0), (0, 8, 1.0)],
            ratio: false,
        };
        assert_eq!(
            rects(&scene(view(0, 1_000), &map, 40.0, None)),
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
