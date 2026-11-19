//! The methylation difference of two groups: a bar at each site, up when group A has more
//! methylation and down when group B has more, and a dot on the sites where Fisher's exact test
//! gives p < 0.05. A pixel with more than one site shows their mean.

use std::borrow::Cow;

use genie_core::Viewport;
use genie_core::methylation::Difference;

use crate::scene::{Align, LABEL_INSET, Paint, Primitive};

/// The height of the lane.
pub const HEIGHT: f32 = 32.0;
/// The p value under which a site gets a dot.
pub const SIGNIFICANT: f64 = 0.05;
const DOT: f32 = 3.0;
const LABEL_SIZE: f32 = 10.0;
/// The widest bar, in pixels from the middle line, for 100 percentage points.
const REACH: f32 = HEIGHT / 2.0 - DOT - 2.0;
const RANGE: &str = "±100 points";

/// A column of the lane: its left edge, its width, the mean difference of its sites in
/// percentage points, and whether a site of it differs with p < 0.05.
type Column = (f32, f32, f32, bool);

fn columns(viewport: Viewport, differences: &[Difference]) -> Vec<Column> {
    let pixels = 1.0 / viewport.scale();
    let visible = viewport.visible();
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    let shown = differences
        .iter()
        .filter(|site| site.position >= visible.start() && site.position < visible.end());
    if pixels >= 1.0 {
        let width = if pixels >= 6.0 { pixels - 1.0 } else { pixels } as f32;
        return shown
            .map(|site| (px(site.position), width, site.points, site.p < SIGNIFICANT))
            .collect();
    }
    let mut found: Vec<Column> = Vec::new();
    let mut column: Option<(i64, f32, bool, f32)> = None;
    for site in shown {
        let here = px(site.position).floor() as i64;
        let marked = site.p < SIGNIFICANT;
        match &mut column {
            Some((at, sum, any, count)) if *at == here => {
                *sum += site.points;
                *any |= marked;
                *count += 1.0;
            }
            _ => {
                if let Some((at, sum, any, count)) = column {
                    found.push((at as f32, 1.0, sum / count, any));
                }
                column = Some((here, site.points, marked, 1.0));
            }
        }
    }
    if let Some((at, sum, any, count)) = column {
        found.push((at as f32, 1.0, sum / count, any));
    }
    found
}

/// The lane of the differences, with the label of the two groups, such as "HP 1 − HP 2".
pub fn lane(viewport: Viewport, differences: &[Difference], label: &str) -> Vec<Primitive> {
    let middle = HEIGHT / 2.0;
    let mut primitives = vec![Primitive::Rect {
        x: 0.0,
        y: middle,
        width: viewport.width() as f32,
        height: 1.0,
        paint: Paint::Line,
    }];
    for (x, width, points, marked) in columns(viewport, differences) {
        let points = points.clamp(-100.0, 100.0);
        let height = points.abs() / 100.0 * REACH;
        let up = points >= 0.0;
        let top = if up { middle - height } else { middle + 1.0 };
        if height > 0.0 {
            primitives.push(Primitive::Rect {
                x,
                y: top,
                width,
                height,
                paint: Paint::Change(points.round() as i8),
            });
        }
        if marked {
            let y = if up {
                top - DOT - 1.0
            } else {
                top + height + 1.0
            };
            primitives.push(Primitive::Rect {
                x: x + (width - DOT) / 2.0,
                y,
                width: DOT,
                height: DOT,
                paint: Paint::Text,
            });
        }
    }
    primitives.push(Primitive::Label {
        x: 4.0,
        y: LABEL_SIZE / 2.0 + 1.0,
        content: Cow::Owned(label.to_owned()),
        size: LABEL_SIZE,
        align: Align::Start,
    });
    primitives.push(Primitive::Label {
        x: viewport.width() as f32 - LABEL_INSET,
        y: LABEL_SIZE / 2.0 + 1.0,
        content: Cow::Borrowed(RANGE),
        size: LABEL_SIZE,
        align: Align::End,
    });
    primitives
}

/// The site under a place of the lane. Zoomed out, the first site of the pixel.
pub fn hit(viewport: Viewport, differences: &[Difference], x: f32) -> Option<Difference> {
    let found = if 1.0 / viewport.scale() >= 1.0 {
        let at = viewport.to_bp(f64::from(x)).floor().max(0.0) as u64;
        differences.iter().find(|site| site.position == at)
    } else {
        let column = x.floor() as i64;
        differences
            .iter()
            .find(|site| viewport.to_px(site.position as f64).floor() as i64 == column)
    };
    found.copied()
}

#[cfg(test)]
mod tests {
    use genie_core::methylation::{Difference, Site};
    use genie_core::{Region, Viewport};

    use super::{HEIGHT, REACH, hit, lane};
    use crate::scene::{Paint, Primitive};

    fn difference(position: u64, points: f32, p: f64) -> Difference {
        let site = Site {
            position,
            modified: 0,
            total: 1,
        };
        Difference {
            position,
            a: site,
            b: site,
            points,
            p,
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

    /// At 10 pixels a base, a bar goes up for group A and down for group B, and a dot shows
    /// where the test gives p < 0.05.
    #[test]
    fn bars_go_up_for_group_a_and_dots_mark_tests() {
        let viewport = Viewport::new(1_000, 100.0).show(Region::between(0, 10));
        let sites = [difference(2, 80.0, 0.001), difference(5, -50.0, 0.3)];
        let primitives = lane(viewport, &sites, "HP 1 − HP 2");
        let found = rects(&primitives);
        let middle = HEIGHT / 2.0;
        assert_eq!(found[0], (0.0, middle, 100.0, 1.0, Paint::Line));
        let up = found[1];
        assert_eq!((up.0, up.2, up.4), (20.0, 9.0, Paint::Change(80)));
        assert!((up.1 + up.3 - middle).abs() < 1e-4 && (up.3 - 0.8 * REACH).abs() < 1e-4);
        let dot = found[2];
        assert_eq!((dot.0, dot.2, dot.4), (23.0, 3.0, Paint::Text));
        assert!(dot.1 < up.1);
        let down = found[3];
        assert_eq!(
            (down.0, down.1, down.4),
            (50.0, middle + 1.0, Paint::Change(-50))
        );
        assert_eq!(found.len(), 4);
        let labels = primitives
            .iter()
            .filter(|primitive| matches!(primitive, Primitive::Label { .. }))
            .count();
        assert_eq!(labels, 2);
        assert_eq!(
            hit(viewport, &sites, 51.0).map(|site| site.position),
            Some(5)
        );
        assert_eq!(hit(viewport, &sites, 31.0), None);
    }

    /// Zoomed out, a pixel shows the mean of its sites, and a dot when one of them differs.
    #[test]
    fn pixels_show_the_mean_of_their_sites() {
        let viewport = Viewport::new(1_000, 10.0).show(Region::between(0, 1_000));
        let sites = [
            difference(10, 60.0, 0.5),
            difference(20, 20.0, 0.01),
            difference(500, -30.0, 0.2),
        ];
        let found = rects(&lane(viewport, &sites, "A − B"));
        assert_eq!(found[1].4, Paint::Change(40));
        assert_eq!(found[2].4, Paint::Text);
        assert_eq!(found[3].4, Paint::Change(-30));
        assert_eq!(found.len(), 4);
    }
}
