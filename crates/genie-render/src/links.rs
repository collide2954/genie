//! The lane of interaction tracks: an arc from end to end for each link, taller for longer links
//! and thicker for stronger links. A link to another contig shows as a stub at its near end.

use genie_core::interaction::Interaction;
use genie_core::{Region, Viewport};

use crate::scene::{Hue, Paint, Primitive};

pub const HEIGHT: f32 = 72.0;
/// The height of the end blocks under the base line of the arcs.
const ENDS: f32 = 3.0;
const TOP: f32 = 3.0;
const LOWEST: f32 = 6.0;
const STUB: f32 = 12.0;
const KNOB: f32 = 4.0;
const THINNEST: f32 = 1.0;
const THICKEST: f32 = 3.0;
const PLAIN: f32 = 1.5;
const REACH: f32 = 4.0;

/// How one link is drawn in a lane of a height.
struct Shape {
    index: usize,
    paint: Paint,
    width: f32,
    from: f32,
    to: f32,
    /// The rise of the arc, or nothing for a stub to another contig.
    rise: Option<f32>,
    ends: Vec<(f32, f32)>,
}

fn middle(region: Region) -> f64 {
    region.start() as f64 + region.len() as f64 / 2.0
}

fn shapes(
    viewport: Viewport,
    links: &[Interaction],
    (hue, colors): (Hue, bool),
    height: f32,
) -> Vec<Shape> {
    let visible = viewport.visible();
    let shown: Vec<(usize, &Interaction)> = links
        .iter()
        .enumerate()
        .filter(|(_, link)| link.span().overlaps(visible))
        .collect();
    let longest = shown
        .iter()
        .filter_map(|(_, link)| link.distance())
        .max()
        .unwrap_or(1)
        .max(1);
    let strongest = shown
        .iter()
        .filter_map(|(_, link)| link.value)
        .fold(0.0, f64::max);
    let room = (height - ENDS - TOP - LOWEST).max(0.0);
    let px = |position: f64| viewport.to_px(position) as f32;
    let mut found: Vec<(f64, Shape)> = shown
        .into_iter()
        .map(|(index, link)| {
            let paint = match link.color {
                Some(color) if colors => Paint::Rgb(color),
                _ => Paint::Hue(hue),
            };
            let share = link
                .value
                .filter(|_| strongest > 0.0)
                .map(|value| (value / strongest).clamp(0.0, 1.0));
            let width = share.map_or(PLAIN, |share| {
                THINNEST + (THICKEST - THINNEST) * share as f32
            });
            let rise = link
                .distance()
                .map(|distance| LOWEST + room * (distance as f32 / longest as f32).sqrt());
            let mut ends = vec![link.near];
            if link.elsewhere.is_none() {
                ends.push(link.far);
            }
            let ends = ends
                .into_iter()
                .map(|end| {
                    let (left, right) = (px(end.start() as f64), px(end.end() as f64));
                    (left, (right - left).max(1.0))
                })
                .collect();
            let drawn = Shape {
                index,
                paint,
                width,
                from: px(middle(link.near)),
                to: px(middle(link.far)),
                rise,
                ends,
            };
            (share.unwrap_or(0.0), drawn)
        })
        .collect();
    found.sort_by(|(a, _), (b, _)| a.total_cmp(b));
    found.into_iter().map(|(_, shape)| shape).collect()
}

/// The arcs, end blocks and stubs of the links in the view, weakest first.
pub fn scene(
    viewport: Viewport,
    links: &[Interaction],
    look: (Hue, bool),
    height: f32,
) -> Vec<Primitive> {
    let base = height - ENDS;
    let mut primitives = Vec::new();
    for shape in shapes(viewport, links, look, height) {
        for (x, width) in &shape.ends {
            primitives.push(Primitive::Rect {
                x: *x,
                y: base,
                width: *width,
                height: ENDS,
                paint: shape.paint,
            });
        }
        if let Some(rise) = shape.rise {
            primitives.push(Primitive::Arc {
                from: shape.from,
                to: shape.to,
                base,
                rise,
                width: shape.width,
                paint: shape.paint,
            });
        } else {
            primitives.push(Primitive::Rect {
                x: shape.from - shape.width / 2.0,
                y: base - STUB,
                width: shape.width,
                height: STUB,
                paint: shape.paint,
            });
            primitives.push(Primitive::Rect {
                x: shape.from - KNOB / 2.0,
                y: base - STUB - KNOB,
                width: KNOB,
                height: KNOB,
                paint: shape.paint,
            });
        }
    }
    primitives
}

/// The link under a point of the lane: the arc nearest to the point, or an end or a stub.
pub fn hit(
    viewport: Viewport,
    links: &[Interaction],
    height: f32,
    x: f32,
    y: f32,
) -> Option<usize> {
    let base = height - ENDS;
    let mut best: Option<(f32, usize)> = None;
    for shape in shapes(viewport, links, (Hue::Blue, false), height) {
        let on_end = y >= base - 1.0
            && shape
                .ends
                .iter()
                .any(|(left, width)| x >= left - 1.0 && x <= left + width + 1.0);
        let distance = if on_end {
            Some(0.0)
        } else {
            match shape.rise {
                Some(rise) if shape.to > shape.from && x >= shape.from && x <= shape.to => {
                    let t = (x - shape.from) / (shape.to - shape.from);
                    Some((base - 4.0 * rise * t * (1.0 - t) - y).abs())
                }
                None if (x - shape.from).abs() <= REACH && y >= base - STUB - KNOB => Some(0.0),
                _ => None,
            }
        };
        if let Some(distance) = distance.filter(|distance| *distance <= REACH)
            && best.is_none_or(|(known, _)| distance <= known)
        {
            best = Some((distance, shape.index));
        }
    }
    best.map(|(_, index)| index)
}

#[cfg(test)]
mod tests {
    use genie_core::interaction::Interaction;
    use genie_core::{Region, Viewport};

    use super::{HEIGHT, hit, scene};
    use crate::scene::{Hue, Paint, Primitive};

    fn link(near: (u64, u64), far: (u64, u64), value: f64, color: Option<[u8; 3]>) -> Interaction {
        Interaction {
            value: Some(value),
            color,
            ..Interaction::new(
                Region::between(near.0, near.1),
                Region::between(far.0, far.1),
                None,
            )
        }
    }

    fn links() -> Vec<Interaction> {
        let far = Interaction::new(
            Region::between(700, 720),
            Region::between(5, 9),
            Some("chr2".to_owned()),
        );
        vec![
            link((100, 120), (300, 320), 10.0, Some([200, 0, 0])),
            link((100, 120), (900, 920), 40.0, None),
            far,
            link((5_000, 5_010), (6_000, 6_010), 99.0, None),
        ]
    }

    fn view() -> Viewport {
        Viewport::new(10_000, 1_000.0).show(Region::between(0, 1_000))
    }

    fn arcs(primitives: &[Primitive]) -> Vec<(f32, f32, f32, f32, Paint)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Arc {
                    from,
                    to,
                    rise,
                    width,
                    paint,
                    ..
                } => Some((*from, *to, *rise, *width, *paint)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn longer_links_rise_higher_and_stronger_links_are_thicker() {
        let drawn = scene(view(), &links(), (Hue::Green, true), HEIGHT);
        let found = arcs(&drawn);
        assert_eq!(found.len(), 2);
        let (short, long) = (found[0], found[1]);
        assert_eq!((short.0, short.1), (110.0, 310.0));
        assert_eq!((long.0, long.1), (110.0, 910.0));
        assert!(long.2 > short.2 && long.2 <= HEIGHT - 6.0, "{long:?}");
        assert!(long.3 > short.3, "{short:?} {long:?}");
        assert_eq!(short.4, Paint::Rgb([200, 0, 0]));
        assert_eq!(long.4, Paint::Hue(Hue::Green));
        let plain = arcs(&scene(view(), &links(), (Hue::Green, false), HEIGHT));
        assert_eq!(plain[0].4, Paint::Hue(Hue::Green));
        let blocks = drawn
            .iter()
            .filter(|primitive| matches!(primitive, Primitive::Rect { .. }))
            .count();
        assert_eq!(blocks, 2 + 2 + 1 + 2, "{drawn:?}");
    }

    #[test]
    fn far_links_show_a_stub_and_links_off_the_view_still_cross_it() {
        let wide = scene(view(), &links(), (Hue::Blue, false), HEIGHT);
        let stub = wide.iter().any(|primitive| {
            matches!(
                primitive,
                Primitive::Rect { x, height, .. }
                    if (*x - 709.25).abs() < 0.01 && (*height - 12.0).abs() < 0.01
            )
        });
        assert!(stub, "{wide:?}");
        let inside = Viewport::new(10_000, 1_000.0).show(Region::between(400, 600));
        let crossing = arcs(&scene(inside, &links(), (Hue::Blue, false), HEIGHT));
        assert_eq!(crossing.len(), 1);
        assert!(
            crossing[0].0 < 0.0 && crossing[0].1 > 1_000.0,
            "{crossing:?}"
        );
    }

    #[test]
    fn hits_find_the_arc_an_end_or_a_stub_under_the_pointer() {
        let links = links();
        let drawn = arcs(&scene(view(), &links, (Hue::Blue, false), HEIGHT));
        let (_, _, rise, _, _) = drawn[1];
        let base = HEIGHT - 3.0;
        assert_eq!(
            hit(view(), &links, HEIGHT, 510.0, base - rise + 1.0),
            Some(1)
        );
        assert_eq!(hit(view(), &links, HEIGHT, 310.0, base + 1.0), Some(0));
        assert_eq!(hit(view(), &links, HEIGHT, 710.0, base - 14.0), Some(2));
        assert_eq!(hit(view(), &links, HEIGHT, 510.0, 40.0), None);
    }
}
