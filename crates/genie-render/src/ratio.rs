//! The lane of a depth ratio track: a dot for the log2 ratio of the depths of each bin, blue
//! under 0 and vermillion over 0 as copy number tracks, and a line for the mean of each segment.
//! A line marks 0, and the labels at the right give the range.

use std::borrow::Cow;
use std::collections::HashSet;

use genie_core::Viewport;
use genie_core::depth::{Bin, Segment};

use crate::scene::{Align, LABEL_INSET, Paint, Primitive};

/// The height of the lane.
pub const HEIGHT: f32 = 120.0;
/// The log2 ratio at the top of the lane, and below 0 at the bottom, without a typed range.
pub const RANGE: f32 = 2.0;
const DOT: f32 = 3.0;
/// The space above the highest dots and under the lowest dots.
const INSET: f32 = 6.0;
/// The thickness of the line of a segment.
const LINE: f32 = 2.0;
const LABEL_SIZE: f32 = 10.0;
/// The farthest that the pointer can be from a dot or a line to show its card.
const REACH: f32 = 3.0;

/// What the pointer is on: a bin or a segment, by their places in their lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Bin(usize),
    Segment(usize),
}

/// The height of a log2 ratio in the lane, on a range of ratios from -range to range.
fn level(ratio: f64, range: f32, height: f32) -> f32 {
    let half = height / 2.0 - INSET;
    height / 2.0 - (ratio as f32 / range).clamp(-1.0, 1.0) * half
}

/// The middle of a span, across.
fn middle(viewport: Viewport, bin: &Bin) -> f32 {
    let center = (bin.span.start() + bin.span.end()) as f64 / 2.0;
    viewport.to_px(center) as f32
}

/// The text of a range label, as +2, 0 or −2.
fn label(value: f32) -> String {
    let text = format!("{}", value.abs());
    if value > 0.0 {
        format!("+{text}")
    } else if value < 0.0 {
        format!("\u{2212}{text}")
    } else {
        text
    }
}

/// The lane of the bins and the segments in view, on a range of log2 ratios from -range to
/// range.
pub fn scene(
    viewport: Viewport,
    (bins, segments): (&[Bin], &[Segment]),
    range: f32,
    height: f32,
) -> Vec<Primitive> {
    let width = viewport.width() as f32;
    let range = if range > 0.0 { range } else { RANGE };
    let mut primitives = vec![Primitive::Rect {
        x: 0.0,
        y: (height / 2.0).floor(),
        width,
        height: 1.0,
        paint: Paint::Line,
    }];
    let mut drawn = HashSet::new();
    for bin in bins {
        let x = middle(viewport, bin) - DOT / 2.0;
        let y = level(bin.ratio, range, height) - DOT / 2.0;
        if x + DOT < 0.0 || x > width || !drawn.insert((x.round() as i32, y.round() as i32)) {
            continue;
        }
        let shade = (bin.ratio as f32 / range).clamp(-1.0, 1.0) * 100.0;
        primitives.push(Primitive::Rect {
            x,
            y,
            width: DOT,
            height: DOT,
            paint: Paint::Change(shade.round() as i8),
        });
    }
    for segment in segments {
        let left = (viewport.to_px(segment.span.start() as f64) as f32).max(-1.0);
        let right = (viewport.to_px(segment.span.end() as f64) as f32).min(width + 1.0);
        if right < 0.0 || left > width {
            continue;
        }
        primitives.push(Primitive::Rect {
            x: left,
            y: level(segment.mean, range, height) - LINE / 2.0,
            width: (right - left).max(1.0),
            height: LINE,
            paint: Paint::Text,
        });
    }
    for (value, y) in [
        (range, INSET + LABEL_SIZE / 2.0),
        (-range, height - INSET - LABEL_SIZE / 2.0),
    ] {
        primitives.push(Primitive::Label {
            x: width - LABEL_INSET,
            y,
            content: Cow::Owned(label(value)),
            size: LABEL_SIZE,
            align: Align::End,
        });
    }
    primitives
}

/// The segment whose line is under the pointer, else the bin whose dot is nearest to it, when
/// the pointer is at most `REACH` pixels from the line or the dot.
pub fn hit(
    viewport: Viewport,
    (bins, segments): (&[Bin], &[Segment]),
    (range, height): (f32, f32),
    (x, y): (f32, f32),
) -> Option<Hit> {
    let range = if range > 0.0 { range } else { RANGE };
    let at = viewport.to_bp(f64::from(x)).max(0.0) as u64;
    let line = segments.iter().position(|segment| {
        segment.span.start() <= at
            && at < segment.span.end()
            && (level(segment.mean, range, height) - y).abs() <= REACH + LINE / 2.0
    });
    if let Some(index) = line {
        return Some(Hit::Segment(index));
    }
    bins.iter()
        .enumerate()
        .map(|(index, bin)| {
            let across = (middle(viewport, bin) - x).abs();
            let down = (level(bin.ratio, range, height) - y).abs();
            (index, across.max(down))
        })
        .filter(|(_, distance)| *distance <= REACH + DOT / 2.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| Hit::Bin(index))
}

#[cfg(test)]
mod tests {
    use genie_core::depth::{Bin, Segment};
    use genie_core::{Region, Viewport};

    use super::{HEIGHT, Hit, hit, label, scene};
    use crate::scene::{Paint, Primitive};

    fn view() -> Viewport {
        Viewport::new(1_000_000, 100.0).show(Region::between(0, 1_000))
    }

    fn bin(start: u64, ratio: f64) -> Bin {
        Bin {
            span: Region::between(start, start + 100),
            first: 1.0,
            second: 1.0,
            ratio,
        }
    }

    fn rects(primitives: &[Primitive]) -> Vec<(f32, f32, f32, f32, Paint)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect {
                    x,
                    y,
                    width,
                    height,
                    paint,
                } => Some((*x, *y, *width, *height, *paint)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn bins_are_dots_and_segments_are_lines_over_a_line_at_zero() {
        let bins = [bin(0, 0.0), bin(100, 2.0), bin(200, -1.0), bin(300, -9.0)];
        let segments = [Segment {
            span: Region::between(0, 400),
            bins: 4,
            mean: 1.0,
        }];
        let found = rects(&scene(view(), (&bins, &segments), 2.0, HEIGHT));
        assert_eq!(found[0], (0.0, 60.0, 100.0, 1.0, Paint::Line));
        assert_eq!(found[1], (3.5, 58.5, 3.0, 3.0, Paint::Change(0)));
        assert_eq!(found[2], (13.5, 4.5, 3.0, 3.0, Paint::Change(100)));
        assert_eq!(found[3], (23.5, 85.5, 3.0, 3.0, Paint::Change(-50)));
        assert_eq!(found[4], (33.5, 112.5, 3.0, 3.0, Paint::Change(-100)));
        assert_eq!(found[5], (0.0, 32.0, 40.0, 2.0, Paint::Text));
        let texts: Vec<String> = scene(view(), (&bins, &segments), 2.0, HEIGHT)
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Label { content, .. } => Some(content.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["+2", "\u{2212}2"]);
        assert_eq!(label(0.0), "0");
        assert_eq!(label(1.5), "+1.5");
    }

    #[test]
    fn dots_on_one_pixel_show_once() {
        let bins = [bin(0, 0.5), bin(1, 0.5), bin(2, 0.5)];
        let wide = Viewport::new(1_000_000, 100.0).show(Region::between(0, 100_000));
        let dots = rects(&scene(wide, (&bins, &[]), 2.0, HEIGHT)).len() - 1;
        assert_eq!(dots, 1);
    }

    #[test]
    fn hits_find_the_segment_under_the_pointer_or_the_nearest_dot() {
        let bins = [bin(0, 0.0), bin(500, 1.0)];
        let segments = [Segment {
            span: Region::between(0, 400),
            bins: 4,
            mean: -1.0,
        }];
        let data = (&bins[..], &segments[..]);
        let size = (2.0, HEIGHT);
        assert_eq!(hit(view(), data, size, (20.0, 87.0)), Some(Hit::Segment(0)));
        assert_eq!(hit(view(), data, size, (5.0, 60.0)), Some(Hit::Bin(0)));
        assert_eq!(hit(view(), data, size, (55.0, 33.0)), Some(Hit::Bin(1)));
        assert_eq!(hit(view(), data, size, (55.0, 60.0)), None);
        assert_eq!(hit(view(), data, size, (80.0, 60.0)), None);
    }
}
