//! Sashimi plots: the coverage of a read track as an area in the color of the track, and its
//! splice junctions as arcs above and below the area, each with its number of reads.

use std::borrow::Cow;

use genie_core::Viewport;
use genie_core::pileup::{Coverage, Junction};

use crate::scene::{Align, Font, Hue, LABEL_INSET, Paint, Primitive};

/// The height of the band of arcs above the coverage, and of the band below it.
pub const ARCS: f32 = 40.0;
pub const COVERAGE: f32 = 60.0;
pub const HEIGHT: f32 = 2.0 * ARCS + COVERAGE;
const COUNT_SIZE: f32 = 9.0;
const COUNT_GAP: f32 = 5.0;
const LOW_RISE: f32 = 8.0;
const HIGH_RISE: f32 = ARCS - COUNT_SIZE - COUNT_GAP - 2.0;
const RANGE_SIZE: f32 = 10.0;
const REACH: f32 = 2.0;

/// The bars of the coverage in view: their left edge, their width and their depth. A bar is a
/// base, or the deepest base of a pixel when a pixel shows more than one base.
fn bars(viewport: Viewport, coverage: &Coverage) -> Vec<(f32, f32, u32)> {
    let Some(shown) = viewport.visible().intersection(coverage.region()) else {
        return Vec::new();
    };
    let pixels = 1.0 / viewport.scale();
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    if pixels >= 1.0 {
        return (shown.start()..shown.end())
            .map(|at| (px(at), pixels as f32, coverage.depth(at)))
            .filter(|(_, _, depth)| *depth > 0)
            .collect();
    }
    let first = px(shown.start()).floor().max(0.0) as u64;
    let last = px(shown.end()).ceil().min(viewport.width() as f32) as u64;
    (first..last)
        .filter_map(|column| {
            let from = viewport.to_bp(column as f64).floor().max(0.0) as u64;
            let to = viewport.to_bp((column + 1) as f64).ceil() as u64;
            let depth = (from.max(shown.start())..to.min(shown.end()))
                .map(|at| coverage.depth(at))
                .max()
                .unwrap_or(0);
            (depth > 0).then_some((column as f32, 1.0, depth))
        })
        .collect()
}

/// The deepest coverage in view.
pub fn deepest(viewport: Viewport, coverage: &Coverage) -> u32 {
    bars(viewport, coverage)
        .into_iter()
        .map(|(_, _, depth)| depth)
        .max()
        .unwrap_or(0)
}

fn arc_width(count: u32) -> f32 {
    (1.0 + (count as f32).log2() / 2.0).clamp(1.0, 3.0)
}

/// The arcs in view of the junctions with at least `least` reads: the place of each junction in
/// the list, and the ends, the base and the rise of its arc. In the order of the starts, the
/// first junction is above the coverage, the second below it and so on, also out of view, so
/// that an arc keeps its side when the view moves.
fn arcs(viewport: Viewport, junctions: &[Junction], least: u32) -> Vec<(usize, [f32; 4])> {
    let mut kept: Vec<usize> = (0..junctions.len())
        .filter(|&at| junctions[at].count >= least)
        .collect();
    kept.sort_by_key(|&at| (junctions[at].span.start(), junctions[at].span.end()));
    let most = kept
        .iter()
        .map(|&at| junctions[at].count)
        .max()
        .unwrap_or(1);
    let visible = viewport.visible();
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    kept.into_iter()
        .enumerate()
        .filter(|(_, at)| {
            let span = junctions[*at].span;
            span.end() >= visible.start() && span.start() <= visible.end()
        })
        .map(|(order, at)| {
            let junction = &junctions[at];
            let (from, to) = (px(junction.span.start()), px(junction.span.end()));
            let share = junction.count as f32 / most.max(1) as f32;
            let rise =
                (LOW_RISE + (HIGH_RISE - LOW_RISE) * share).min(((to - from) / 2.0).max(4.0));
            let (base, rise) = if order % 2 == 0 {
                (ARCS, rise)
            } else {
                (ARCS + COVERAGE, -rise)
            };
            (at, [from, to, base, rise])
        })
        .collect()
}

/// The coverage up to `top` reads, and the arcs of the junctions with at least `least` reads.
pub fn scene(
    viewport: Viewport,
    coverage: &Coverage,
    junctions: &[Junction],
    (hue, least, top): (Hue, u32, u32),
) -> Vec<Primitive> {
    let top = top.max(1);
    let bars = bars(viewport, coverage);
    let mut primitives: Vec<Primitive> = bars
        .iter()
        .map(|&(x, width, depth)| {
            let size = depth.min(top) as f32 * COVERAGE / top as f32;
            Primitive::Rect {
                x,
                y: ARCS + COVERAGE - size,
                width,
                height: size,
                paint: Paint::Tint(hue),
            }
        })
        .collect();
    if !bars.is_empty() {
        primitives.push(Primitive::Label {
            x: viewport.width() as f32 - LABEL_INSET,
            y: ARCS + RANGE_SIZE / 2.0 + 1.0,
            content: Cow::Owned(format!("0-{top}")),
            size: RANGE_SIZE,
            align: Align::End,
        });
    }
    let shown = arcs(viewport, junctions, least);
    for &(at, [from, to, base, rise]) in &shown {
        primitives.push(Primitive::Arc {
            from,
            to,
            base,
            rise,
            width: arc_width(junctions[at].count),
            paint: Paint::Hue(hue),
        });
    }
    let mut placed: Vec<(f32, f32, f32)> = Vec::new();
    for (at, [from, to, base, rise]) in shown {
        let words = junctions[at].count.to_string();
        let half = words.len() as f32 * COUNT_SIZE * 0.3;
        let peak = base - rise;
        let y = if rise > 0.0 {
            peak - COUNT_GAP
        } else {
            peak + COUNT_GAP
        };
        let x = f32::midpoint(from, to);
        let crowded = placed.iter().any(|&(left, top, other)| {
            (left - x).abs() < half + other + 2.0 && (top - y).abs() < COUNT_SIZE
        });
        if crowded {
            continue;
        }
        placed.push((x, y, half));
        primitives.push(Primitive::Text {
            x,
            y,
            content: Cow::Owned(words),
            size: COUNT_SIZE,
            paint: Paint::Text,
            font: Font::Ui,
            align: Align::Center,
        });
    }
    primitives
}

/// The junction of the arc under the pointer.
pub fn hit(
    viewport: Viewport,
    junctions: &[Junction],
    least: u32,
    x: f32,
    y: f32,
) -> Option<usize> {
    arcs(viewport, junctions, least)
        .into_iter()
        .filter_map(|(at, [from, to, base, rise])| {
            if to <= from || x < from || x > to {
                return None;
            }
            let t = (x - from) / (to - from);
            let curve = base - 4.0 * rise * t * (1.0 - t);
            let distance = (curve - y).abs();
            let reach = REACH.max(arc_width(junctions[at].count));
            (distance <= reach).then_some((at, distance))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(at, _)| at)
}

#[cfg(test)]
mod tests {
    use genie_core::alignment::{Alignment, Op, Tags};
    use genie_core::pileup::{Coverage, Junction};
    use genie_core::{Region, Strand, Viewport};

    use super::{ARCS, COVERAGE, HEIGHT, deepest, hit, scene};
    use crate::scene::{Hue, Paint, Primitive};

    fn view(start: u64, end: u64) -> Viewport {
        Viewport::new(1_000_000, 100.0).show(Region::between(start, end))
    }

    fn read(start: u64, cigar: Vec<(Op, u32)>) -> Alignment {
        let length = cigar
            .iter()
            .filter(|(op, _)| *op == Op::Match)
            .map(|(_, length)| *length as usize)
            .sum();
        Alignment {
            name: format!("r{start}"),
            start,
            flags: 0,
            mapq: 60,
            cigar,
            bases: vec![b'A'; length],
            qualities: vec![30; length],
            mate: None,
            tags: Tags::default(),
        }
    }

    /// Two reads over 10-30, and a spliced read that covers 10-30 again and 60-80.
    fn covered() -> Coverage {
        let mut coverage = Coverage::new(Region::between(0, 100));
        coverage.add(&read(10, vec![(Op::Match, 20)]));
        coverage.add(&read(10, vec![(Op::Match, 20)]));
        coverage.add(&read(
            10,
            vec![(Op::Match, 20), (Op::Skip, 30), (Op::Match, 20)],
        ));
        coverage
    }

    fn junction(start: u64, end: u64, count: u32) -> Junction {
        Junction {
            span: Region::between(start, end),
            count,
            strand: Strand::Forward,
        }
    }

    fn arcs(primitives: &[Primitive]) -> Vec<(f32, f32, f32, f32, Paint)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Arc {
                    from,
                    to,
                    base,
                    rise,
                    paint,
                    ..
                } => Some((*from, *to, *base, *rise, *paint)),
                _ => None,
            })
            .collect()
    }

    fn texts(primitives: &[Primitive]) -> Vec<(String, f32, f32)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text { x, y, content, .. } => Some((content.to_string(), *x, *y)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn coverage_fills_the_middle_band_up_to_the_deepest_base() {
        let coverage = covered();
        assert_eq!(deepest(view(0, 100), &coverage), 3);
        assert_eq!(deepest(view(50, 100), &coverage), 1);
        let primitives = scene(view(0, 100), &coverage, &[], (Hue::Green, 1, 3));
        let bars: Vec<(f32, f32, f32)> = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect {
                    x,
                    y,
                    height,
                    paint,
                    ..
                } if *paint == Paint::Tint(Hue::Green) => Some((*x, *y, *height)),
                _ => None,
            })
            .collect();
        assert_eq!(bars.len(), 40);
        assert_eq!(bars[0], (10.0, ARCS, COVERAGE));
        assert_eq!(
            bars[20],
            (60.0, ARCS + COVERAGE * 2.0 / 3.0, COVERAGE / 3.0)
        );
        let shared = scene(view(0, 100), &coverage, &[], (Hue::Green, 1, 6));
        let range = shared.iter().find_map(|primitive| match primitive {
            Primitive::Label { content, .. } => Some(content.to_string()),
            _ => None,
        });
        assert_eq!(range.as_deref(), Some("0-6"));
        assert!((HEIGHT - 140.0).abs() < f32::EPSILON);
    }

    #[test]
    fn arcs_alternate_above_and_below_with_their_counts() {
        let junctions = [
            junction(60, 90, 1),
            junction(30, 60, 12),
            junction(10, 40, 2),
            junction(500, 600, 50),
        ];
        let primitives = scene(view(0, 100), &covered(), &junctions, (Hue::Green, 2, 3));
        let found = arcs(&primitives);
        assert_eq!(found.len(), 2);
        let (above, below) = (found[0], found[1]);
        assert_eq!((above.0, above.1, above.2), (10.0, 40.0, ARCS));
        assert!(above.3 > 0.0);
        assert_eq!((below.0, below.1, below.2), (30.0, 60.0, ARCS + COVERAGE));
        assert!(below.3 < 0.0);
        assert_eq!(above.4, Paint::Hue(Hue::Green));
        let counts = texts(&primitives);
        assert_eq!(counts.len(), 2);
        assert_eq!((counts[0].0.as_str(), counts[0].1), ("2", 25.0));
        assert!(counts[0].2 < ARCS - above.3);
        assert_eq!((counts[1].0.as_str(), counts[1].1), ("12", 45.0));
        assert!(counts[1].2 > ARCS + COVERAGE - below.3);
        assert!(counts.iter().all(|(_, _, y)| *y > 0.0 && *y < HEIGHT));
        let at = |x, y| hit(view(0, 100), &junctions, 2, x, y);
        assert_eq!(at(25.0, ARCS - above.3), Some(2));
        assert_eq!(at(45.0, ARCS + COVERAGE - below.3), Some(1));
        assert_eq!(at(75.0, ARCS - 4.0), None);
        assert_eq!(at(25.0, ARCS + COVERAGE / 2.0), None);
    }
}
