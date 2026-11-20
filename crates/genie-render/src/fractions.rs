//! The B-allele frequency lane of a variant track: a dot for each site of one sample, at the
//! fraction of the reads of the site that carry an allele that is not the reference allele. The
//! fraction goes from 0 at the bottom to 1 at the top, with a line at 0.5. Zoomed out, each
//! pixel shows a height once.

use std::borrow::Cow;
use std::collections::HashSet;

use genie_core::Viewport;
use genie_core::variant::Fraction;

use crate::scene::{Align, Font, Hue, LABEL_INSET, Paint, Primitive};

/// The height of the lane.
pub const HEIGHT: f32 = 40.0;
/// What the lane says when the data of the view is too large to read.
pub const ZOOM_HINT: &str = "Zoom in to see the B-allele frequency";
const DOT: f32 = 3.0;
/// The space above the dots of 1 and under the dots of 0.
const INSET: f32 = 1.0;
const LABEL_SIZE: f32 = 10.0;
/// The farthest that the pointer can be from the edge of a dot to show its card.
const REACH: f32 = 3.0;
const RANGE: &str = "0-1";

/// The top of the dot of a fraction.
fn top(value: f32) -> f32 {
    INSET + (1.0 - value.clamp(0.0, 1.0)) * (HEIGHT - 2.0 * INSET - DOT)
}

/// The middle of the dot of a site, across.
fn middle(viewport: Viewport, position: u64) -> f32 {
    viewport.to_px(position as f64 + 0.5) as f32
}

/// The lane of the fractions of a sample, with its label, such as "B-allele frequency ·
/// NA12878". Without fractions, the lane tells to zoom in.
pub fn lane(
    viewport: Viewport,
    fractions: Option<&[Fraction]>,
    hue: Hue,
    label: &str,
) -> Vec<Primitive> {
    let width = viewport.width() as f32;
    let mut primitives = vec![Primitive::Rect {
        x: 0.0,
        y: (top(0.5) + DOT / 2.0).floor(),
        width,
        height: 1.0,
        paint: Paint::Line,
    }];
    match fractions {
        Some(fractions) => {
            let visible = viewport.visible();
            let first = fractions.partition_point(|site| site.position < visible.start());
            let mut drawn = HashSet::new();
            for site in fractions[first..]
                .iter()
                .take_while(|site| site.position < visible.end())
            {
                let (x, y) = (middle(viewport, site.position) - DOT / 2.0, top(site.value));
                if drawn.insert((x.round() as i32, y.round() as i32)) {
                    primitives.push(Primitive::Rect {
                        x,
                        y,
                        width: DOT,
                        height: DOT,
                        paint: Paint::Hue(hue),
                    });
                }
            }
        }
        None => primitives.push(Primitive::Text {
            x: width / 2.0,
            y: HEIGHT / 2.0,
            content: Cow::Borrowed(ZOOM_HINT),
            size: LABEL_SIZE,
            paint: Paint::Muted,
            font: Font::Ui,
            align: Align::Center,
        }),
    }
    primitives.push(Primitive::Label {
        x: 4.0,
        y: LABEL_SIZE / 2.0 + 1.0,
        content: Cow::Owned(label.to_owned()),
        size: LABEL_SIZE,
        align: Align::Start,
    });
    primitives.push(Primitive::Label {
        x: width - LABEL_INSET,
        y: LABEL_SIZE / 2.0 + 1.0,
        content: Cow::Borrowed(RANGE),
        size: LABEL_SIZE,
        align: Align::End,
    });
    primitives
}

/// The site whose dot is nearest to a point of the lane, when the point is at most `REACH`
/// pixels from the dot.
pub fn hit(viewport: Viewport, fractions: &[Fraction], (x, y): (f32, f32)) -> Option<usize> {
    let far = REACH + DOT;
    let low = viewport.to_bp(f64::from(x - far)).floor().max(0.0) as u64;
    let high = viewport.to_bp(f64::from(x + far)).ceil().max(0.0) as u64;
    let first = fractions.partition_point(|site| site.position < low);
    fractions[first..]
        .iter()
        .enumerate()
        .take_while(|(_, site)| site.position <= high)
        .map(|(index, site)| {
            let across = (middle(viewport, site.position) - x).abs();
            let down = (top(site.value) + DOT / 2.0 - y).abs();
            (first + index, across.max(down))
        })
        .filter(|(_, distance)| *distance <= REACH + DOT / 2.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use genie_core::variant::{Call, Fraction};
    use genie_core::{Region, Viewport};

    use super::{HEIGHT, ZOOM_HINT, hit, lane, top};
    use crate::scene::{Hue, Paint, Primitive};

    fn site(position: u64, value: f32) -> Fraction {
        Fraction {
            position,
            value,
            depth: Some(30),
            call: Call::Het,
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

    /// At 10 pixels a base, each site has a dot in the middle of its base, at the height of
    /// its fraction, and a line marks 0.5.
    #[test]
    fn dots_show_the_fraction_of_each_site() {
        let viewport = Viewport::new(1_000, 100.0).show(Region::between(0, 10));
        let sites = [site(2, 0.5), site(5, 1.0), site(8, 0.0), site(20, 0.5)];
        let primitives = lane(
            viewport,
            Some(&sites),
            Hue::Green,
            "B-allele frequency · NA1",
        );
        let found = rects(&primitives);
        assert_eq!(found[0], (0.0, 20.0, 100.0, 1.0, Paint::Line));
        let dot = Paint::Hue(Hue::Green);
        assert_eq!(found[1], (23.5, 18.5, 3.0, 3.0, dot));
        assert_eq!(found[2], (53.5, 1.0, 3.0, 3.0, dot));
        assert_eq!(found[3], (83.5, HEIGHT - 4.0, 3.0, 3.0, dot));
        assert_eq!(found.len(), 4);
        let labels: Vec<String> = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Label { content, .. } => Some(content.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(labels, ["B-allele frequency · NA1", "0-1"]);
        assert_eq!(hit(viewport, &sites, (25.0, 20.0)), Some(0));
        assert_eq!(hit(viewport, &sites, (56.0, 4.5)), Some(1));
        assert_eq!(hit(viewport, &sites, (25.0, 4.0)), None);
        assert_eq!(hit(viewport, &sites, (70.0, 20.0)), None);
    }

    /// Zoomed out, sites in one pixel at the same height give one dot.
    #[test]
    fn pixels_show_each_height_once() {
        let viewport = Viewport::new(1_000_000, 100.0).show(Region::between(0, 1_000_000));
        let sites = [
            site(100, 0.5),
            site(200, 0.5),
            site(300, 0.5),
            site(400, 0.0),
        ];
        let found = rects(&lane(viewport, Some(&sites), Hue::Blue, "NA1"));
        assert_eq!(found.len(), 3);
        assert!((found[1].1 - top(0.5)).abs() < 1e-4);
        assert!((found[2].1 - top(0.0)).abs() < 1e-4);
    }

    #[test]
    fn lanes_without_fractions_tell_to_zoom_in() {
        let viewport = Viewport::new(1_000, 100.0).show(Region::between(0, 1_000));
        let primitives = lane(viewport, None, Hue::Blue, "NA1");
        let hint = primitives.iter().any(|primitive| {
            matches!(primitive, Primitive::Text { content, .. } if content == ZOOM_HINT)
        });
        assert!(hint);
        assert_eq!(rects(&primitives).len(), 1);
    }
}
