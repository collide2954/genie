//! The scene of a dot plot: the target sequences across, the query sequences down, a line for
//! each alignment, and the names of the sequences that have space for them.

use std::borrow::Cow;
use std::collections::HashSet;

use genie_core::Region;
use genie_core::dotplot::{Hit, Plot, Window};

use crate::scene::{Align, Font, Hue, Paint, Primitive};

/// The colors of the alignments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Coloring {
    /// Blue for the forward strand and vermillion for the reverse strand.
    #[default]
    Strand,
    /// A scale from light for a low identity to dark for a high identity.
    Identity,
}

/// The space at the left of the plot for the names of the queries.
pub const LEFT: f32 = 120.0;
/// The space over the plot for the names of the targets.
pub const TOP: f32 = 18.0;
/// The space at the right of the plot and under it.
const EDGE: f32 = 8.0;
const SIZE: f32 = 10.0;
const CHARACTER: f32 = 6.0;
const WIDTH: f32 = 2.0;
/// The width of the line of the alignment under the pointer.
const CHOSEN: f32 = 3.5;
/// The least space between two lines of the grid, so that many short sequences do not fill the
/// plot with lines.
const GRID: f32 = 3.0;
/// The farthest distance of the pointer from a line that still picks the line.
const REACH: f32 = 5.0;
/// The least size of a dragged box that zooms in, in pixels.
const LEAST: f32 = 4.0;
/// The least size of a window, in bases.
const SMALLEST: u64 = 20;
/// The lightest color of the identity scale, so that a line of low identity stays in sight.
const FLOOR: f64 = 25.0;

/// The part of a scene that shows the plot, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl Area {
    /// The plot area of a scene of a size, in pixels.
    pub fn of((width, height): (f32, f32)) -> Self {
        Self {
            left: LEFT,
            top: TOP,
            width: (width - LEFT - EDGE).max(1.0),
            height: (height - TOP - EDGE).max(1.0),
        }
    }

    fn x(self, window: Window, across: f64) -> f32 {
        let share = (across - window.across.start() as f64) / window.across.len().max(1) as f64;
        self.left + (share * f64::from(self.width)) as f32
    }

    fn y(self, window: Window, down: f64) -> f32 {
        let share = (down - window.down.start() as f64) / window.down.len().max(1) as f64;
        self.top + (share * f64::from(self.height)) as f32
    }

    /// The place of the plot in bases at a place of the scene, inside the window.
    fn bases(self, window: Window, (x, y): (f32, f32)) -> (u64, u64) {
        let along = |at: f32, from: f32, size: f32, region: Region| {
            let share = f64::from(((at - from) / size).clamp(0.0, 1.0));
            region.start() + (share * region.len() as f64).round() as u64
        };
        (
            along(x, self.left, self.width, window.across),
            along(y, self.top, self.height, window.down),
        )
    }
}

/// How the scene colors the alignments, and what it shows on top of them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Marks {
    pub coloring: Coloring,
    /// The identities at the light end and at the dark end of the identity scale.
    pub identities: (f64, f64),
    /// The alignment under the pointer, which the scene draws wider.
    pub chosen: Option<usize>,
    /// The box that the pointer drags, from its first corner to its second, in pixels.
    pub dragged: Option<((f32, f32), (f32, f32))>,
}

/// The place of an identity on the identity scale: from the lightest color for the low end of
/// the range or less, to 100 for its high end.
pub fn level(identity: f64, (low, high): (f64, f64)) -> u8 {
    let share = if high - low > f64::EPSILON {
        ((identity - low) / (high - low)).clamp(0.0, 1.0)
    } else {
        1.0
    };
    (FLOOR + share * (100.0 - FLOOR)).round() as u8
}

/// The color of an alignment, and a number for it that is the same for the same color.
fn paint(hit: &Hit, marks: &Marks) -> (Paint, u8) {
    match marks.coloring {
        Coloring::Strand if hit.reverse => (Paint::Hue(Hue::Vermillion), 0),
        Coloring::Strand => (Paint::Hue(Hue::Blue), 1),
        Coloring::Identity => {
            let level = level(hit.identity(), marks.identities);
            (Paint::Level(level), level)
        }
    }
}

/// The part of the line of an alignment in a window, in pixels of the plot area. None when the
/// line is not in the window.
fn line(plot: &Plot, window: Window, area: Area, hit: &Hit) -> Option<((f32, f32), (f32, f32))> {
    let ((x0, y0), (x1, y1)) = plot.line(hit);
    let (x0, y0, x1, y1) = (x0 as f64, y0 as f64, x1 as f64, y1 as f64);
    let (mut low, mut high) = (0.0_f64, 1.0_f64);
    for (start, delta, region) in [(x0, x1 - x0, window.across), (y0, y1 - y0, window.down)] {
        let (least, most) = (region.start() as f64, region.end() as f64);
        if delta.abs() < f64::EPSILON {
            if start < least || start > most {
                return None;
            }
            continue;
        }
        let (first, second) = ((least - start) / delta, (most - start) / delta);
        low = low.max(first.min(second));
        high = high.min(first.max(second));
    }
    if low > high {
        return None;
    }
    let at = |t: f64| {
        (
            area.x(window, x0 + t * (x1 - x0)),
            area.y(window, y0 + t * (y1 - y0)),
        )
    };
    Some((at(low), at(high)))
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Primitive {
    Primitive::Rect {
        x,
        y,
        width,
        height,
        paint: Paint::Line,
    }
}

/// The frame of the plot area, and a line between each two sequences in view that are far
/// enough apart.
fn grid(plot: &Plot, window: Window, area: Area) -> Vec<Primitive> {
    let (right, bottom) = (area.left + area.width - 1.0, area.top + area.height - 1.0);
    let mut found = vec![
        rect(area.left, area.top, area.width, 1.0),
        rect(area.left, bottom, area.width, 1.0),
        rect(area.left, area.top, 1.0, area.height),
        rect(right, area.top, 1.0, area.height),
    ];
    let inside = |region: Region, offset: u64| offset > region.start() && offset < region.end();
    let mut last = area.left;
    for target in &plot.targets {
        let x = area.x(window, target.offset as f64).floor();
        if inside(window.across, target.offset) && x - last >= GRID && right - x >= GRID {
            found.push(rect(x, area.top, 1.0, area.height));
            last = x;
        }
    }
    let mut last = area.top;
    for query in &plot.queries {
        let y = area.y(window, query.offset as f64).floor();
        if inside(window.down, query.offset) && y - last >= GRID && bottom - y >= GRID {
            found.push(rect(area.left, y, area.width, 1.0));
            last = y;
        }
    }
    found
}

fn text(x: f32, y: f32, content: String, align: Align) -> Primitive {
    Primitive::Text {
        x,
        y,
        content: Cow::Owned(content),
        size: SIZE,
        paint: Paint::Muted,
        font: Font::Ui,
        align,
    }
}

fn wide(name: &str) -> f32 {
    name.chars().count() as f32 * CHARACTER
}

/// The name of a target that fits in a width: the whole name, or the name without "chr".
fn fitted(name: &str, room: f32) -> Option<String> {
    if wide(name) <= room {
        return Some(name.to_owned());
    }
    let short = name.strip_prefix("chr").filter(|rest| !rest.is_empty())?;
    (wide(short) <= room).then(|| short.to_owned())
}

/// The name of a query in the space at the left of the plot, cut with "…" if it is too long.
fn cut(name: &str) -> String {
    let most = ((LEFT - 8.0) / CHARACTER) as usize;
    if name.chars().count() <= most {
        return name.to_owned();
    }
    let kept: String = name.chars().take(most.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// The names of the targets over the plot and of the queries at its left, where they fit.
fn names(plot: &Plot, window: Window, area: Area) -> Vec<Primitive> {
    let mut found = Vec::new();
    let span = |offset: u64, length: u64, region: Region| {
        Region::between(offset, offset + length).intersection(region)
    };
    for target in &plot.targets {
        let Some(shown) = span(target.offset, target.length, window.across) else {
            continue;
        };
        let from = area.x(window, shown.start() as f64);
        let to = area.x(window, shown.end() as f64);
        if let Some(name) = fitted(&target.name, to - from - 4.0) {
            found.push(text(
                f32::midpoint(from, to),
                area.top / 2.0,
                name,
                Align::Center,
            ));
        }
    }
    for query in &plot.queries {
        let Some(shown) = span(query.offset, query.length, window.down) else {
            continue;
        };
        let from = area.y(window, shown.start() as f64);
        let to = area.y(window, shown.end() as f64);
        if to - from >= SIZE + 2.0 {
            let (x, y) = (area.left - 4.0, f32::midpoint(from, to));
            found.push(text(x, y, cut(&query.name), Align::End));
        }
    }
    found
}

/// The scene of the part of a plot in a window, for a space of `size` pixels. Alignments that
/// fall on the same pixels with the same color give one line.
pub fn scene(plot: &Plot, window: Window, size: (f32, f32), marks: Marks) -> Vec<Primitive> {
    let area = Area::of(size);
    let mut primitives = grid(plot, window, area);
    primitives.extend(names(plot, window, area));
    let half = |value: f32| (value * 2.0).round() as i32;
    let mut seen = HashSet::new();
    for hit in &plot.hits {
        let Some((from, to)) = line(plot, window, area, hit) else {
            continue;
        };
        let (paint, tone) = paint(hit, &marks);
        if seen.insert((half(from.0), half(from.1), half(to.0), half(to.1), tone)) {
            primitives.push(Primitive::Line {
                from,
                to,
                width: WIDTH,
                paint,
            });
        }
    }
    if let Some(hit) = marks.chosen.and_then(|chosen| plot.hits.get(chosen))
        && let Some((from, to)) = line(plot, window, area, hit)
    {
        primitives.push(Primitive::Line {
            from,
            to,
            width: CHOSEN,
            paint: Paint::Accent,
        });
    }
    if let Some((first, second)) = marks.dragged {
        primitives.push(Primitive::Rect {
            x: first.0.min(second.0),
            y: first.1.min(second.1),
            width: (first.0 - second.0).abs(),
            height: (first.1 - second.1).abs(),
            paint: Paint::Selection,
        });
    }
    primitives
}

/// The distance from a point to the line between two points.
fn distance(point: (f32, f32), from: (f32, f32), to: (f32, f32)) -> f32 {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((point.0 - from.0) * dx + (point.1 - from.1) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (x, y) = (from.0 + t * dx, from.1 + t * dy);
    (point.0 - x).hypot(point.1 - y)
}

/// The alignment nearest to a place of the scene, if its line passes near the place.
pub fn hit(plot: &Plot, window: Window, size: (f32, f32), point: (f32, f32)) -> Option<usize> {
    let area = Area::of(size);
    plot.hits
        .iter()
        .enumerate()
        .filter_map(|(index, hit)| {
            let (from, to) = line(plot, window, area, hit)?;
            let distance = distance(point, from, to);
            (distance <= REACH).then_some((index, distance))
        })
        .min_by(|(_, first), (_, second)| first.total_cmp(second))
        .map(|(index, _)| index)
}

/// The part of the plot in a box that the pointer drags from one corner to the other. None for
/// a box of less than a few pixels, or for a window of less than a few bases.
pub fn zoomed(
    window: Window,
    size: (f32, f32),
    from: (f32, f32),
    to: (f32, f32),
) -> Option<Window> {
    if (from.0 - to.0).abs() < LEAST || (from.1 - to.1).abs() < LEAST {
        return None;
    }
    let area = Area::of(size);
    let (first, second) = (area.bases(window, from), area.bases(window, to));
    let zoomed = Window {
        across: Region::between(first.0, second.0),
        down: Region::between(first.1, second.1),
    };
    (zoomed.across.len() >= SMALLEST && zoomed.down.len() >= SMALLEST).then_some(zoomed)
}

#[cfg(test)]
mod tests {
    use genie_core::Region;
    use genie_core::dotplot::{Builder, Hit, Plot, Window};

    use super::{Area, Coloring, Marks, hit, level, scene, zoomed};
    use crate::scene::{Hue, Paint, Primitive};

    /// A plot of two targets of 1,000 bases and two queries of 500 bases: a forward alignment
    /// of `a` with the start of chr1, and a reverse alignment of `b` with the end of chr2.
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

    /// The size of a scene with a plot area of 200 by 100 pixels.
    const SIZE: (f32, f32) = (328.0, 126.0);

    /// A line of a scene: its two ends and its color.
    type Line = ((f32, f32), (f32, f32), Paint);

    fn lines(primitives: &[Primitive]) -> Vec<Line> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Line {
                    from, to, paint, ..
                } => Some((*from, *to, *paint)),
                _ => None,
            })
            .collect()
    }

    fn texts(primitives: &[Primitive]) -> Vec<String> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text { content, .. } => Some(content.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn alignments_are_lines_in_the_colors_of_their_strands() {
        let plot = plot();
        let area = Area::of(SIZE);
        assert_eq!((area.width, area.height), (200.0, 100.0));
        let primitives = scene(&plot, plot.whole(), SIZE, Marks::default());
        let (left, top) = (area.left, area.top);
        assert_eq!(
            lines(&primitives),
            [
                (
                    (left, top),
                    (left + 50.0, top + 50.0),
                    Paint::Hue(Hue::Blue)
                ),
                (
                    (left + 150.0, top + 100.0),
                    (left + 200.0, top + 50.0),
                    Paint::Hue(Hue::Vermillion)
                ),
            ]
        );
        assert_eq!(texts(&primitives), ["chr1", "chr2", "a", "b"]);
        let middle = primitives.iter().any(|primitive| {
            matches!(primitive, Primitive::Rect { x, width, .. }
                if (x - left - 100.0).abs() < 1e-3 && (width - 1.0).abs() < 1e-3)
        });
        assert!(middle, "a line between the two targets");
    }

    #[test]
    fn a_window_clips_the_lines_and_drops_the_lines_outside_it() {
        let plot = plot();
        let window = Window {
            across: Region::between(250, 750),
            down: Region::between(0, 500),
        };
        let area = Area::of(SIZE);
        let found = lines(&scene(&plot, window, SIZE, Marks::default()));
        assert_eq!(found.len(), 1);
        let (from, to, _) = found[0];
        assert_eq!(from, (area.left, area.top + 50.0));
        assert_eq!(to, (area.left + 100.0, area.top + 100.0));
    }

    #[test]
    fn identity_colors_go_from_light_to_dark_over_the_range_of_the_plot() {
        let range = (0.7, 1.0);
        assert_eq!(level(0.5, range), 25);
        assert_eq!(level(0.7, range), 25);
        assert_eq!(level(1.0, range), 100);
        assert_eq!(level(0.775, range), 44);
        assert_eq!(level(0.9, (0.9, 0.9)), 100);
        let plot = plot();
        let marks = Marks {
            coloring: Coloring::Identity,
            identities: (0.8, 1.0),
            ..Marks::default()
        };
        let found = lines(&scene(&plot, plot.whole(), SIZE, marks));
        assert_eq!(found[0].2, Paint::Level(level(0.9, (0.8, 1.0))));
    }

    #[test]
    fn alignments_on_the_same_pixels_give_one_line() {
        let mut plot = plot();
        let copy = plot.hits[0];
        plot.hits.push(Hit {
            on_target: Region::between(1, 500),
            ..copy
        });
        assert_eq!(
            lines(&scene(&plot, plot.whole(), SIZE, Marks::default())).len(),
            2
        );
    }

    #[test]
    fn the_pointer_picks_the_nearest_line_and_boxes_zoom_in() {
        let plot = plot();
        let area = Area::of(SIZE);
        let whole = plot.whole();
        let near = (area.left + 26.0, area.top + 24.0);
        assert_eq!(hit(&plot, whole, SIZE, near), Some(0));
        let far = (area.left + 100.0, area.top + 10.0);
        assert_eq!(hit(&plot, whole, SIZE, far), None);
        let marks = Marks {
            chosen: Some(1),
            dragged: Some(((10.0, 20.0), (4.0, 30.0))),
            ..Marks::default()
        };
        let primitives = scene(&plot, whole, SIZE, marks);
        let chosen = lines(&primitives);
        assert_eq!(chosen.last().unwrap().2, Paint::Accent);
        assert!(primitives.contains(&Primitive::Rect {
            x: 4.0,
            y: 20.0,
            width: 6.0,
            height: 10.0,
            paint: Paint::Selection,
        }));
        let from = (area.left + 50.0, area.top);
        let to = (area.left + 100.0, area.top + 50.0);
        assert_eq!(
            zoomed(whole, SIZE, to, from),
            Some(Window {
                across: Region::between(500, 1_000),
                down: Region::between(0, 500),
            })
        );
        assert_eq!(zoomed(whole, SIZE, from, (from.0 + 2.0, to.1)), None);
    }
}
