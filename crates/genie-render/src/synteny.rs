//! The synteny view of a dot plot: the target sequences in a row at the top, the query sequences
//! in a row at the bottom, and a band between the two parts of each alignment. The band of a
//! reverse alignment crosses over itself.

use std::borrow::Cow;
use std::collections::HashSet;

use genie_core::Region;
use genie_core::dotplot::{Hit, Plot, Sequence, Window};

use crate::dotplot::{self, Coloring, Marks, level};
use crate::scene::{Align, Font, Hue, Paint, Primitive};

/// The space at the left and at the right of the rows.
const EDGE: f32 = 8.0;
/// The height of the names over the top row and under the bottom row.
const NAMES: f32 = 16.0;
/// The height of a row of sequences.
const ROW: f32 = 8.0;
const SIZE: f32 = 10.0;
const CHARACTER: f32 = 6.0;
/// The least width of an end of a band, so that a short alignment stays in sight.
const THIN: f32 = 1.0;
/// The farthest distance of the pointer from a band that still picks the band.
const REACH: f32 = 4.0;
/// The least width of a drag that zooms in, in pixels.
const LEAST: f32 = 4.0;
/// The least size of a window, in bases.
const SMALLEST: u64 = 20;
/// The share of the parts that align that a zoom adds at each side of the other row.
const MARGIN: f64 = 0.02;
/// A zoom leaves out the parts of the other row at its two ends that hold one aligned base in
/// this number.
const TRIM: u64 = 20;
/// The two greys of the sequences of a row, one after the other.
const STAINS: [u8; 2] = [30, 50];

/// The two rows of the view in a scene, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rows {
    pub left: f32,
    pub width: f32,
    /// The top of the row of the targets.
    pub top: f32,
    /// The top of the row of the queries.
    pub bottom: f32,
}

impl Rows {
    /// The rows of a scene of a size, in pixels.
    pub fn of((width, height): (f32, f32)) -> Self {
        Self {
            left: EDGE,
            width: (width - 2.0 * EDGE).max(1.0),
            top: NAMES,
            bottom: (height - NAMES - ROW).max(NAMES + 2.0 * ROW),
        }
    }

    fn x(self, region: Region, place: f64) -> f32 {
        let share = (place - region.start() as f64) / region.len().max(1) as f64;
        self.left + (share * f64::from(self.width)) as f32
    }

    fn clamped(self, region: Region, place: f64) -> f32 {
        self.x(region, place)
            .clamp(self.left, self.left + self.width)
    }

    /// The place in bases at a place of a row that shows a region.
    fn place(self, region: Region, x: f32) -> u64 {
        let share = f64::from(((x - self.left) / self.width).clamp(0.0, 1.0));
        region.start() + (share * region.len() as f64).round() as u64
    }

    /// True for a place in the top half of the view, nearer to the row of the targets.
    fn is_top(self, y: f32) -> bool {
        y < f32::midpoint(self.top + ROW, self.bottom)
    }
}

/// The two ends of a part of a row, at least [`THIN`] apart, in their order.
fn widened(first: f32, second: f32) -> (f32, f32) {
    if (second - first).abs() >= THIN {
        return (first, second);
    }
    let middle = f32::midpoint(first, second);
    if first <= second {
        (middle - THIN / 2.0, middle + THIN / 2.0)
    } else {
        (middle + THIN / 2.0, middle - THIN / 2.0)
    }
}

/// The corners of the band of the part of an alignment that has both ends in the window, in the
/// order that draws it: the two ends of its part of the top row, then the place on the bottom
/// row of its last base on the target, then the place of its first base. None when no part of
/// the alignment has both ends in the window.
fn band(plot: &Plot, window: Window, rows: Rows, hit: &Hit) -> Option<[(f32, f32); 4]> {
    let ((x0, y0), (x1, y1)) = dotplot::clip(plot, window, hit)?;
    let (first, last) = widened(
        rows.clamped(window.across, x0),
        rows.clamped(window.across, x1),
    );
    let (start, end) = widened(rows.clamped(window.down, y0), rows.clamped(window.down, y1));
    let (upper, lower) = (rows.top + ROW, rows.bottom);
    Some([(first, upper), (last, upper), (end, lower), (start, lower)])
}

/// The color of the band of an alignment, a number for it that is the same for the same color,
/// and the color of the band under the pointer.
fn paint(hit: &Hit, marks: &Marks) -> (Paint, u8, Paint) {
    match marks.coloring {
        Coloring::Strand if hit.reverse => {
            (Paint::Tint(Hue::Vermillion), 0, Paint::Hue(Hue::Vermillion))
        }
        Coloring::Strand => (Paint::Tint(Hue::Blue), 1, Paint::Hue(Hue::Blue)),
        Coloring::Identity => {
            let level = level(hit.identity(), marks.identities);
            (Paint::Level(level), level, Paint::Accent)
        }
    }
}

fn text(x: f32, y: f32, content: String) -> Primitive {
    Primitive::Text {
        x,
        y,
        content: Cow::Owned(content),
        size: SIZE,
        paint: Paint::Muted,
        font: Font::Ui,
        align: Align::Center,
    }
}

/// The name of a sequence that fits in a width: the whole name, or the name without "chr".
fn fitted(name: &str, room: f32) -> Option<String> {
    let wide = |name: &str| name.chars().count() as f32 * CHARACTER;
    if wide(name) <= room {
        return Some(name.to_owned());
    }
    let short = name.strip_prefix("chr").filter(|rest| !rest.is_empty())?;
    (wide(short) <= room).then(|| short.to_owned())
}

/// A row of sequences in their two greys, with the names that fit at `names`.
fn row(
    sequences: &[Sequence],
    region: Region,
    rows: Rows,
    (y, names): (f32, f32),
) -> Vec<Primitive> {
    let mut found = Vec::new();
    for (index, sequence) in sequences.iter().enumerate() {
        let span = Region::between(sequence.offset, sequence.offset + sequence.length);
        let Some(shown) = span.intersection(region) else {
            continue;
        };
        let from = rows.x(region, shown.start() as f64);
        let to = rows.x(region, shown.end() as f64);
        found.push(Primitive::Rect {
            x: from,
            y,
            width: (to - from).max(THIN),
            height: ROW,
            paint: Paint::Stain(STAINS[index % 2]),
        });
        if let Some(name) = fitted(&sequence.name, to - from - 4.0) {
            found.push(text(f32::midpoint(from, to), names, name));
        }
    }
    found
}

/// The synteny view of the part of a plot in a window, for a space of `size` pixels: the targets
/// of `window.across` in the top row and the queries of `window.down` in the bottom row. Bands
/// on the same pixels with the same color give one band.
pub fn scene(plot: &Plot, window: Window, size: (f32, f32), marks: Marks) -> Vec<Primitive> {
    let rows = Rows::of(size);
    let mut primitives = row(&plot.targets, window.across, rows, (rows.top, NAMES / 2.0));
    let names = rows.bottom + ROW + NAMES / 2.0;
    primitives.extend(row(&plot.queries, window.down, rows, (rows.bottom, names)));
    let half = |value: f32| (value * 2.0).round() as i32;
    let mut seen = HashSet::new();
    for hit in &plot.hits {
        let Some(corners) = band(plot, window, rows, hit) else {
            continue;
        };
        let (paint, tone, _) = paint(hit, &marks);
        let key = corners.map(|(x, _)| half(x));
        if seen.insert((key, tone)) {
            primitives.push(Primitive::Ribbon { corners, paint });
        }
    }
    if let Some(hit) = marks.chosen.and_then(|chosen| plot.hits.get(chosen))
        && let Some(corners) = band(plot, window, rows, hit)
    {
        let (_, _, paint) = paint(hit, &marks);
        primitives.push(Primitive::Ribbon { corners, paint });
    }
    if let Some((first, second)) = marks.dragged {
        let (y, height) = if rows.is_top(first.1) {
            (0.0, rows.top + ROW)
        } else {
            (rows.bottom, size.1 - rows.bottom)
        };
        primitives.push(Primitive::Rect {
            x: first.0.min(second.0),
            y,
            width: (first.0 - second.0).abs(),
            height,
            paint: Paint::Selection,
        });
    }
    primitives
}

/// True when a point is inside a band, also inside the two halves of a crossed band.
fn contains(corners: &[(f32, f32); 4], (x, y): (f32, f32)) -> bool {
    let mut inside = false;
    for (index, &(x1, y1)) in corners.iter().enumerate() {
        let (x2, y2) = corners[(index + 1) % corners.len()];
        if (y1 > y) != (y2 > y) && x < x1 + (y - y1) / (y2 - y1) * (x2 - x1) {
            inside = !inside;
        }
    }
    inside
}

/// The distance from a point to the middle line of a band, from the middle of its top end to
/// the middle of its bottom end.
fn distance(corners: &[(f32, f32); 4], point: (f32, f32)) -> f32 {
    let from = (f32::midpoint(corners[0].0, corners[1].0), corners[0].1);
    let to = (f32::midpoint(corners[2].0, corners[3].0), corners[2].1);
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((point.0 - from.0) * dx + (point.1 - from.1) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (point.0 - from.0 - t * dx).hypot(point.1 - from.1 - t * dy)
}

/// The alignment of the band under a place of the view, or of the nearest thin band. Of two
/// bands under the place, the narrower one.
pub fn hit(plot: &Plot, window: Window, size: (f32, f32), point: (f32, f32)) -> Option<usize> {
    let rows = Rows::of(size);
    plot.hits
        .iter()
        .enumerate()
        .filter_map(|(index, hit)| {
            let corners = band(plot, window, rows, hit)?;
            let away = if contains(&corners, point) {
                0.0
            } else {
                distance(&corners, point)
            };
            let wide = (corners[1].0 - corners[0].0).abs();
            (away <= REACH).then_some((index, away, wide))
        })
        .min_by(|first, second| {
            first
                .1
                .total_cmp(&second.1)
                .then(first.2.total_cmp(&second.2))
        })
        .map(|(index, ..)| index)
}

/// The part of the other row that holds the bases that align with a range of a row: of the
/// bottom row for a range of the top row when `top` is true, else of the top row. The part
/// leaves out the parts at its two ends that hold one base in 20 of the aligned bases, so that a
/// few repeats far away do not stretch it.
fn matching(plot: &Plot, range: Region, top: bool) -> Option<Region> {
    let mut pieces: Vec<Region> = plot
        .hits
        .iter()
        .filter_map(|hit| {
            let ((x0, y0), (x1, y1)) = plot.line(hit);
            let (from, to, other_from, other_to) = if top {
                (x0, x1, y0, y1)
            } else {
                (y0, y1, x0, x1)
            };
            let part = Region::between(from, to).intersection(range)?;
            let at = |place: u64| {
                let share = (place as f64 - from as f64) / (to as f64 - from as f64);
                (other_from as f64 + share * (other_to as f64 - other_from as f64)).round() as u64
            };
            Some(Region::between(at(part.start()), at(part.end())))
        })
        .collect();
    let total: u64 = pieces.iter().map(|piece| piece.len().max(1)).sum();
    let trim = total / TRIM;
    let reach = |pieces: &[Region]| {
        let mut sum = 0;
        pieces
            .iter()
            .find(|piece| {
                sum += piece.len().max(1);
                sum > trim
            })
            .copied()
    };
    pieces.sort_by_key(|piece| piece.start());
    let start = reach(&pieces)?.start();
    pieces.sort_by_key(|piece| std::cmp::Reverse(piece.end()));
    let end = reach(&pieces)?.end();
    Some(Region::between(start, end))
}

/// The part of the plot for a range that the pointer drags along a row. The other row shows the
/// parts that align with the range, with a margin. None for a drag of less than a few pixels.
pub fn zoomed(
    plot: &Plot,
    window: Window,
    size: (f32, f32),
    from: (f32, f32),
    to: (f32, f32),
) -> Option<Window> {
    if (from.0 - to.0).abs() < LEAST {
        return None;
    }
    let rows = Rows::of(size);
    let top = rows.is_top(from.1);
    let (chosen, other, whole) = if top {
        (window.across, window.down, plot.whole().down)
    } else {
        (window.down, window.across, plot.whole().across)
    };
    let range = Region::between(rows.place(chosen, from.0), rows.place(chosen, to.0));
    if range.len() < SMALLEST {
        return None;
    }
    let other = matching(plot, range, top).map_or(other, |hull| {
        let margin = (hull.len() as f64 * MARGIN).round() as u64;
        let padded = hull.expand(margin, whole.end());
        if padded.len() < SMALLEST {
            padded.expand(SMALLEST, whole.end())
        } else {
            padded
        }
    });
    Some(if top {
        Window {
            across: range,
            down: other,
        }
    } else {
        Window {
            across: other,
            down: range,
        }
    })
}

#[cfg(test)]
mod tests {
    use genie_core::Region;
    use genie_core::dotplot::{Builder, Hit, Plot, Window};

    use super::{Rows, hit, scene, zoomed};
    use crate::dotplot::Marks;
    use crate::scene::{Hue, Paint, Primitive};

    /// The plot of the tests of the dot plot: a forward alignment of `a` with the start of chr1,
    /// and a reverse alignment of `b` with the end of chr2.
    fn plot() -> Plot {
        let mut builder = Builder::default();
        let one = builder.target("chr1", 1_000);
        let two = builder.target("chr2", 1_000);
        let a = builder.query("a", 500);
        let b = builder.query("b", 500);
        let alignment = |query, target, on_target: (u64, u64), reverse| Hit {
            query,
            target,
            on_query: Region::between(0, 500),
            on_target: Region::between(on_target.0, on_target.1),
            reverse,
            matches: 450,
            block: 500,
            mapq: Some(60),
            kind: Some('P'),
        };
        builder.push(alignment(a, one, (0, 500), false));
        builder.push(alignment(b, two, (500, 1_000), true));
        builder.finish(|name| ["chr1", "chr2"].iter().position(|known| *known == name))
    }

    /// The size of a scene with rows of 200 pixels, the top row at 16 and the bottom row at 76.
    const SIZE: (f32, f32) = (216.0, 100.0);

    /// A band of a scene: its corners and its color.
    type Band = ([(f32, f32); 4], Paint);

    fn bands(primitives: &[Primitive]) -> Vec<Band> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Ribbon { corners, paint } => Some((*corners, *paint)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn bands_join_the_two_parts_of_each_alignment_and_cross_for_the_reverse_strand() {
        let plot = plot();
        let rows = Rows::of(SIZE);
        assert_eq!(
            (rows.left, rows.width, rows.top, rows.bottom),
            (8.0, 200.0, 16.0, 76.0)
        );
        let primitives = scene(&plot, plot.whole(), SIZE, Marks::default());
        assert_eq!(
            bands(&primitives),
            [
                (
                    [(8.0, 24.0), (58.0, 24.0), (108.0, 76.0), (8.0, 76.0)],
                    Paint::Tint(Hue::Blue)
                ),
                (
                    [(158.0, 24.0), (208.0, 24.0), (108.0, 76.0), (208.0, 76.0)],
                    Paint::Tint(Hue::Vermillion)
                ),
            ]
        );
        let names: Vec<String> = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text { content, .. } => Some(content.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["chr1", "chr2", "a", "b"]);
        let rows = primitives
            .iter()
            .filter(|primitive| matches!(primitive, Primitive::Rect { .. }))
            .count();
        assert_eq!(rows, 4, "a bar for each sequence");
    }

    #[test]
    fn a_window_keeps_the_parts_of_the_bands_with_both_ends_in_it() {
        let plot = plot();
        let window = Window {
            across: Region::between(250, 750),
            down: Region::between(0, 1_000),
        };
        let found = bands(&scene(&plot, window, SIZE, Marks::default()));
        assert_eq!(
            found,
            [(
                [(8.0, 24.0), (108.0, 24.0), (108.0, 76.0), (58.0, 76.0)],
                Paint::Tint(Hue::Blue)
            )]
        );
    }

    #[test]
    fn a_zoom_leaves_out_a_few_bases_that_align_far_away() {
        let mut builder = Builder::default();
        let target = builder.target("chr1", 10_000);
        let far = builder.query("far", 5_000);
        let near = builder.query("near", 5_000);
        let alignment = |query, on_target: (u64, u64)| Hit {
            query,
            target,
            on_query: Region::between(0, on_target.1 - on_target.0),
            on_target: Region::between(on_target.0, on_target.1),
            reverse: false,
            matches: 90,
            block: 100,
            mapq: None,
            kind: None,
        };
        builder.push(alignment(near, (0, 5_000)));
        builder.push(alignment(far, (100, 150)));
        let plot = builder.finish(|_| None);
        assert_eq!(plot.queries[0].name, "far", "the repeat comes first");
        assert_eq!(
            zoomed(&plot, plot.whole(), SIZE, (8.0, 10.0), (108.0, 10.0)),
            Some(Window {
                across: Region::between(0, 5_000),
                down: Region::between(4_900, 10_000),
            })
        );
    }

    #[test]
    fn the_pointer_picks_the_band_under_it() {
        let plot = plot();
        let whole = plot.whole();
        assert_eq!(hit(&plot, whole, SIZE, (40.0, 50.0)), Some(0));
        assert_eq!(hit(&plot, whole, SIZE, (180.0, 30.0)), Some(1));
        assert_eq!(hit(&plot, whole, SIZE, (130.0, 30.0)), None);
        let marks = Marks {
            chosen: Some(1),
            ..Marks::default()
        };
        let found = bands(&scene(&plot, whole, SIZE, marks));
        assert_eq!(found.last().unwrap().1, Paint::Hue(Hue::Vermillion));
    }

    #[test]
    fn a_drag_along_a_row_zooms_in_and_the_other_row_follows() {
        let plot = plot();
        let whole = plot.whole();
        assert_eq!(
            zoomed(&plot, whole, SIZE, (58.0, 10.0), (8.0, 10.0)),
            Some(Window {
                across: Region::between(0, 500),
                down: Region::between(0, 510),
            })
        );
        assert_eq!(
            zoomed(&plot, whole, SIZE, (108.0, 90.0), (208.0, 90.0)),
            Some(Window {
                across: Region::between(1_490, 2_000),
                down: Region::between(500, 1_000),
            })
        );
        assert_eq!(zoomed(&plot, whole, SIZE, (8.0, 10.0), (10.0, 10.0)), None);
    }
}
