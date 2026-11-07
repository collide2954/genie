use genie_core::Viewport;
use genie_core::variant::{Call, Kind, Variant};

use crate::features::Mode;
use crate::scene::{Allele, Base, Paint, Primitive, Shade};

pub const SITE_HEIGHT: f32 = 12.0;
pub const LIMIT: usize = 100;

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

pub fn scene(viewport: Viewport, variants: &[Variant], mode: Mode) -> Vec<Primitive> {
    let (row, offset, size) = shape(mode);
    let visible = viewport.visible();
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    let samples = if mode == Mode::Collapsed {
        0
    } else {
        variants
            .iter()
            .map(|variant| variant.calls.len())
            .max()
            .unwrap_or(0)
    };
    let mut drawn: Vec<(i64, u8)> = vec![(i64::MIN, 0); samples + 1];
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
        for (sample, call) in variant.calls.iter().take(samples).enumerate() {
            let paint = match call {
                Call::Missing => continue,
                Call::Reference => Paint::HomRef,
                Call::Het => Paint::Allele(allele, Shade::Half),
                Call::Alternate => Paint::Allele(allele, Shade::Full),
            };
            if place(sample + 1, strength(*call)) {
                primitives.push(Primitive::Rect {
                    x,
                    y: SITE_HEIGHT + 1.0 + sample as f32 * row + offset,
                    width,
                    height: size,
                    paint,
                });
            }
        }
    }
    primitives
}

pub fn hit(
    viewport: Viewport,
    variants: &[Variant],
    mode: Mode,
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
    let sample = ((y - SITE_HEIGHT - 1.0) / row) as usize;
    (sample < variants[index].calls.len()).then_some((index, Some(sample)))
}

#[cfg(test)]
mod tests {
    use genie_core::variant::{Call, Variant};
    use genie_core::{Region, Viewport};

    use super::{SITE_HEIGHT, hit, row_height, scene};
    use crate::features::Mode;
    use crate::scene::{Allele, Base, Paint, Primitive, Shade};

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
        let found = rects(&scene(view(0, 100), &sites, Mode::Collapsed));
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
        let found = rects(&scene(view(0, 100), &sites, Mode::Expanded));
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
        let found: Vec<[f32; 2]> = rects(&scene(view(0, 100), &sites, Mode::Collapsed))
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
        let found = rects(&scene(view(0, 10_000), &sites, Mode::Expanded));
        assert!(found.len() <= 2 * 11, "{}", found.len());
    }

    #[test]
    fn hits_find_the_site_and_the_sample() {
        let calls = [Call::Het, Call::Reference];
        let sites = [variant(10, "A", "T", &calls), variant(50, "A", "C", &calls)];
        let viewport = view(0, 100);
        assert_eq!(
            hit(viewport, &sites, Mode::Expanded, 10.5, 5.0),
            Some((0, None))
        );
        let second_row = SITE_HEIGHT + 1.0 + 15.0;
        assert_eq!(
            hit(viewport, &sites, Mode::Expanded, 50.5, second_row),
            Some((1, Some(1)))
        );
        assert_eq!(hit(viewport, &sites, Mode::Expanded, 30.0, 5.0), None);
        assert_eq!(hit(viewport, &sites, Mode::Expanded, 50.5, 200.0), None);
    }
}
