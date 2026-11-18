use std::borrow::Cow;
use std::iter;

use genie_core::{Viewport, grouped};

use crate::scene::{Align, Font, Paint, Primitive};

const MAJOR_HEIGHT: f32 = 8.0;
const MINOR_HEIGHT: f32 = 4.0;
const LABEL_SPACING: f64 = 100.0;
const MINOR_SPACING: f64 = 10.0;
const LABEL_SIZE: f32 = 11.0;
const LABEL_OFFSET: f32 = 4.0;
const MONO_ADVANCE: f32 = 0.6;
const BAR_SHARE: f32 = 1.0 / 6.0;
const BAR_LEAST: f32 = 24.0;
const BAR_MOST: f32 = 120.0;
const BAR_TICK: f32 = 6.0;
const BAR_GAP: f32 = 6.0;

pub fn scene(viewport: Viewport, height: f32) -> Vec<Primitive> {
    let scale = viewport.scale();
    let major = major_step(scale, LABEL_SPACING);
    let minor = minor_step(major, scale, MINOR_SPACING).unwrap_or(major);
    let first = viewport.start().max(0.0);
    let last = viewport.end().min(viewport.length() as f64);
    let width = viewport.width() as f32;
    let mut primitives = Vec::new();
    let mut position = (first / minor as f64).ceil() as u64 * minor;
    while position as f64 <= last {
        let x = viewport.to_px(position as f64) as f32;
        if position.is_multiple_of(major) {
            primitives.push(tick(x, height, MAJOR_HEIGHT, Paint::Muted));
            let text = label(position, major);
            let size = text.chars().count() as f32 * MONO_ADVANCE * LABEL_SIZE;
            if x + LABEL_OFFSET + size <= width {
                primitives.push(Primitive::Text {
                    x: x + LABEL_OFFSET,
                    y: (height - MAJOR_HEIGHT) / 2.0,
                    content: Cow::Owned(text),
                    size: LABEL_SIZE,
                    paint: Paint::Muted,
                    font: Font::Mono,
                    align: Align::Start,
                });
            }
        } else {
            primitives.push(tick(x, height, MINOR_HEIGHT, Paint::Line));
        }
        position += minor;
    }
    primitives
}

/// A scale bar for figures: a line of a round length that ends at `right`, centered on `middle`,
/// with the length on its left. The length is the longest 1, 2 or 5 step that takes at most a
/// sixth of the width (24 to 120 pixels), and one base when no step is that short.
pub fn scale_bar(viewport: Viewport, right: f32, middle: f32) -> Vec<Primitive> {
    let scale = viewport.scale();
    let most = f64::from((viewport.width() as f32 * BAR_SHARE).clamp(BAR_LEAST, BAR_MOST));
    let step = steps()
        .take_while(|&step| step as f64 / scale <= most)
        .last()
        .unwrap_or(1);
    let span = (step as f64 / scale) as f32;
    let left = right - span;
    let end = |x| Primitive::Rect {
        x,
        y: middle - BAR_TICK / 2.0,
        width: 1.0,
        height: BAR_TICK,
        paint: Paint::Text,
    };
    vec![
        Primitive::Rect {
            x: left,
            y: middle - 0.5,
            width: span,
            height: 1.0,
            paint: Paint::Text,
        },
        end(left),
        end(right - 1.0),
        Primitive::Text {
            x: left - BAR_GAP,
            y: middle,
            content: Cow::Owned(length(step)),
            size: LABEL_SIZE,
            paint: Paint::Muted,
            font: Font::Ui,
            align: Align::End,
        },
    ]
}

/// A round length with the unit of its size: 500 bp, 20 kb, 1 Mb.
fn length(bases: u64) -> String {
    if bases < 1_000 {
        format!("{bases} bp")
    } else {
        label(bases, bases)
    }
}

fn tick(x: f32, height: f32, length: f32, paint: Paint) -> Primitive {
    Primitive::Rect {
        x,
        y: height - length,
        width: 1.0,
        height: length,
        paint,
    }
}

pub fn major_step(scale: f64, min_spacing: f64) -> u64 {
    steps()
        .find(|&step| step as f64 / scale >= min_spacing)
        .unwrap_or(u64::MAX)
}

pub fn minor_step(major: u64, scale: f64, min_spacing: f64) -> Option<u64> {
    steps()
        .skip_while(|&step| step.saturating_mul(10) < major)
        .take_while(|&step| step < major)
        .filter(|&step| major.is_multiple_of(step))
        .find(|&step| step as f64 / scale >= min_spacing)
}

pub fn label(position: u64, step: u64) -> String {
    let (unit, suffix) = [(1_000_000_000, " Gb"), (1_000_000, " Mb"), (1_000, " kb")]
        .into_iter()
        .find(|&(unit, _)| step >= unit)
        .unwrap_or((1, ""));
    format!("{}{suffix}", grouped(position / unit))
}

fn steps() -> impl Iterator<Item = u64> {
    iter::successors(Some(1_u64), |power| power.checked_mul(10)).flat_map(|power| {
        [1, 2, 5]
            .into_iter()
            .filter_map(move |factor| power.checked_mul(factor))
    })
}

#[cfg(test)]
mod tests {
    use genie_core::{Region, Viewport};

    use super::{label, major_step, minor_step, scale_bar, scene};
    use crate::scene::{Paint, Primitive};

    fn ticks(primitives: &[Primitive], paint: Paint) -> Vec<(f32, f32)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect {
                    x,
                    height,
                    paint: p,
                    ..
                } if *p == paint => Some((*x, *height)),
                _ => None,
            })
            .collect()
    }

    fn positions(ticks: &[(f32, f32)]) -> Vec<f32> {
        ticks.iter().map(|(x, _)| *x).collect()
    }

    fn labels(primitives: &[Primitive]) -> Vec<(f32, String)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text { x, content, .. } => Some((*x, content.to_string())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn scene_labels_major_ticks() {
        let viewport = Viewport::new(1_000, 100.0).show(Region::new(400, 500).unwrap());
        let primitives = scene(viewport, 24.0);
        assert_eq!(positions(&ticks(&primitives, Paint::Muted)), [0.0, 100.0]);
        assert_eq!(labels(&primitives), [(4.0, "400".to_owned())]);
        assert_eq!(ticks(&primitives, Paint::Line).len(), 9);
    }

    #[test]
    fn labels_end_inside_the_view() {
        let viewport = Viewport::new(120_000, 999.5).show(Region::between(4_999, 8_000));
        let found = labels(&scene(viewport, 20.0));
        assert!(found.iter().any(|(_, text)| text == "7,500"), "{found:?}");
        for (x, text) in found {
            let end = x + text.chars().count() as f32 * 0.6 * 11.0;
            assert!(end <= 999.5, "{text} ends at {end}");
        }
    }

    #[test]
    fn scene_stays_inside_the_contig() {
        let primitives = scene(Viewport::new(1_000, 100.0), 24.0);
        assert_eq!(positions(&ticks(&primitives, Paint::Muted)), [0.0, 100.0]);
    }

    #[test]
    fn major_ticks_are_taller_than_minor_ticks() {
        let primitives = scene(Viewport::new(1_000, 100.0), 24.0);
        let major = ticks(&primitives, Paint::Muted)[0].1;
        let minor = ticks(&primitives, Paint::Line)[0].1;
        assert!(major > minor);
    }

    #[test]
    fn major_step_is_smallest_spaced_1_2_5_value() {
        assert_eq!(major_step(1.0, 100.0), 100);
        assert_eq!(major_step(10.0, 100.0), 1_000);
        assert_eq!(major_step(3.0, 100.0), 500);
        assert_eq!(major_step(1.0 / 32.0, 100.0), 5);
        assert_eq!(major_step(1e7, 100.0), 1_000_000_000);
    }

    #[test]
    fn minor_step_divides_major_step() {
        assert_eq!(minor_step(1_000, 1.0, 10.0), Some(100));
        assert_eq!(minor_step(2_000, 1.0, 10.0), Some(200));
        assert_eq!(minor_step(5_000, 1.0, 600.0), Some(1_000));
        assert_eq!(minor_step(5_000, 1.0, 2_000.0), None);
        assert_eq!(minor_step(1, 1.0, 1.0), None);
    }

    fn bar(primitives: &[Primitive]) -> (f32, f32) {
        match primitives.first() {
            Some(Primitive::Rect { x, width, .. }) => (*x, *width),
            other => panic!("no bar: {other:?}"),
        }
    }

    #[test]
    fn scale_bars_take_the_longest_round_length_that_fits() {
        let viewport = Viewport::new(10_000, 1_000.0).show(Region::between(0, 1_000));
        let primitives = scale_bar(viewport, 990.0, 11.0);
        assert_eq!(bar(&primitives), (890.0, 100.0));
        assert_eq!(labels(&primitives), [(884.0, "100 bp".to_owned())]);
        let ends: Vec<f32> = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect { x, height, .. } if *height > 1.0 => Some(*x),
                _ => None,
            })
            .collect();
        assert_eq!(ends, [890.0, 989.0]);
    }

    #[test]
    fn scale_bars_name_kilobases_and_megabases() {
        let kilobases = Viewport::new(100_000, 1_000.0).show(Region::between(0, 50_000));
        assert_eq!(labels(&scale_bar(kilobases, 1_000.0, 11.0))[0].1, "5 kb");
        let megabases = Viewport::new(50_000_000, 1_000.0).show(Region::between(0, 20_000_000));
        let primitives = scale_bar(megabases, 1_000.0, 11.0);
        assert_eq!(labels(&primitives)[0].1, "2 Mb");
        assert_eq!(bar(&primitives), (900.0, 100.0));
    }

    #[test]
    fn scale_bars_keep_one_base_at_the_deepest_zoom() {
        let viewport = Viewport::new(1_000, 120.0).show(Region::between(0, 3));
        let primitives = scale_bar(viewport, 120.0, 11.0);
        assert_eq!(bar(&primitives), (88.0, 32.0));
        assert_eq!(labels(&primitives)[0].1, "1 bp");
    }

    #[test]
    fn labels_use_unit_of_step() {
        assert_eq!(label(12_500_000, 500_000), "12,500 kb");
        assert_eq!(label(14_000_000, 2_000_000), "14 Mb");
        assert_eq!(label(3_000_000_000, 1_000_000_000), "3 Gb");
        assert_eq!(label(38_270_125, 5), "38,270,125");
    }
}
