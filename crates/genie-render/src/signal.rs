use std::borrow::Cow;

use genie_core::signal::{Bin, Summary, slice};
use genie_core::{Region, Viewport};

use crate::scene::{Align, Font, Hue, Paint, Primitive};

pub const HEIGHT: f32 = 48.0;

const RANGE_SIZE: f32 = 10.0;
const LINE: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    Bars,
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    pub min: f32,
    pub max: f32,
}

impl Scale {
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    pub x: f32,
    pub width: f32,
    pub span: Region,
    pub bin: Bin,
}

fn same(a: Bin, b: Bin) -> bool {
    let close = (a.mean() - b.mean()).abs() <= 1e-9 * a.mean().abs().max(1.0);
    a.min.to_bits() == b.min.to_bits() && a.max.to_bits() == b.max.to_bits() && close
}

pub fn columns(viewport: Viewport, summaries: &[Summary]) -> Vec<Column> {
    let visible = viewport.visible();
    let left = viewport.to_px(visible.start() as f64).floor().max(0.0) as u64;
    let right = viewport
        .to_px(visible.end() as f64)
        .ceil()
        .min(viewport.width()) as u64;
    let mut found: Vec<Column> = Vec::new();
    let mut first = 0;
    for pixel in left..right {
        let from = viewport.to_bp(pixel as f64).floor().max(0.0) as u64;
        let to = (viewport.to_bp((pixel + 1) as f64).floor() as u64).max(from + 1);
        while summaries
            .get(first)
            .is_some_and(|summary| summary.span.end() <= from)
        {
            first += 1;
        }
        let span = Region::between(from, to);
        let Some(bin) = slice(&summaries[first..], span) else {
            continue;
        };
        let x = pixel as f32;
        match found.last_mut() {
            Some(last) if same(last.bin, bin) && (last.x + last.width - x).abs() < 0.5 => {
                last.width += 1.0;
                last.span = last.span.hull(span);
                last.bin.count += bin.count;
                last.bin.sum += bin.sum;
            }
            _ => found.push(Column {
                x,
                width: 1.0,
                span,
                bin,
            }),
        }
    }
    found
}

pub fn range(columns: &[Column]) -> Option<Scale> {
    let first = columns.first()?;
    let (min, max) = columns
        .iter()
        .fold((first.bin.min, first.bin.max), |(min, max), column| {
            (min.min(column.bin.min), max.max(column.bin.max))
        });
    let (min, max) = (min.min(0.0), max.max(0.0));
    Some(if max > min {
        Scale { min, max }
    } else {
        Scale { min: 0.0, max: 1.0 }
    })
}

fn number(value: f32) -> String {
    let size = value.abs();
    let digits = if size >= 100.0 {
        0
    } else if size >= 10.0 {
        1
    } else if size >= 1.0 {
        2
    } else {
        3
    };
    let text = format!("{value:.digits$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

fn label(scale: Scale) -> String {
    if scale.min == 0.0 {
        format!("0-{}", number(scale.max))
    } else {
        format!("{} to {}", number(scale.min), number(scale.max))
    }
}

pub fn scene(
    columns: &[Column],
    scale: Scale,
    style: Style,
    hue: Hue,
    height: f32,
    width: f32,
) -> Vec<Primitive> {
    let usable = height - 1.0;
    let y = |value: f32| {
        (scale.max - value.clamp(scale.min, scale.max)) / (scale.max - scale.min) * usable
    };
    let bar = |x: f32, width: f32, from: f32, to: f32, paint: Paint| {
        let (top, bottom) = (y(from.max(to)), y(from.min(to)));
        (bottom > top).then_some(Primitive::Rect {
            x,
            y: top,
            width,
            height: bottom - top,
            paint,
        })
    };
    let mut primitives = Vec::new();
    let mut previous: Option<(f32, f32)> = None;
    for column in columns {
        let mean = column.bin.mean() as f32;
        let (x, size) = (column.x, column.width);
        if style == Style::Bars {
            let (low, high) = (column.bin.min.min(0.0), column.bin.max.max(0.0));
            primitives.extend(bar(x, size, low, high, Paint::Tint(hue)));
            primitives.extend(bar(x, size, 0.0, mean, Paint::Hue(hue)));
            continue;
        }
        primitives.extend(bar(x, size, 0.0, mean, Paint::Tint(hue)));
        let top = (y(mean) - LINE / 2.0).clamp(0.0, (usable - LINE).max(0.0));
        primitives.push(Primitive::Rect {
            x,
            y: top,
            width: size,
            height: LINE,
            paint: Paint::Hue(hue),
        });
        if let Some((right, last)) = previous
            && (right - x).abs() < 0.5
        {
            primitives.push(Primitive::Rect {
                x: x - 0.5,
                y: last.min(top),
                width: 1.0,
                height: (last - top).abs() + LINE,
                paint: Paint::Hue(hue),
            });
        }
        previous = Some((x + size, top));
    }
    primitives.push(Primitive::Text {
        x: width - 4.0,
        y: RANGE_SIZE / 2.0 + 1.0,
        content: Cow::Owned(label(scale)),
        size: RANGE_SIZE,
        paint: Paint::Muted,
        font: Font::Ui,
        align: Align::End,
    });
    primitives
}

pub fn hit(columns: &[Column], x: f32) -> Option<Column> {
    columns
        .iter()
        .find(|column| column.x <= x && x < column.x + column.width)
        .copied()
}

#[cfg(test)]
mod tests {
    use genie_core::signal::{Bin, Summary};
    use genie_core::{Region, Viewport};

    use super::{Column, Scale, Style, columns, hit, range, scene};
    use crate::scene::{Hue, Paint, Primitive};

    fn view(start: u64, end: u64) -> Viewport {
        Viewport::new(1_000_000, 100.0).show(Region::between(start, end))
    }

    fn value(start: u64, end: u64, value: f32) -> Summary {
        Summary::single(Region::between(start, end), value)
    }

    fn column(x: f32, mean: f32, min: f32, max: f32) -> Column {
        Column {
            x,
            width: 1.0,
            span: Region::between(0, 1),
            bin: Bin {
                count: 1.0,
                min,
                max,
                sum: f64::from(mean),
            },
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

    fn texts(primitives: &[Primitive]) -> Vec<(String, f32)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text { content, x, .. } => Some((content.to_string(), *x)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn columns_average_the_bases_under_each_pixel() {
        let summaries = [
            value(0, 100, 1.0),
            value(100, 150, 3.0),
            value(150, 200, 5.0),
        ];
        let found = columns(view(0, 200), &summaries);
        let shape: Vec<(f32, f32, f64)> = found
            .iter()
            .map(|column| (column.x, column.width, column.bin.mean()))
            .collect();
        assert_eq!(
            shape,
            [(0.0, 50.0, 1.0), (50.0, 25.0, 3.0), (75.0, 25.0, 5.0)]
        );
        assert_eq!(found[1].span, Region::between(100, 150));
        let zoomed = columns(view(10, 20), &[value(12, 13, 2.0)]);
        assert_eq!(zoomed.len(), 1);
        assert_eq!((zoomed[0].x, zoomed[0].width), (20.0, 10.0));
    }

    #[test]
    fn pixels_with_the_same_value_make_one_column() {
        let found = columns(view(0, 150), &[value(30, 60, 2.0), value(60, 90, 2.5)]);
        let shape: Vec<(f32, f32, Region)> = found
            .iter()
            .map(|column| (column.x, column.width, column.span))
            .collect();
        assert_eq!(
            shape,
            [
                (20.0, 20.0, Region::between(30, 60)),
                (40.0, 20.0, Region::between(60, 90)),
            ]
        );
        assert!((found[0].bin.mean() - 2.0).abs() < 1e-9);
        assert!((found[0].bin.count - 30.0).abs() < 1e-9);
    }

    #[test]
    fn bars_reach_the_mean_and_the_tint_reaches_the_max() {
        let scale = Scale { min: 0.0, max: 4.0 };
        let found = rects(&scene(
            &[column(5.0, 2.0, 1.0, 4.0)],
            scale,
            Style::Bars,
            Hue::Green,
            41.0,
            100.0,
        ));
        assert_eq!(
            found,
            [
                ([5.0, 0.0, 1.0, 40.0], Paint::Tint(Hue::Green)),
                ([5.0, 20.0, 1.0, 20.0], Paint::Hue(Hue::Green)),
            ]
        );
    }

    #[test]
    fn negative_values_hang_from_the_zero_line() {
        let scale = Scale {
            min: -2.0,
            max: 2.0,
        };
        let found = rects(&scene(
            &[column(0.0, -1.0, -2.0, 0.0)],
            scale,
            Style::Bars,
            Hue::Blue,
            41.0,
            100.0,
        ));
        assert_eq!(
            found,
            [
                ([0.0, 20.0, 1.0, 20.0], Paint::Tint(Hue::Blue)),
                ([0.0, 20.0, 1.0, 10.0], Paint::Hue(Hue::Blue)),
            ]
        );
    }

    #[test]
    fn lines_follow_the_mean() {
        let scale = Scale { min: 0.0, max: 4.0 };
        let shown = [column(0.0, 2.0, 2.0, 2.0), column(1.0, 4.0, 4.0, 4.0)];
        let found = rects(&scene(&shown, scale, Style::Line, Hue::Blue, 41.0, 100.0));
        let lines: Vec<[f32; 4]> = found
            .iter()
            .filter(|(_, paint)| *paint == Paint::Hue(Hue::Blue))
            .map(|(rect, _)| *rect)
            .collect();
        assert_eq!(lines[0], [0.0, 19.0, 1.0, 2.0]);
        assert_eq!(lines[1], [1.0, 0.0, 1.0, 2.0]);
        assert_eq!(lines[2], [0.5, 0.0, 1.0, 21.0]);
        assert!(found.contains(&([0.0, 20.0, 1.0, 20.0], Paint::Tint(Hue::Blue))));
    }

    #[test]
    fn the_range_covers_zero_and_the_highest_max() {
        assert_eq!(range(&[]), None);
        let positive = [column(0.0, 2.0, 1.0, 5.0), column(1.0, 3.0, 2.0, 4.0)];
        assert_eq!(range(&positive), Some(Scale { min: 0.0, max: 5.0 }));
        let mixed = [column(0.0, -1.0, -3.0, 0.5), column(1.0, 3.0, 2.0, 4.0)];
        assert_eq!(
            range(&mixed),
            Some(Scale {
                min: -3.0,
                max: 4.0
            })
        );
        let flat = [column(0.0, 0.0, 0.0, 0.0)];
        assert_eq!(range(&flat), Some(Scale { min: 0.0, max: 1.0 }));
        let linked = Scale { min: 0.0, max: 5.0 }.union(Scale {
            min: -3.0,
            max: 4.0,
        });
        assert_eq!(
            linked,
            Scale {
                min: -3.0,
                max: 5.0
            }
        );
    }

    #[test]
    fn labels_show_the_range_at_the_right() {
        let shown = [column(0.0, 1.0, 1.0, 1.0)];
        let label = |scale| texts(&scene(&shown, scale, Style::Bars, Hue::Blue, 41.0, 100.0));
        assert_eq!(
            label(Scale {
                min: 0.0,
                max: 12.5
            }),
            [("0-12.5".to_owned(), 96.0)]
        );
        assert_eq!(
            label(Scale {
                min: -3.25,
                max: 1250.0
            }),
            [("-3.25 to 1250".to_owned(), 96.0)]
        );
        assert_eq!(
            label(Scale {
                min: 0.0,
                max: 0.0312
            }),
            [("0-0.031".to_owned(), 96.0)]
        );
    }

    #[test]
    fn hits_find_the_column_under_the_pointer() {
        let shown = [column(0.0, 1.0, 1.0, 1.0), column(5.0, 2.0, 2.0, 2.0)];
        assert_eq!(hit(&shown, 5.5), Some(shown[1]));
        assert_eq!(hit(&shown, 3.0), None);
    }
}
