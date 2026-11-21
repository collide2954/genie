//! Lanes of cell types: the coverage of the reads or the fragments of each cell type of a
//! single-cell track, per cell, on one scale.

use std::borrow::Cow;

use genie_core::cells::{Depths, LANES};
use genie_core::{Region, Viewport, grouped};

use crate::scene::{Align, Font, Paint, Primitive};

/// The height of a lane.
pub const LANE_HEIGHT: f32 = 28.0;
/// The space at the top of a lane for its name.
const NAME_SPACE: f32 = 11.0;
const NAME_SIZE: f32 = 10.0;

/// The name and the number of cells of each lane. A lane without cells shows its mean depth.
pub type Lanes = [(String, usize)];

/// The part of the contig under a pixel column.
fn part_at(viewport: Viewport, px: f64) -> Region {
    let from = viewport.to_bp(px).floor().max(0.0) as u64;
    let to = viewport.to_bp(px + 1.0).ceil().max(0.0) as u64;
    Region::between(from, to.max(from + 1))
}

/// The value of a lane in a part: its mean depth per cell, or its mean depth without cells.
fn value(depths: &Depths, (lane, cells): (usize, usize), part: Region) -> f64 {
    let mean = depths.mean(lane, part);
    if cells == 0 {
        mean
    } else {
        mean / cells as f64
    }
}

/// The value of each lane at each pixel column of the view.
fn values(viewport: Viewport, depths: &Depths, lanes: &Lanes) -> Vec<Vec<f64>> {
    let columns = viewport.width().ceil() as usize;
    lanes
        .iter()
        .enumerate()
        .map(|(lane, (_, cells))| {
            (0..columns)
                .map(|px| value(depths, (lane, *cells), part_at(viewport, px as f64)))
                .collect()
        })
        .collect()
}

/// The text of the top of a scale: 2 significant digits for a small value.
pub fn scale_text(top: f64) -> String {
    if top >= 10.0 {
        grouped(top.round() as u64)
    } else if top >= 1.0 {
        format!("{top:.1}")
    } else {
        let digits = (-top.log10().floor()) as usize + 1;
        format!("{top:.digits$}")
    }
}

/// The paint of a lane: a color for each of the first lanes, grey for the lane "Other".
fn paint(lane: usize) -> Paint {
    if lane >= LANES {
        Paint::Line
    } else {
        Paint::Category(u8::try_from(lane).unwrap_or(0))
    }
}

/// The bars and the names of the lanes, each `LANE_HEIGHT` high, on one scale from 0 to the
/// largest value in view, with the top of the scale at the right of the first lane.
pub fn scene(viewport: Viewport, depths: &Depths, lanes: &Lanes) -> Vec<Primitive> {
    let values = values(viewport, depths, lanes);
    let top = values.iter().flatten().copied().fold(0.0_f64, f64::max);
    let room = LANE_HEIGHT - NAME_SPACE;
    let mut primitives = Vec::new();
    for (lane, ((name, cells), row)) in lanes.iter().zip(&values).enumerate() {
        let bottom = (lane + 1) as f32 * LANE_HEIGHT;
        let mut run: Option<(usize, f32)> = None;
        let close = |run: Option<(usize, f32)>, end: usize, primitives: &mut Vec<Primitive>| {
            if let Some((start, height)) = run.filter(|(_, height)| *height > 0.0) {
                primitives.push(Primitive::Rect {
                    x: start as f32,
                    y: bottom - height,
                    width: (end - start) as f32,
                    height,
                    paint: paint(lane),
                });
            }
        };
        for (px, &value) in row.iter().enumerate() {
            let height = if top > 0.0 {
                ((value / top) as f32 * room).max(if value > 0.0 { 1.0 } else { 0.0 })
            } else {
                0.0
            };
            match run {
                Some((_, known)) if (known - height).abs() < 0.5 => {}
                _ => {
                    close(run, px, &mut primitives);
                    run = Some((px, height));
                }
            }
        }
        close(run, row.len(), &mut primitives);
        let label = if *cells == 0 {
            name.clone()
        } else {
            let noun = if *cells == 1 { "cell" } else { "cells" };
            format!("{name} · {} {noun}", grouped(*cells as u64))
        };
        primitives.push(Primitive::Text {
            x: 4.0,
            y: bottom - LANE_HEIGHT + NAME_SPACE / 2.0,
            content: Cow::Owned(label),
            size: NAME_SIZE,
            paint: Paint::Muted,
            font: Font::Ui,
            align: Align::Start,
        });
    }
    if top > 0.0 && !lanes.is_empty() {
        primitives.push(Primitive::Text {
            x: viewport.width() as f32 - 4.0,
            y: NAME_SPACE / 2.0,
            content: Cow::Owned(format!("0-{}", scale_text(top))),
            size: NAME_SIZE,
            paint: Paint::Muted,
            font: Font::Ui,
            align: Align::End,
        });
    }
    primitives
}

/// The height of the lanes.
pub fn height(lanes: usize) -> f32 {
    lanes as f32 * LANE_HEIGHT
}

/// The lane under a point, the part of the contig under it, its mean depth there, and its
/// value per cell when the lane has cells.
pub fn hit(
    viewport: Viewport,
    depths: &Depths,
    lanes: &Lanes,
    (x, y): (f32, f32),
) -> Option<(usize, Region, f64, Option<f64>)> {
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let lane = (y / LANE_HEIGHT) as usize;
    let (_, cells) = lanes.get(lane)?;
    let part = part_at(viewport, f64::from(x).floor());
    let mean = depths.mean(lane, part);
    let per_cell = (*cells > 0).then(|| mean / *cells as f64);
    Some((lane, part, mean, per_cell))
}

#[cfg(test)]
mod tests {
    use genie_core::cells::Depths;
    use genie_core::{Region, Viewport};

    use super::{LANE_HEIGHT, hit, scale_text, scene};
    use crate::scene::{Paint, Primitive};

    #[test]
    fn lanes_share_one_scale_per_cell() {
        let mut depths = Depths::new(Region::between(0, 100), 2);
        depths.add_span(0, Region::between(0, 50));
        for _ in 0..4 {
            depths.add_span(1, Region::between(50, 100));
        }
        let lanes = [("T cell".to_owned(), 1), ("B cell".to_owned(), 2)];
        let viewport = Viewport::new(100, 100.0).show(Region::between(0, 100));
        let drawn = scene(viewport, &depths, &lanes);
        let bars: Vec<(f32, f32, f32, Paint)> = drawn
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect {
                    x,
                    width,
                    height,
                    paint,
                    ..
                } => Some((*x, *width, *height, *paint)),
                _ => None,
            })
            .collect();
        let room = LANE_HEIGHT - 11.0;
        assert_eq!(
            bars,
            [
                (0.0, 50.0, room / 2.0, Paint::Category(0)),
                (50.0, 50.0, room, Paint::Category(1)),
            ],
            "1 read per T cell is half of 4 reads for 2 B cells"
        );
        let texts: Vec<String> = drawn
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text { content, .. } => Some(content.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["T cell · 1 cell", "B cell · 2 cells", "0-2.0"]);
        let (lane, part, mean, per_cell) =
            hit(viewport, &depths, &lanes, (75.0, LANE_HEIGHT + 5.0)).unwrap();
        assert_eq!((lane, part), (1, Region::between(75, 76)));
        assert!((mean - 4.0).abs() < 1e-9 && per_cell == Some(2.0));
        assert_eq!(scale_text(0.0123), "0.012");
        assert_eq!(scale_text(1_234.0), "1,234");
    }
}
