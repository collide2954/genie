//! Copy number segment tracks: a row for each sample, blue for a loss, red for a gain and light
//! grey near 0.

use std::borrow::Cow;

use genie_core::Viewport;
use genie_core::segment::Segment;

use crate::features::Mode;
use crate::samples::Rows;
use crate::scene::{Align, Paint, Primitive};

/// The log2 values that get the full colors when the track has no range of its own.
pub const RANGE: (f32, f32) = (-1.5, 1.5);
/// The most samples that show with their names at first.
pub const NAMED: usize = 20;
const EXPANDED: f32 = 12.0;
/// The height that squished rows share, until each row is 1 px.
const FIT: f32 = 160.0;
const SQUISHED_MOST: f32 = 6.0;
const THINNEST: f32 = 1.0;
const NAME_SIZE: f32 = 9.0;
const NAME_INSET: f32 = 4.0;
const REACH: f64 = 1.0;

/// Expanded rows show the names, so they start with few samples.
pub fn fit(samples: usize) -> Mode {
    if samples <= NAMED {
        Mode::Expanded
    } else {
        Mode::Squished
    }
}

/// The height of one sample row: fixed when expanded, shared by all samples when squished.
pub fn row_height(mode: Mode, samples: usize) -> f32 {
    match mode {
        Mode::Expanded => EXPANDED,
        Mode::Squished => (FIT / samples.max(1) as f32)
            .floor()
            .clamp(THINNEST, SQUISHED_MOST),
        Mode::Collapsed => THINNEST,
    }
}

/// The paint of a value: the share of the full color on its side of 0.
pub fn change(value: f32, (low, high): (f32, f32)) -> Paint {
    let share = if value >= 0.0 {
        (value / high.max(f32::EPSILON)).min(1.0)
    } else {
        -(value / low.min(-f32::EPSILON)).min(1.0)
    };
    Paint::Change((share * 100.0).round() as i8)
}

/// The segments in the view as colored rows in the order of `rows`, with the sample names when
/// the rows are expanded, and the bands and strips of a sample table.
pub fn scene(
    viewport: Viewport,
    segments: &[Segment],
    samples: &[String],
    mode: Mode,
    range: (f32, f32),
    rows: &Rows,
) -> Vec<Primitive> {
    let row = row_height(mode, samples.len());
    let gap = if row >= 4.0 { 1.0 } else { 0.0 };
    let visible = viewport.visible();
    let px = |position: u64| viewport.to_px(position as f64) as f32;
    let mut primitives: Vec<Primitive> = segments
        .iter()
        .filter(|segment| segment.span.overlaps(visible))
        .filter_map(|segment| {
            let left = px(segment.span.start());
            Some(Primitive::Rect {
                x: left,
                y: rows.top(*rows.place.get(segment.sample)?, row),
                width: (px(segment.span.end()) - left).max(1.0),
                height: row - gap,
                paint: change(segment.value, range),
            })
        })
        .collect();
    if mode == Mode::Expanded {
        let right = viewport.width() as f32 - NAME_INSET;
        primitives.extend(
            rows.members
                .iter()
                .enumerate()
                .filter_map(|(place, members)| {
                    Some(Primitive::Label {
                        x: right,
                        y: rows.top(place, row) + (row - gap) / 2.0,
                        content: Cow::Owned(samples.get(*members.first()?)?.clone()),
                        size: NAME_SIZE,
                        align: Align::End,
                    })
                }),
        );
    }
    primitives.extend(rows.marks(row, gap));
    primitives
}

/// The segment under a point: the row gives the sample, and the place gives the segment.
pub fn hit(
    viewport: Viewport,
    segments: &[Segment],
    samples: usize,
    mode: Mode,
    rows: &Rows,
    (x, y): (f32, f32),
) -> Option<usize> {
    let place = rows.at(y, row_height(mode, samples))?;
    let sample = *rows.members.get(place)?.first()?;
    let position = viewport.to_bp(f64::from(x));
    let reach = REACH * viewport.scale();
    segments.iter().position(|segment| {
        segment.sample == sample
            && segment.span.start() as f64 - reach <= position
            && position <= segment.span.end() as f64 + reach
    })
}

#[cfg(test)]
mod tests {
    use genie_core::samples::{Arrangement, Group, Tint};
    use genie_core::segment::Segment;
    use genie_core::{Region, Viewport};

    use super::{change, fit, hit, row_height, scene};
    use crate::features::Mode;
    use crate::samples::{GROUP_GAP, Rows, STRIP};
    use crate::scene::{Paint, Primitive};

    fn segment(sample: usize, start: u64, end: u64, value: f32) -> Segment {
        Segment {
            span: Region::between(start, end),
            sample,
            value,
            markers: None,
        }
    }

    fn view() -> Viewport {
        Viewport::new(10_000, 1_000.0).show(Region::between(0, 1_000))
    }

    #[test]
    fn values_take_the_share_of_their_full_color() {
        let range = (-1.5, 1.5);
        assert_eq!(change(0.0, range), Paint::Change(0));
        assert_eq!(change(0.75, range), Paint::Change(50));
        assert_eq!(change(-3.0, range), Paint::Change(-100));
        assert_eq!(change(9.0, (-1.0, 2.0)), Paint::Change(100));
        assert_eq!(change(-0.5, (-1.0, 2.0)), Paint::Change(-50));
    }

    #[test]
    fn rows_squish_to_fit_many_samples() {
        assert_eq!(fit(20), Mode::Expanded);
        assert_eq!(fit(21), Mode::Squished);
        assert!((row_height(Mode::Expanded, 3) - 12.0).abs() < f32::EPSILON);
        assert!((row_height(Mode::Squished, 30) - 5.0).abs() < f32::EPSILON);
        assert!((row_height(Mode::Squished, 500) - 1.0).abs() < f32::EPSILON);
        assert!((row_height(Mode::Squished, 2) - 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn each_sample_gets_a_row_and_expanded_rows_name_it() {
        let segments = [segment(0, 100, 300, 1.5), segment(1, 200, 900, -0.75)];
        let names = ["tumor".to_owned(), "normal".to_owned()];
        let rows = Rows::plain(2, 1);
        let drawn = scene(
            view(),
            &segments,
            &names,
            Mode::Expanded,
            (-1.5, 1.5),
            &rows,
        );
        let rects: Vec<(f32, f32, f32, Paint)> = drawn
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect {
                    x, y, width, paint, ..
                } => Some((*x, *y, *width, *paint)),
                _ => None,
            })
            .collect();
        assert_eq!(
            rects,
            [
                (100.0, 0.0, 200.0, Paint::Change(100)),
                (200.0, 12.0, 700.0, Paint::Change(-50))
            ]
        );
        let labels: Vec<(String, f32)> = drawn
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Label { content, x, .. } => Some((content.to_string(), *x)),
                _ => None,
            })
            .collect();
        assert_eq!(
            labels,
            [("tumor".to_owned(), 996.0), ("normal".to_owned(), 996.0)]
        );
        let squished = scene(
            view(),
            &segments,
            &names,
            Mode::Squished,
            (-1.5, 1.5),
            &rows,
        );
        assert!(
            squished
                .iter()
                .all(|primitive| matches!(primitive, Primitive::Rect { .. }))
        );
        let at = |x, y| hit(view(), &segments, 2, Mode::Expanded, &rows, (x, y));
        assert_eq!(at(250.0, 5.0), Some(0));
        assert_eq!(at(250.0, 17.0), Some(1));
        assert_eq!(at(950.0, 17.0), None);
        assert_eq!(at(250.0, 40.0), None);
    }

    #[test]
    fn grouped_samples_sit_under_the_band_of_their_group() {
        let segments = [segment(0, 100, 300, 1.5), segment(1, 200, 900, -0.75)];
        let names = ["tumor".to_owned(), "normal".to_owned()];
        let arrangement = Arrangement {
            order: vec![1, 0],
            groups: vec![
                Group {
                    name: "blood".to_owned(),
                    first: 0,
                    size: 1,
                },
                Group {
                    name: "breast".to_owned(),
                    first: 1,
                    size: 1,
                },
            ],
            tints: vec![Tint::Value(0), Tint::Other],
        };
        let rows = Rows::new(&arrangement, 1);
        let drawn = scene(
            view(),
            &segments,
            &names,
            Mode::Expanded,
            (-1.5, 1.5),
            &rows,
        );
        let rects: Vec<(f32, f32, Paint)> = drawn
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect { x, y, paint, .. } if *x > STRIP => Some((*x, *y, *paint)),
                _ => None,
            })
            .collect();
        assert_eq!(
            rects,
            [
                (100.0, 2.0 * GROUP_GAP + 12.0, Paint::Change(100)),
                (200.0, GROUP_GAP, Paint::Change(-50))
            ]
        );
        let labels: Vec<(String, f32)> = drawn
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Label { content, y, .. } => Some((content.to_string(), *y)),
                _ => None,
            })
            .collect();
        assert_eq!(
            labels,
            [
                ("normal".to_owned(), GROUP_GAP + 5.5),
                ("tumor".to_owned(), 2.0 * GROUP_GAP + 17.5)
            ]
        );
        let strips = drawn
            .iter()
            .filter(|primitive| matches!(primitive, Primitive::Rect { x, .. } if *x < 1.0))
            .count();
        let bands = drawn
            .iter()
            .filter(|primitive| matches!(primitive, Primitive::Text { .. }))
            .count();
        assert_eq!((strips, bands), (2, 2));
        let at = |y| hit(view(), &segments, 2, Mode::Expanded, &rows, (250.0, y));
        assert_eq!(at(5.0), None);
        assert_eq!(at(GROUP_GAP + 5.0), Some(1));
        assert_eq!(at(2.0 * GROUP_GAP + 17.0), Some(0));
    }
}
