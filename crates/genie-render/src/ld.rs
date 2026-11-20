//! The LD lane of a variant track: under the sites, a triangle with a cell for each pair of
//! variants in view, in the color of their r², from light grey for 0 to vermillion for 1. Each
//! variant has a slot from the middle between it and the variant before it to the middle
//! between it and the variant after it. The cell of two variants is their two slots turned by
//! 45 degrees, as in Haploview and the Hi-C maps: under the middle between them, and lower for
//! variants farther apart. The lane shows the cells that start in its height.

use std::borrow::Cow;

use genie_core::Viewport;
use genie_core::ld::Matrix;

use crate::scene::{Align, Font, LABEL_INSET, Paint, Primitive};

/// The height of the lane.
pub const HEIGHT: f32 = 120.0;
/// The most variants in view that the lane shows.
pub const MOST: usize = 200;
/// What the lane says when it has no LD to show.
pub const ZOOM_HINT: &str = "Zoom in to 200 variants or less to see LD";
const LABEL_SIZE: f32 = 10.0;
const RANGE: &str = "r² 0-1";

/// The variants in view, by their place in `starts`, with the left and the right edge of the
/// slot of each.
fn slots(viewport: Viewport, starts: &[u64]) -> Vec<(usize, f32, f32)> {
    let visible = viewport.visible();
    let shown: Vec<(usize, f32)> = starts
        .iter()
        .enumerate()
        .filter(|(_, start)| visible.contains(**start))
        .map(|(index, start)| (index, viewport.to_px(*start as f64 + 0.5) as f32))
        .collect();
    let count = shown.len();
    (0..count)
        .map(|at| {
            let (index, middle) = shown[at];
            let before = (at > 0).then(|| f32::midpoint(shown[at - 1].1, middle));
            let after = (at + 1 < count).then(|| f32::midpoint(middle, shown[at + 1].1));
            let left = before
                .or_else(|| after.map(|right| 2.0 * middle - right))
                .unwrap_or(middle - 2.0);
            let right = after.unwrap_or(2.0 * middle - left);
            (index, left, right)
        })
        .collect()
}

/// The lane of the LD of the variants in view, with `starts` the start of each variant of the
/// matrix, and its label, such as "LD r² · EUR". Without a matrix or with more than `MOST`
/// variants in view, the lane tells to zoom in.
pub fn lane(
    viewport: Viewport,
    starts: &[u64],
    matrix: Option<&Matrix>,
    label: &str,
) -> Vec<Primitive> {
    let width = viewport.width() as f32;
    let slots = slots(viewport, starts);
    let mut primitives = Vec::new();
    match matrix.filter(|_| slots.len() <= MOST) {
        Some(matrix) => {
            for (at, &(first, left_a, right_a)) in slots.iter().enumerate() {
                for &(second, left_b, right_b) in &slots[at + 1..] {
                    let top = (left_b - right_a) / 2.0;
                    if top >= HEIGHT {
                        break;
                    }
                    let Some(r2) = matrix.get(first, second) else {
                        continue;
                    };
                    primitives.push(Primitive::Ribbon {
                        corners: [
                            (f32::midpoint(right_a, left_b), top),
                            (f32::midpoint(right_a, right_b), (right_b - right_a) / 2.0),
                            (f32::midpoint(left_a, right_b), (right_b - left_a) / 2.0),
                            (f32::midpoint(left_a, left_b), (left_b - left_a) / 2.0),
                        ],
                        paint: Paint::Change((r2 * 100.0).round() as i8),
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
    let bottom = HEIGHT - LABEL_SIZE / 2.0 - 1.0;
    primitives.push(Primitive::Label {
        x: 4.0,
        y: bottom,
        content: Cow::Owned(label.to_owned()),
        size: LABEL_SIZE,
        align: Align::Start,
    });
    primitives.push(Primitive::Label {
        x: width - LABEL_INSET,
        y: bottom,
        content: Cow::Borrowed(RANGE),
        size: LABEL_SIZE,
        align: Align::End,
    });
    primitives
}

/// The two variants, by their place in `starts`, of the cell under a point of the lane.
pub fn hit(viewport: Viewport, starts: &[u64], (x, y): (f32, f32)) -> Option<(usize, usize)> {
    if !(0.0..HEIGHT).contains(&y) {
        return None;
    }
    let slots = slots(viewport, starts);
    if slots.len() > MOST {
        return None;
    }
    let find = |value: f32| {
        slots
            .iter()
            .position(|&(_, left, right)| left <= value && value < right)
    };
    let (first, second) = (find(x - y)?, find(x + y)?);
    (first < second).then(|| (slots[first].0, slots[second].0))
}

#[cfg(test)]
mod tests {
    use genie_core::ld::Matrix;
    use genie_core::variant::Call::{Alternate, Het, Reference};
    use genie_core::{Region, Viewport};

    use super::{HEIGHT, ZOOM_HINT, hit, lane};
    use crate::scene::{Paint, Primitive};

    fn ribbons(primitives: &[Primitive]) -> Vec<([(f32, f32); 4], Paint)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Ribbon { corners, paint } => Some((*corners, *paint)),
                _ => None,
            })
            .collect()
    }

    /// Variants at 10, 20 and 40 at a pixel a base: the first two have r² 1, the last one has
    /// r² 0.5 with each.
    fn matrix() -> Matrix {
        let a = [Reference, Het, Alternate, Het];
        let half = [Reference, Reference, Alternate, Alternate];
        Matrix::new(&[&a, &a, &half], None)
    }

    #[test]
    fn pairs_of_variants_are_cells_under_the_middle_between_them() {
        let viewport = Viewport::new(1_000, 100.0).show(Region::between(0, 100));
        let starts = [10, 20, 40];
        let found = ribbons(&lane(viewport, &starts, Some(&matrix()), "LD r² · all"));
        assert_eq!(found.len(), 3);
        assert_eq!(
            found[0],
            (
                [(15.5, 0.0), (23.0, 7.5), (18.0, 12.5), (10.5, 5.0)],
                Paint::Change(100)
            )
        );
        assert_eq!(found[1].1, Paint::Change(50));
        assert_eq!(found[2].1, Paint::Change(50));
        assert_eq!(hit(viewport, &starts, (17.0, 6.25)), Some((0, 1)));
        assert_eq!(hit(viewport, &starts, (30.0, 12.0)), Some((1, 2)));
        assert_eq!(hit(viewport, &starts, (40.0, 5.0)), None);
        assert_eq!(hit(viewport, &starts, (40.0, HEIGHT + 1.0)), None);
    }

    #[test]
    fn lanes_without_a_matrix_or_with_many_variants_tell_to_zoom_in() {
        let viewport = Viewport::new(100_000, 100.0).show(Region::between(0, 100_000));
        let many: Vec<u64> = (0..300).map(|index| index * 100).collect();
        for matrix in [None, Some(&Matrix::default())] {
            let primitives = lane(viewport, &many, matrix, "LD");
            let hint = primitives.iter().any(|primitive| {
                matches!(primitive, Primitive::Text { content, .. } if content == ZOOM_HINT)
            });
            assert!(hint);
            assert_eq!(ribbons(&primitives).len(), 0);
        }
    }
}
