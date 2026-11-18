//! The marked regions of a figure: a band for each region in view, with its name, in rows.

use std::borrow::Cow;

use genie_core::{Region, Viewport};

use crate::scene::{Align, Font, Paint, Primitive};

/// The height of one row of bands.
pub const ROW_HEIGHT: f32 = 14.0;
const BAND_HEIGHT: f32 = 10.0;
const NAME_SIZE: f32 = 10.0;
const GLYPH_WIDTH: f32 = 0.6;
const INSET: f32 = 3.0;
const GAP: f32 = 6.0;
const LEAST: f32 = 2.0;
const MOST_ROWS: usize = 4;
/// The grey of a band: a quarter of the way from the color of lines to that of text.
const INK: u8 = 25;

struct Band<'a> {
    left: f32,
    width: f32,
    name: &'a str,
    /// Where the name goes, its alignment and if it is in the band.
    label: Option<(f32, Align, bool)>,
    /// The pixels that the band and its name use.
    reach: (f32, f32),
}

fn place(viewport: Viewport, span: Region, name: &str) -> Option<Band<'_>> {
    let edge = viewport.width() as f32;
    let left = viewport.to_px(span.start() as f64) as f32;
    let right = viewport.to_px(span.end() as f64) as f32;
    if right < 0.0 || left > edge {
        return None;
    }
    let left = left.max(0.0);
    let width = (right.min(edge) - left).max(LEAST);
    let end = left + width;
    let size = name.chars().count() as f32 * NAME_SIZE * GLYPH_WIDTH;
    let (label, reach) = if name.is_empty() {
        (None, (left, end))
    } else if size + 2.0 * INSET <= width {
        (Some((left + INSET, Align::Start, true)), (left, end))
    } else if end + INSET + size <= edge || left - INSET - size < 0.0 {
        let label = (end + INSET, Align::Start, false);
        (Some(label), (left, end + INSET + size))
    } else {
        let label = (left - INSET, Align::End, false);
        (Some(label), (left - INSET - size, end))
    };
    Some(Band {
        left,
        width,
        name,
        label,
        reach,
    })
}

/// The first row with room from `start` on. After the last row, the row that ends first.
fn row(edges: &mut Vec<f32>, start: f32) -> usize {
    if let Some(row) = edges.iter().position(|edge| edge + GAP <= start) {
        return row;
    }
    if edges.len() < MOST_ROWS {
        edges.push(f32::NEG_INFINITY);
        return edges.len() - 1;
    }
    (0..edges.len())
        .min_by(|a, b| edges[*a].total_cmp(&edges[*b]))
        .unwrap_or(0)
}

/// The bands of the regions in view, in rows so that no two bands or names touch, and the number
/// of rows: 0 when no region is in view. A name goes in its band when it fits, else next to it.
pub fn scene(viewport: Viewport, regions: &[(Region, &str)]) -> (Vec<Primitive>, usize) {
    let mut bands: Vec<Band> = regions
        .iter()
        .filter_map(|(span, name)| place(viewport, *span, name))
        .collect();
    bands.sort_by(|a, b| a.reach.0.total_cmp(&b.reach.0));
    let mut edges = Vec::new();
    let mut primitives = Vec::new();
    for band in bands {
        let row = row(&mut edges, band.reach.0);
        edges[row] = edges[row].max(band.reach.1);
        let top = row as f32 * ROW_HEIGHT + (ROW_HEIGHT - BAND_HEIGHT) / 2.0;
        primitives.push(Primitive::Rect {
            x: band.left,
            y: top,
            width: band.width,
            height: BAND_HEIGHT,
            paint: Paint::Stain(INK),
        });
        if let Some((x, align, inside)) = band.label {
            primitives.push(Primitive::Text {
                x,
                y: top + BAND_HEIGHT / 2.0,
                content: Cow::Owned(band.name.to_owned()),
                size: NAME_SIZE,
                paint: if inside { Paint::Text } else { Paint::Muted },
                font: Font::Ui,
                align,
            });
        }
    }
    (primitives, edges.len())
}

#[cfg(test)]
mod tests {
    use genie_core::{Region, Viewport};

    use super::{ROW_HEIGHT, scene};
    use crate::scene::{Align, Paint, Primitive};

    fn view() -> Viewport {
        Viewport::new(10_000, 1_000.0).show(Region::between(0, 1_000))
    }

    fn bands(primitives: &[Primitive]) -> Vec<(f32, f32, f32)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect { x, y, width, .. } => Some((*x, *y, *width)),
                _ => None,
            })
            .collect()
    }

    fn names(primitives: &[Primitive]) -> Vec<(String, f32, Paint, Align)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text {
                    x,
                    content,
                    paint,
                    align,
                    ..
                } => Some((content.to_string(), *x, *paint, *align)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn regions_in_view_become_bands_with_their_names() {
        let regions = [
            (Region::between(100, 300), "Promoter"),
            (Region::between(500, 505), "Enhancer"),
            (Region::between(5_000, 6_000), "Away"),
        ];
        let (primitives, rows) = scene(view(), &regions);
        assert_eq!(rows, 1);
        assert_eq!(bands(&primitives), [(100.0, 2.0, 200.0), (500.0, 2.0, 5.0)]);
        assert_eq!(
            names(&primitives),
            [
                ("Promoter".to_owned(), 103.0, Paint::Text, Align::Start),
                ("Enhancer".to_owned(), 508.0, Paint::Muted, Align::Start),
            ]
        );
    }

    #[test]
    fn overlapping_regions_take_more_rows() {
        let regions = [
            (Region::between(100, 300), "A"),
            (Region::between(200, 400), "B"),
            (Region::between(600, 700), "C"),
        ];
        let (primitives, rows) = scene(view(), &regions);
        assert_eq!(rows, 2);
        let tops: Vec<f32> = bands(&primitives).iter().map(|band| band.1).collect();
        assert_eq!(tops, [2.0, ROW_HEIGHT + 2.0, 2.0]);
    }

    #[test]
    fn names_at_the_right_edge_go_left_of_their_band() {
        let (primitives, _) = scene(view(), &[(Region::between(990, 995), "Last region")]);
        assert_eq!(
            names(&primitives),
            [("Last region".to_owned(), 987.0, Paint::Muted, Align::End)]
        );
    }

    #[test]
    fn tiny_regions_stay_visible_and_long_ones_end_at_the_edges() {
        let regions = [
            (Region::between(2_040, 2_041), ""),
            (Region::between(1_000, 9_000), "Long"),
        ];
        let (primitives, rows) = scene(view().show(Region::between(2_000, 6_000)), &regions);
        assert_eq!(rows, 2);
        assert_eq!(
            bands(&primitives),
            [(0.0, 2.0, 1_000.0), (10.0, ROW_HEIGHT + 2.0, 2.0)]
        );
        assert_eq!(names(&primitives).len(), 1);
    }

    #[test]
    fn no_region_in_view_gives_no_rows() {
        let (primitives, rows) = scene(view(), &[(Region::between(5_000, 6_000), "Far")]);
        assert_eq!(primitives, []);
        assert_eq!(rows, 0);
    }

    #[test]
    fn rows_stop_at_four() {
        let regions: Vec<(Region, &str)> = (0..6)
            .map(|index| (Region::between(100 + index, 300), "Same"))
            .collect();
        let (primitives, rows) = scene(view(), &regions);
        assert_eq!(rows, 4);
        assert_eq!(bands(&primitives).len(), 6);
    }
}
