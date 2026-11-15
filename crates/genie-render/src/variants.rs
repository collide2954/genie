use genie_core::Viewport;
use genie_core::variant::{Call, Kind, Variant};

use crate::features::Mode;
use crate::samples::Rows;
use crate::scene::{Allele, Base, Paint, Primitive, Shade};

pub const SITE_HEIGHT: f32 = 12.0;
pub const LIMIT: usize = 100;
pub const GROUP_LIMIT: usize = 300;

const REACH: f32 = 3.0;
const NARROWEST: f32 = 3.0;

pub fn fit(samples: usize) -> Mode {
    if samples <= LIMIT {
        Mode::Expanded
    } else {
        Mode::Squished
    }
}

pub fn row_height(mode: Mode) -> f32 {
    shape(mode).0
}

fn shape(mode: Mode) -> (f32, f32, f32) {
    match mode {
        Mode::Expanded => (10.0, 1.0, 8.0),
        Mode::Squished => (3.0, 0.0, 2.0),
        Mode::Collapsed => (0.0, 0.0, 0.0),
    }
}

fn allele(variant: &Variant) -> Allele {
    match variant.kind() {
        Kind::Snv(base) => Allele::Base(Base::from_byte(base)),
        Kind::Insertion => Allele::Insertion,
        Kind::Deletion => Allele::Deletion,
        Kind::Complex => Allele::Other,
    }
}

fn strength(call: Call) -> u8 {
    match call {
        Call::Missing => 0,
        Call::Reference => 1,
        Call::Het => 2,
        Call::Alternate => 3,
    }
}

/// The number of samples that share a row, so that a big cohort has at most about
/// `GROUP_LIMIT` rows.
pub fn group(samples: usize) -> usize {
    samples.div_ceil(GROUP_LIMIT).max(1)
}

/// The call of a row of samples: the strongest of their calls.
pub fn strongest(calls: impl IntoIterator<Item = Call>) -> Call {
    calls
        .into_iter()
        .max_by_key(|call| strength(*call))
        .unwrap_or(Call::Missing)
}

pub fn scene(viewport: Viewport, variants: &[Variant], mode: Mode, rows: &Rows) -> Vec<Primitive> {
    let (row, offset, mark) = shape(mode);
    let visible = viewport.visible();
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    let members: &[Vec<usize>] = if mode == Mode::Collapsed {
        &[]
    } else {
        &rows.members
    };
    let mut drawn: Vec<(i64, u8)> = vec![(i64::MIN, 0); members.len() + 1];
    let mut primitives = Vec::new();
    for variant in variants {
        let span = variant.span;
        if span.start() > visible.end() || span.end() < visible.start() {
            continue;
        }
        let left = px(span.start());
        let extent = px(span.end()) - left;
        let width = if extent > NARROWEST + 1.0 {
            extent - 1.0
        } else {
            NARROWEST
        };
        let x = left + (extent - width).min(0.0) / 2.0;
        let column = left.floor() as i64;
        let narrow = extent <= 1.0;
        let allele = allele(variant);
        let shade = if variant.passes() {
            Shade::Full
        } else {
            Shade::Faded
        };
        let mut place = |slot: usize, power: u8| {
            let (last, strongest) = drawn[slot];
            if narrow && last == column && power <= strongest {
                return false;
            }
            drawn[slot] = (
                column,
                if last == column {
                    power.max(strongest)
                } else {
                    power
                },
            );
            true
        };
        if place(0, 1) {
            primitives.push(Primitive::Rect {
                x,
                y: 1.0,
                width,
                height: SITE_HEIGHT - 2.0,
                paint: Paint::Allele(allele, shade),
            });
        }
        for (index, part) in members.iter().enumerate() {
            let call = strongest(
                part.iter()
                    .filter_map(|&sample| variant.calls.get(sample).copied()),
            );
            let paint = match call {
                Call::Missing => continue,
                Call::Reference => Paint::HomRef,
                Call::Het => Paint::Allele(allele, Shade::Half),
                Call::Alternate => Paint::Allele(allele, Shade::Full),
            };
            if place(index + 1, strength(call)) {
                primitives.push(Primitive::Rect {
                    x,
                    y: SITE_HEIGHT + 1.0 + rows.top(index, row) + offset,
                    width,
                    height: mark,
                    paint,
                });
            }
        }
    }
    if !members.is_empty() {
        let lowered = SITE_HEIGHT + 1.0 + offset;
        primitives.extend(
            rows.marks(row, row - mark)
                .into_iter()
                .map(|primitive| primitive.lowered(lowered)),
        );
    }
    primitives
}

pub fn hit(
    viewport: Viewport,
    variants: &[Variant],
    mode: Mode,
    rows: &Rows,
    x: f32,
    y: f32,
) -> Option<(usize, Option<usize>)> {
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    let (index, _) = variants
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            let left = px(variant.span.start());
            let right = px(variant.span.end()).max(left + 1.0);
            (index, (left - x).max(x - right).max(0.0))
        })
        .filter(|(_, distance)| *distance <= REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    if y < SITE_HEIGHT {
        return Some((index, None));
    }
    let (row, ..) = shape(mode);
    if row <= 0.0 {
        return None;
    }
    let line = rows.at(y - SITE_HEIGHT - 1.0, row)?;
    Some((index, Some(line)))
}

#[cfg(test)]
mod tests {
    use genie_core::samples::{Arrangement, Group, Tint};
    use genie_core::variant::{Annotations, Call, Variant};
    use genie_core::{Region, Viewport};

    use super::{SITE_HEIGHT, group, hit, row_height, scene};
    use crate::features::Mode;
    use crate::samples::{GROUP_GAP, Rows, STRIP};
    use crate::scene::{Allele, Base, Paint, Primitive, Shade};

    fn plain(samples: usize) -> Rows {
        Rows::plain(samples, group(samples))
    }

    fn view(start: u64, end: u64) -> Viewport {
        Viewport::new(1_000_000, 100.0).show(Region::between(start, end))
    }

    fn variant(start: u64, reference: &str, alternate: &str, calls: &[Call]) -> Variant {
        Variant {
            id: format!("v{start}"),
            span: Region::between(start, start + reference.len() as u64),
            reference: reference.to_owned(),
            alternates: vec![alternate.to_owned()],
            quality: None,
            filter: "PASS".to_owned(),
            info: String::new(),
            annotations: Annotations::default(),
            structure: None,
            calls: calls.to_vec(),
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

    #[test]
    fn sites_take_the_color_of_their_change() {
        let mut low = variant(30, "A", "C", &[]);
        low.filter = "LowQual".to_owned();
        let sites = [
            variant(10, "A", "G", &[]),
            variant(20, "A", "ACG", &[]),
            low,
            variant(40, "ACGTA", "A", &[]),
        ];
        let found = rects(&scene(view(0, 100), &sites, Mode::Collapsed, &plain(0)));
        assert_eq!(
            found,
            [
                (
                    [9.0, 1.0, 3.0, 10.0],
                    Paint::Allele(Allele::Base(Base::G), Shade::Full)
                ),
                (
                    [19.0, 1.0, 3.0, 10.0],
                    Paint::Allele(Allele::Insertion, Shade::Full)
                ),
                (
                    [29.0, 1.0, 3.0, 10.0],
                    Paint::Allele(Allele::Base(Base::C), Shade::Faded)
                ),
                (
                    [40.0, 1.0, 4.0, 10.0],
                    Paint::Allele(Allele::Deletion, Shade::Full)
                ),
            ]
        );
    }

    #[test]
    fn genotypes_fill_one_row_per_sample() {
        let genotypes = [Call::Het, Call::Reference, Call::Missing, Call::Alternate];
        let sites = [variant(10, "A", "T", &genotypes)];
        let found = rects(&scene(view(0, 100), &sites, Mode::Expanded, &plain(4)));
        let rows: Vec<([f32; 4], Paint)> = found
            .into_iter()
            .filter(|(rect, _)| rect[1] >= SITE_HEIGHT)
            .collect();
        let top = SITE_HEIGHT + 1.0;
        assert_eq!(
            rows,
            [
                (
                    [9.0, top + 1.0, 3.0, 8.0],
                    Paint::Allele(Allele::Base(Base::T), Shade::Half)
                ),
                ([9.0, top + 11.0, 3.0, 8.0], Paint::HomRef),
                (
                    [9.0, top + 31.0, 3.0, 8.0],
                    Paint::Allele(Allele::Base(Base::T), Shade::Full)
                ),
            ]
        );
        assert!((row_height(Mode::Expanded) - 10.0).abs() < f32::EPSILON);
        assert!((row_height(Mode::Squished) - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn narrow_sites_are_three_pixels_wide() {
        let sites = [
            variant(10, "A", "G", &[]),
            variant(20, "AC", "A", &[]),
            variant(30, "ACGT", "A", &[]),
            variant(40, "ACGTAC", "A", &[]),
        ];
        let found: Vec<[f32; 2]> = rects(&scene(view(0, 100), &sites, Mode::Collapsed, &plain(0)))
            .into_iter()
            .map(|(rect, _)| [rect[0], rect[2]])
            .collect();
        assert_eq!(found, [[9.0, 3.0], [19.5, 3.0], [30.0, 3.0], [40.0, 5.0]]);
    }

    #[test]
    fn dense_sites_share_pixel_columns() {
        let sites: Vec<Variant> = (0..1_000)
            .map(|index| variant(index, "A", "G", &[Call::Het]))
            .collect();
        let found = rects(&scene(view(0, 10_000), &sites, Mode::Expanded, &plain(1)));
        assert!(found.len() <= 2 * 11, "{}", found.len());
    }

    #[test]
    fn hits_find_the_site_and_the_sample() {
        let calls = [Call::Het, Call::Reference];
        let sites = [variant(10, "A", "T", &calls), variant(50, "A", "C", &calls)];
        let viewport = view(0, 100);
        let rows = plain(2);
        assert_eq!(
            hit(viewport, &sites, Mode::Expanded, &rows, 10.5, 5.0),
            Some((0, None))
        );
        let second_row = SITE_HEIGHT + 1.0 + 15.0;
        assert_eq!(
            hit(viewport, &sites, Mode::Expanded, &rows, 50.5, second_row),
            Some((1, Some(1)))
        );
        assert_eq!(
            hit(viewport, &sites, Mode::Expanded, &rows, 30.0, 5.0),
            None
        );
        assert_eq!(
            hit(viewport, &sites, Mode::Expanded, &rows, 50.5, 200.0),
            None
        );
    }

    #[test]
    fn grouped_rows_leave_a_band_for_the_name_of_each_group() {
        let calls = [Call::Het, Call::Reference, Call::Alternate];
        let sites = [variant(10, "A", "T", &calls)];
        let arrangement = Arrangement {
            order: vec![2, 0, 1],
            groups: vec![
                Group {
                    name: "case".to_owned(),
                    first: 0,
                    size: 1,
                },
                Group {
                    name: "control".to_owned(),
                    first: 1,
                    size: 2,
                },
            ],
            tints: vec![Tint::Value(1), Tint::Value(0), Tint::Value(0)],
        };
        let rows = Rows::new(&arrangement, 1);
        let drawn = scene(view(0, 100), &sites, Mode::Expanded, &rows);
        let top = SITE_HEIGHT + 1.0;
        let t = Allele::Base(Base::T);
        let marks: Vec<([f32; 4], Paint)> = rects(&drawn)
            .into_iter()
            .filter(|(rect, _)| rect[1] >= top && rect[0] > STRIP)
            .collect();
        assert_eq!(
            marks,
            [
                (
                    [9.0, top + GROUP_GAP + 1.0, 3.0, 8.0],
                    Paint::Allele(t, Shade::Full)
                ),
                (
                    [9.0, top + 2.0 * GROUP_GAP + 11.0, 3.0, 8.0],
                    Paint::Allele(t, Shade::Half)
                ),
                ([9.0, top + 2.0 * GROUP_GAP + 21.0, 3.0, 8.0], Paint::HomRef),
            ]
        );
        let strips: Vec<([f32; 4], Paint)> = rects(&drawn)
            .into_iter()
            .filter(|(rect, _)| rect[0] < 1.0 && rect[1] >= top)
            .collect();
        assert_eq!(
            strips[0],
            ([0.0, top + GROUP_GAP + 1.0, STRIP, 8.0], Paint::Category(0))
        );
        assert_eq!(strips[1].1, Paint::Category(1));
        let names = drawn
            .iter()
            .filter(|primitive| matches!(primitive, Primitive::Text { .. }))
            .count();
        assert_eq!(names, 2);
        let viewport = view(0, 100);
        let at = |y| hit(viewport, &sites, Mode::Expanded, &rows, 10.5, y);
        assert_eq!(at(top + 5.0), None);
        assert_eq!(at(top + GROUP_GAP + 5.0), Some((0, Some(0))));
        assert_eq!(at(top + 2.0 * GROUP_GAP + 25.0), Some((0, Some(2))));
    }

    #[test]
    fn big_cohorts_draw_one_row_per_group_of_samples() {
        let mut calls = vec![Call::Reference; 3_202];
        calls[25] = Call::Het;
        calls[3_201] = Call::Alternate;
        let sites = [variant(10, "A", "T", &calls), variant(50, "A", "C", &calls)];
        assert_eq!((group(3_202), plain(3_202).len()), (11, 292));
        assert_eq!((group(300), plain(300).len()), (1, 300));
        let rows = plain(3_202);
        let found = rects(&scene(view(0, 100), &sites, Mode::Squished, &rows));
        assert_eq!(found.len(), 2 * (1 + 292));
        let row = row_height(Mode::Squished);
        let paint_at = |y: f32| {
            found
                .iter()
                .find(|(rect, _)| rect[0] < 10.0 && (rect[1] - y).abs() < 0.01)
                .map(|(_, paint)| *paint)
        };
        let top = SITE_HEIGHT + 1.0;
        let t = Allele::Base(Base::T);
        assert_eq!(
            paint_at(top + 2.0 * row),
            Some(Paint::Allele(t, Shade::Half))
        );
        assert_eq!(paint_at(top + 3.0 * row), Some(Paint::HomRef));
        assert_eq!(
            paint_at(top + 291.0 * row),
            Some(Paint::Allele(t, Shade::Full))
        );
        let in_row = |row_index: f32| top + (row_index + 0.5) * row;
        let viewport = view(0, 100);
        assert_eq!(
            hit(viewport, &sites, Mode::Squished, &rows, 10.5, in_row(2.0)),
            Some((0, Some(2)))
        );
        assert_eq!(
            hit(viewport, &sites, Mode::Squished, &rows, 10.5, in_row(292.0)),
            None
        );
    }
}
