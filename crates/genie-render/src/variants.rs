use std::collections::HashSet;

use genie_core::interaction::Interaction;
use genie_core::review::Verdict;
use genie_core::trio::{self, Inheritance, Trio};
use genie_core::variant::{Call, Kind, Structure, Sv, Variant};
use genie_core::{Region, Viewport};

use crate::features::Mode;
use crate::links;
use crate::samples::Rows;
use crate::scene::{Allele, Base, Hue, Paint, Primitive, Shade};

pub const SITE_HEIGHT: f32 = 12.0;
pub const LIMIT: usize = 100;
pub const GROUP_LIMIT: usize = 300;
/// The height of the lane of breakend arcs above the sites.
pub const BREAKEND_HEIGHT: f32 = 40.0;

const REACH: f32 = 3.0;
const NARROWEST: f32 = 3.0;
/// The height of the ends of a whisker over a confidence interval.
const WHISKER: f32 = 5.0;

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
    if let Some(structure) = &variant.structure {
        return match structure.kind {
            Sv::Deletion => Allele::Deletion,
            Sv::Duplication => Allele::Duplication,
            Sv::Inversion => Allele::Inversion,
            Sv::Insertion => Allele::Insertion,
            Sv::CopyNumber | Sv::Breakend | Sv::Complex => Allele::Other,
        };
    }
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
        let (x, width) = cell(viewport, span);
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
            if let Some(structure) = &variant.structure {
                primitives.extend(whiskers(
                    viewport,
                    span,
                    structure,
                    Paint::Allele(allele, shade),
                ));
            }
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

/// The height of the strip of review marks above the sites.
pub const REVIEW_HEIGHT: f32 = 4.0;
/// The narrowest review mark, wider than a narrow site so that its color shows.
const REVIEW_WIDTH: f32 = 7.0;

/// The color of the review mark of a verdict: green for true, vermillion for false and grey for
/// not sure.
pub fn verdict_paint(verdict: Verdict) -> Paint {
    match verdict {
        Verdict::True => Paint::Hue(Hue::Green),
        Verdict::False => Paint::Hue(Hue::Vermillion),
        Verdict::Unsure => Paint::Muted,
    }
}

/// The marks of the reviewed variants in a strip above the sites, as wide as the marks of their
/// sites and at least `REVIEW_WIDTH`, with the verdict of each variant in the same order as the
/// variants.
pub fn review_marks(
    viewport: Viewport,
    variants: &[Variant],
    verdicts: &[Option<Verdict>],
) -> Vec<Primitive> {
    let visible = viewport.visible();
    variants
        .iter()
        .zip(verdicts)
        .filter_map(|(variant, verdict)| {
            let verdict = (*verdict)?;
            let span = variant.span;
            if span.start() > visible.end() || span.end() < visible.start() {
                return None;
            }
            let (x, width) = cell(viewport, span);
            let wide = width.max(REVIEW_WIDTH);
            Some(Primitive::Rect {
                x: x + (width - wide) / 2.0,
                y: 0.0,
                width: wide,
                height: REVIEW_HEIGHT - 1.0,
                paint: verdict_paint(verdict),
            })
        })
        .collect()
}

/// Moves the marks of the rows of samples down by `by` pixels, for a lane between the sites and
/// the rows. The marks of the sites stay.
pub fn rows_lowered(primitives: Vec<Primitive>, by: f32) -> Vec<Primitive> {
    primitives
        .into_iter()
        .map(|primitive| {
            if primitive.anchor() >= SITE_HEIGHT {
                primitive.lowered(by)
            } else {
                primitive
            }
        })
        .collect()
}

/// Frames around the calls of children that their parents cannot give them, as in a de novo
/// variant. They have the color of text, so that they show around every allele color.
pub fn trio_marks(
    viewport: Viewport,
    variants: &[Variant],
    mode: Mode,
    rows: &Rows,
    trios: &[Trio],
) -> Vec<Primitive> {
    if trios.is_empty() || mode == Mode::Collapsed {
        return Vec::new();
    }
    let (row, offset, mark) = shape(mode);
    let visible = viewport.visible();
    let mut primitives = Vec::new();
    for variant in variants {
        let span = variant.span;
        if span.start() > visible.end() || span.end() < visible.start() {
            continue;
        }
        let call = |sample: usize| variant.calls.get(sample).copied().unwrap_or(Call::Missing);
        let biallelic = variant.alternates.len() <= 1;
        let children: Vec<usize> = trios
            .iter()
            .filter(|trio| {
                let calls = (call(trio.child), call(trio.father), call(trio.mother));
                matches!(
                    trio::check(calls.0, calls.1, calls.2, biallelic),
                    Some(Inheritance::DeNovo | Inheritance::Error)
                )
            })
            .map(|trio| trio.child)
            .collect();
        if children.is_empty() {
            continue;
        }
        let (x, width) = cell(viewport, span);
        for (index, part) in rows.members.iter().enumerate() {
            if part.iter().any(|sample| children.contains(sample)) {
                let y = SITE_HEIGHT + 1.0 + rows.top(index, row) + offset;
                primitives.extend(outline(x, y, width, mark));
            }
        }
    }
    primitives
}

/// Four lines of one pixel around a box, outside it, so that a call keeps its own color.
fn outline(x: f32, y: f32, width: f32, height: f32) -> [Primitive; 4] {
    let line = |x, y, width, height| Primitive::Rect {
        x,
        y,
        width,
        height,
        paint: Paint::Text,
    };
    [
        line(x - 1.0, y - 1.0, width + 2.0, 1.0),
        line(x - 1.0, y + height, width + 2.0, 1.0),
        line(x - 1.0, y, 1.0, height),
        line(x + width, y, 1.0, height),
    ]
}

/// The left edge and the width of the marks of a site: at least `NARROWEST` pixels, around the
/// middle of a short site.
fn cell(viewport: Viewport, span: Region) -> (f32, f32) {
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    let left = px(span.start());
    let extent = px(span.end()) - left;
    let width = if extent > NARROWEST + 1.0 {
        extent - 1.0
    } else {
        NARROWEST
    };
    (left + (extent - width).min(0.0) / 2.0, width)
}

/// Lines over the confidence intervals of the start and the end of a structural variant.
fn whiskers(
    viewport: Viewport,
    span: Region,
    structure: &Structure,
    paint: Paint,
) -> Vec<Primitive> {
    let px = |position: f64| viewport.to_px(position) as f32;
    let middle = SITE_HEIGHT / 2.0;
    let mut found = Vec::new();
    for (at, around) in [span.start(), span.end()].into_iter().zip(structure.around) {
        let Some((low, high)) = around else {
            continue;
        };
        let (left, right) = (px(at as f64 + low as f64), px(at as f64 + high as f64));
        if right - left < 2.0 {
            continue;
        }
        found.push(Primitive::Rect {
            x: left,
            y: middle - 0.5,
            width: right - left,
            height: 1.0,
            paint,
        });
        for x in [left, right - 1.0] {
            found.push(Primitive::Rect {
                x,
                y: middle - WHISKER / 2.0,
                width: 1.0,
                height: WHISKER,
                paint,
            });
        }
    }
    found
}

fn joined(variant: &Variant) -> Option<&(String, u64)> {
    match &variant.structure {
        Some(Structure {
            kind: Sv::Breakend,
            mate: Some(mate),
            ..
        }) => Some(mate),
        _ => None,
    }
}

/// The height of the lane of breakend arcs: none when no variant is a breakend.
pub fn breakend_height(variants: &[Variant]) -> f32 {
    if variants.iter().any(|variant| joined(variant).is_some()) {
        BREAKEND_HEIGHT
    } else {
        0.0
    }
}

/// The breakends of a region as links: an arc to the other end on the same contig, or a stub
/// to another contig. Two breakends that join each other give one arc.
#[derive(Debug, Clone, PartialEq)]
pub struct Breakends {
    pub links: Vec<Interaction>,
    /// The variant of each link.
    pub variants: Vec<usize>,
}

impl Breakends {
    pub fn new(variants: &[Variant], contig: &str) -> Self {
        let mut links = Vec::new();
        let mut owners = Vec::new();
        let mut seen = HashSet::new();
        for (index, variant) in variants.iter().enumerate() {
            let Some((other, position)) = joined(variant) else {
                continue;
            };
            let here = Region::between(variant.span.start(), variant.span.start() + 1);
            let there = Region::between(*position, position + 1);
            let elsewhere = (other != contig).then(|| other.clone());
            let mut link = Interaction::new(here, there, elsewhere);
            if link.elsewhere.is_none() && !seen.insert((link.near.start(), link.far.start())) {
                continue;
            }
            link.name.clone_from(&variant.id);
            links.push(link);
            owners.push(index);
        }
        Self {
            links,
            variants: owners,
        }
    }

    pub fn scene(&self, viewport: Viewport, hue: Hue) -> Vec<Primitive> {
        links::scene(viewport, &self.links, (hue, false), BREAKEND_HEIGHT)
    }

    /// The variant of the arc or the stub under a point of the lane.
    pub fn hit(&self, viewport: Viewport, x: f32, y: f32) -> Option<usize> {
        links::hit(viewport, &self.links, BREAKEND_HEIGHT, x, y).map(|index| self.variants[index])
    }
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
    use genie_core::trio::{Sex, Trio};
    use genie_core::variant::{Annotations, Call, Structure, Sv, Variant};
    use genie_core::{Region, Viewport};

    use genie_core::review::Verdict;

    use super::{
        BREAKEND_HEIGHT, Breakends, REVIEW_HEIGHT, SITE_HEIGHT, breakend_height, group, hit,
        review_marks, row_height, rows_lowered, scene, trio_marks, verdict_paint,
    };
    use crate::features::Mode;
    use crate::samples::{GROUP_GAP, Rows, STRIP};
    use crate::scene::{Allele, Base, Hue, Paint, Primitive, Shade};

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

    fn sv(
        span: (u64, u64),
        kind: Sv,
        around: [Option<(i64, i64)>; 2],
        mate: Option<(&str, u64)>,
    ) -> Variant {
        let mut found = variant(span.0, "N", "<SV>", &[]);
        found.span = Region::between(span.0, span.1);
        found.structure = Some(Structure {
            kind,
            length: None,
            mate: mate.map(|(contig, position)| (contig.to_owned(), position)),
            around,
            precise: true,
        });
        found
    }

    #[test]
    fn structural_variants_have_their_colors_and_whiskers() {
        let sites = [
            sv((10, 20), Sv::Duplication, [None, None], None),
            sv((30, 40), Sv::Inversion, [None, None], None),
            sv((50, 70), Sv::Deletion, [Some((-4, 4)), Some((0, 0))], None),
            sv((80, 81), Sv::Breakend, [None, None], Some(("chr2", 500))),
        ];
        let found = rects(&scene(view(0, 100), &sites, Mode::Collapsed, &plain(0)));
        let deletion = Paint::Allele(Allele::Deletion, Shade::Full);
        assert_eq!(
            found,
            [
                (
                    [10.0, 1.0, 9.0, 10.0],
                    Paint::Allele(Allele::Duplication, Shade::Full)
                ),
                (
                    [30.0, 1.0, 9.0, 10.0],
                    Paint::Allele(Allele::Inversion, Shade::Full)
                ),
                ([50.0, 1.0, 19.0, 10.0], deletion),
                ([46.0, 5.5, 8.0, 1.0], deletion),
                ([46.0, 3.5, 1.0, 5.0], deletion),
                ([53.0, 3.5, 1.0, 5.0], deletion),
                (
                    [79.0, 1.0, 3.0, 10.0],
                    Paint::Allele(Allele::Other, Shade::Full)
                ),
            ]
        );
    }

    #[test]
    fn breakends_link_their_two_ends_once() {
        let sites = [
            sv((100, 101), Sv::Breakend, [None, None], Some(("chr1", 900))),
            sv((900, 901), Sv::Breakend, [None, None], Some(("chr1", 100))),
            sv((300, 301), Sv::Breakend, [None, None], Some(("chr7", 12))),
            sv((600, 700), Sv::Deletion, [None, None], None),
        ];
        assert!((breakend_height(&sites) - BREAKEND_HEIGHT).abs() < f32::EPSILON);
        assert!(breakend_height(&sites[3..]).abs() < f32::EPSILON);
        let breakends = Breakends::new(&sites, "chr1");
        assert_eq!(breakends.variants, [0, 2]);
        let ends: Vec<(Region, Region)> = breakends
            .links
            .iter()
            .map(|link| (link.near, link.far))
            .collect();
        assert_eq!(
            ends,
            [
                (Region::between(100, 101), Region::between(900, 901)),
                (Region::between(300, 301), Region::between(12, 13)),
            ]
        );
        assert_eq!(breakends.links[1].elsewhere.as_deref(), Some("chr7"));
        let viewport = view(0, 1_000);
        let arcs = breakends.scene(viewport, Hue::Blue);
        assert!(
            arcs.iter()
                .any(|primitive| matches!(primitive, Primitive::Arc { .. }))
        );
        assert_eq!(breakends.hit(viewport, 50.05, 3.0), Some(0));
        assert_eq!(breakends.hit(viewport, 30.0, 30.0), Some(2));
        assert_eq!(breakends.hit(viewport, 70.0, 30.0), None);
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
    fn reviewed_sites_have_a_mark_in_the_color_of_their_verdict() {
        let sites = [
            variant(10, "A", "T", &[]),
            variant(20, "C", "G", &[]),
            variant(30, "G", "A", &[]),
            variant(500, "G", "A", &[]),
        ];
        let verdicts = [
            Some(Verdict::True),
            None,
            Some(Verdict::Unsure),
            Some(Verdict::False),
        ];
        let found = rects(&review_marks(view(0, 100), &sites, &verdicts));
        let height = REVIEW_HEIGHT - 1.0;
        assert_eq!(
            found,
            [
                ([7.0, 0.0, 7.0, height], Paint::Hue(Hue::Green)),
                ([27.0, 0.0, 7.0, height], Paint::Muted),
            ]
        );
        let wide = [variant(0, &"A".repeat(20), "T", &[])];
        let found = rects(&review_marks(view(0, 100), &wide, &[Some(Verdict::False)]));
        assert_eq!(found[0].0, [0.0, 0.0, 19.0, height]);
        assert_eq!(verdict_paint(Verdict::False), Paint::Hue(Hue::Vermillion));
    }

    /// A lane between the sites and the rows moves the rows down and keeps the sites.
    #[test]
    fn rows_move_under_a_lane_and_sites_stay() {
        let sites = [variant(10, "A", "T", &[Call::Het, Call::Alternate])];
        let primitives = scene(view(0, 100), &sites, Mode::Expanded, &plain(2));
        let before: Vec<f32> = primitives.iter().map(Primitive::anchor).collect();
        let after: Vec<f32> = rows_lowered(primitives, 40.0)
            .iter()
            .map(Primitive::anchor)
            .collect();
        assert!(before.iter().any(|top| *top < SITE_HEIGHT));
        assert!(before.iter().any(|top| *top >= SITE_HEIGHT));
        for (old, new) in before.iter().zip(&after) {
            let moved = if *old >= SITE_HEIGHT { 40.0 } else { 0.0 };
            assert!((new - old - moved).abs() < 1e-4, "{old} {new}");
        }
    }

    #[test]
    fn children_with_calls_that_their_parents_cannot_give_get_a_frame() {
        let trio = Trio {
            child: 0,
            father: 1,
            mother: 2,
            sex: Sex::Unknown,
        };
        let sites = [
            variant(10, "A", "T", &[Call::Het, Call::Reference, Call::Reference]),
            variant(30, "A", "T", &[Call::Het, Call::Het, Call::Reference]),
            variant(50, "A", "T", &[Call::Reference, Call::Alternate, Call::Het]),
            variant(70, "A", "T", &[Call::Het, Call::Missing, Call::Reference]),
        ];
        let rows = plain(3);
        let marks = trio_marks(view(0, 100), &sites, Mode::Expanded, &rows, &[trio]);
        let top = SITE_HEIGHT + 1.0;
        let frame = |x: f32| {
            [
                ([x - 1.0, top, 5.0, 1.0], Paint::Text),
                ([x - 1.0, top + 9.0, 5.0, 1.0], Paint::Text),
                ([x - 1.0, top + 1.0, 1.0, 8.0], Paint::Text),
                ([x + 3.0, top + 1.0, 1.0, 8.0], Paint::Text),
            ]
        };
        let expected: Vec<([f32; 4], Paint)> = frame(9.0).into_iter().chain(frame(49.0)).collect();
        assert_eq!(rects(&marks), expected);
        assert_eq!(
            trio_marks(view(0, 100), &sites, Mode::Expanded, &rows, &[]),
            []
        );
        assert_eq!(
            trio_marks(view(0, 100), &sites, Mode::Collapsed, &rows, &[trio]),
            []
        );
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
