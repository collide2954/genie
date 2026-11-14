//! The lane of sequence matches: a bar per match, arrows for its strand, marks on wrong bases.

use genie_core::search::Hit;
use genie_core::{Region, Strand, Viewport};

use crate::scene::{Hue, Paint, Primitive};

pub const HEIGHT: f32 = 14.0;
const BODY_TOP: f32 = 3.0;
const BODY_HEIGHT: f32 = 8.0;
const LINE_TOP: f32 = 12.0;
const LINE_HEIGHT: f32 = 2.0;
const NARROWEST: f32 = 2.0;
const UNDERLINE: f32 = 3.0;
const CHEVRON_MIN: f32 = 12.0;
const CHEVRON_SPACING: f32 = 16.0;
const CHEVRON_SIZE: f32 = 2.5;
const MARKS_FROM: f64 = 2.0;
const REACH: f64 = 3.0;
const EXACT: Paint = Paint::Hue(Hue::Blue);
const NEAR: Paint = Paint::Tint(Hue::Blue);
const WRONG: Paint = Paint::Hue(Hue::Vermillion);
const WHITE: Paint = Paint::Rgb([255, 255, 255]);

fn bar(left: f32, right: f32, width: f32, least: f32) -> (f32, f32) {
    let (mut left, mut right) = (left, right);
    let narrow = right - left < least;
    if narrow {
        let center = f32::midpoint(left, right);
        left = center - least / 2.0;
        right = center + least / 2.0;
    }
    if left < 0.0 {
        if narrow {
            right -= left;
        }
        left = 0.0;
    }
    if right > width {
        if narrow {
            left -= right - width;
        }
        right = width;
    }
    (left.max(0.0), right)
}

fn arrows(primitives: &mut Vec<Primitive>, left: f32, right: f32, forward: bool, paint: Paint) {
    let length = right - left;
    if length < CHEVRON_MIN {
        return;
    }
    let count = (length / CHEVRON_SPACING).floor().max(1.0);
    let step = length / count;
    for index in 0..count as usize {
        primitives.push(Primitive::Chevron {
            x: left + step * (index as f32 + 0.5),
            y: BODY_TOP + BODY_HEIGHT / 2.0,
            size: CHEVRON_SIZE,
            forward,
            paint,
        });
    }
}

pub fn scene(viewport: Viewport, hits: &[Hit], current: Option<Region>) -> Vec<Primitive> {
    let visible = viewport.visible();
    let width = viewport.width() as f32;
    let per_base = 1.0 / viewport.scale();
    let first = hits.partition_point(|hit| hit.span.end() <= visible.start());
    let mut primitives = Vec::new();
    let mut body: Option<(usize, Paint)> = None;
    let mut covered = f32::NEG_INFINITY;
    let mut underline = None;
    for hit in hits[first..]
        .iter()
        .take_while(|hit| hit.span.start() < visible.end())
    {
        let left = viewport.to_px(hit.span.start() as f64) as f32;
        let right = viewport.to_px(hit.span.end() as f64) as f32;
        let exact = hit.mismatches.is_empty();
        let paint = if exact { EXACT } else { NEAR };
        let (from, to) = bar(left, right, width, NARROWEST);
        match body {
            Some((index, drawn)) if drawn == paint && from <= covered => {
                if let Primitive::Rect { x, width, .. } = &mut primitives[index] {
                    *width = (to - *x).max(*width);
                }
            }
            _ if to <= covered => {}
            _ => {
                body = Some((primitives.len(), paint));
                primitives.push(Primitive::Rect {
                    x: from,
                    y: BODY_TOP,
                    width: to - from,
                    height: BODY_HEIGHT,
                    paint,
                });
            }
        }
        covered = covered.max(to);
        if hit.strand != Strand::Unknown {
            let ink = if exact { WHITE } else { EXACT };
            let forward = hit.strand == Strand::Forward;
            arrows(
                &mut primitives,
                left.max(0.0),
                right.min(width),
                forward,
                ink,
            );
        }
        if per_base >= MARKS_FROM {
            for offset in &hit.mismatches {
                let at = hit.span.start() + u64::from(*offset);
                primitives.push(Primitive::Rect {
                    x: viewport.to_px(at as f64) as f32,
                    y: BODY_TOP,
                    width: per_base as f32,
                    height: BODY_HEIGHT,
                    paint: WRONG,
                });
            }
        }
        if current == Some(hit.span) {
            let (from, to) = bar(left, right, width, UNDERLINE);
            underline = Some(Primitive::Rect {
                x: from,
                y: LINE_TOP,
                width: to - from,
                height: LINE_HEIGHT,
                paint: Paint::Accent,
            });
        }
    }
    primitives.extend(underline);
    primitives
}

fn distance(span: Region, position: f64) -> f64 {
    let (start, end) = (span.start() as f64, span.end() as f64);
    if position < start {
        start - position
    } else if position > end {
        position - end
    } else {
        0.0
    }
}

pub fn hit(viewport: Viewport, hits: &[Hit], x: f32) -> Option<usize> {
    let reach = REACH * viewport.scale();
    let position = viewport.to_bp(f64::from(x));
    let first = hits.partition_point(|hit| (hit.span.end() as f64) < position - reach);
    hits[first..]
        .iter()
        .take_while(|hit| hit.span.start() as f64 <= position + reach)
        .enumerate()
        .map(|(index, hit)| (first + index, distance(hit.span, position)))
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .filter(|(_, gap)| *gap <= reach)
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use genie_core::search::Hit;
    use genie_core::{Region, Strand, Viewport};

    use super::{hit, scene};
    use crate::scene::{Hue, Paint, Primitive};

    fn found(start: u64, end: u64, strand: Strand, mismatches: &[u32]) -> Hit {
        Hit {
            span: Region::between(start, end),
            strand,
            mismatches: mismatches.to_vec(),
        }
    }

    fn view(start: u64, end: u64, width: f64) -> Viewport {
        Viewport::new(10_000, width).show(Region::between(start, end))
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

    fn chevrons(primitives: &[Primitive]) -> Vec<(f32, bool, Paint)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Chevron {
                    x, forward, paint, ..
                } => Some((*x, *forward, *paint)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn exact_and_near_matches_draw_as_bars() {
        let hits = [
            found(10, 20, Strand::Forward, &[]),
            found(50, 60, Strand::Reverse, &[3]),
        ];
        let drawn = rects(&scene(view(0, 100, 100.0), &hits, None));
        assert_eq!(
            drawn,
            [
                (10.0, 3.0, 10.0, 8.0, Paint::Hue(Hue::Blue)),
                (50.0, 3.0, 10.0, 8.0, Paint::Tint(Hue::Blue)),
            ]
        );
    }

    #[test]
    fn arrows_show_the_strand_of_wide_matches() {
        let hits = [
            found(10, 30, Strand::Forward, &[]),
            found(40, 60, Strand::Reverse, &[2]),
            found(70, 90, Strand::Unknown, &[]),
            found(95, 99, Strand::Forward, &[]),
        ];
        let drawn = chevrons(&scene(view(0, 100, 100.0), &hits, None));
        assert_eq!(
            drawn,
            [
                (20.0, true, Paint::Rgb([255, 255, 255])),
                (50.0, false, Paint::Hue(Hue::Blue)),
            ]
        );
    }

    #[test]
    fn wrong_bases_get_marks_when_bases_are_wide() {
        let hits = [found(10, 20, Strand::Forward, &[2, 5])];
        let close = rects(&scene(view(0, 50, 200.0), &hits, None));
        assert_eq!(
            close[1..],
            [
                (48.0, 3.0, 4.0, 8.0, Paint::Hue(Hue::Vermillion)),
                (60.0, 3.0, 4.0, 8.0, Paint::Hue(Hue::Vermillion)),
            ]
        );
        let far = rects(&scene(view(0, 1_000, 200.0), &hits, None));
        assert_eq!(far.len(), 1);
    }

    #[test]
    fn the_current_match_is_underlined() {
        let hits = [
            found(10, 20, Strand::Forward, &[]),
            found(30, 31, Strand::Forward, &[]),
        ];
        let drawn = rects(&scene(
            view(0, 100, 100.0),
            &hits,
            Some(Region::between(30, 31)),
        ));
        assert_eq!(drawn.last(), Some(&(29.0, 12.0, 3.0, 2.0, Paint::Accent)));
    }

    #[test]
    fn crowded_matches_merge_into_few_bars() {
        let hits: Vec<Hit> = (0..2_000)
            .map(|start| found(start * 5, start * 5 + 4, Strand::Forward, &[]))
            .collect();
        let drawn = rects(&scene(view(0, 10_000, 100.0), &hits, None));
        assert_eq!(
            drawn,
            [(0.0, 3.0, 100.0, 8.0, Paint::Hue(Hue::Blue))],
            "{drawn:?}"
        );
        let narrow = rects(&scene(view(0, 10_000, 100.0), &hits[..1], None));
        assert_eq!(narrow, [(0.0, 3.0, 2.0, 8.0, Paint::Hue(Hue::Blue))]);
    }

    #[test]
    fn matches_out_of_view_are_left_out() {
        let hits = [
            found(10, 20, Strand::Forward, &[]),
            found(500, 520, Strand::Forward, &[]),
        ];
        assert_eq!(rects(&scene(view(400, 600, 200.0), &hits, None)).len(), 1);
    }

    #[test]
    fn the_pointer_finds_the_nearest_match() {
        let hits = [
            found(10, 20, Strand::Forward, &[]),
            found(22, 30, Strand::Reverse, &[]),
            found(60, 61, Strand::Forward, &[]),
        ];
        let viewport = view(0, 100, 100.0);
        assert_eq!(hit(viewport, &hits, 15.0), Some(0));
        assert_eq!(hit(viewport, &hits, 21.2), Some(1));
        assert_eq!(hit(viewport, &hits, 62.5), Some(2));
        assert_eq!(hit(viewport, &hits, 45.0), None);
        assert_eq!(hit(viewport, &[], 45.0), None);
    }
}
