use std::borrow::Cow;

use genie_core::alignment::{Alignment, Gap, Op, Pairing};
use genie_core::methylation::Site;
use genie_core::pileup::{Coverage, Junction, Read};
use genie_core::{Region, Strand, Viewport};

use crate::features::Mode;
use crate::scene::{Align, Base, Font, Hue, LABEL_INSET, Paint, Primitive, Shade};

pub const COVERAGE_HEIGHT: f32 = 36.0;
pub const JUNCTION_HEIGHT: f32 = 32.0;
const COUNT_SIZE: f32 = 9.0;
pub const LIMIT: usize = 100;
pub const GROUP_GAP: f32 = 14.0;

const GROUP_SIZE: f32 = 10.0;

const VARIANT: f32 = 0.2;
const FADED_BELOW: u8 = 20;
const GAPS_FROM: f64 = 6.0;
const LETTERS_FROM: f64 = 8.0;
const LETTER_SIZE: f32 = 8.0;
const RANGE_SIZE: f32 = 10.0;
const REACH: f32 = 2.0;
const BASES: [Base; 5] = [Base::A, Base::C, Base::G, Base::T, Base::N];

pub fn fit(rows: usize) -> Mode {
    if rows <= LIMIT {
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
        Mode::Squished => (3.0, 0.0, 2.0),
        Mode::Expanded | Mode::Collapsed => (10.0, 1.0, 8.0),
    }
}

pub fn coverage(
    viewport: Viewport,
    coverage: &Coverage,
    reference: impl Fn(u64) -> Option<u8>,
    height: f32,
) -> Vec<Primitive> {
    let Some(shown) = viewport.visible().intersection(coverage.region()) else {
        return Vec::new();
    };
    let positions = shown.start()..shown.end();
    let max = positions
        .clone()
        .map(|at| coverage.depth(at))
        .max()
        .unwrap_or(0);
    if max == 0 {
        return Vec::new();
    }
    let usable = height - 1.0;
    let scale = |count: u32| count as f32 / max as f32 * usable;
    let pixels = 1.0 / viewport.scale();
    let width = cell(pixels);
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    let mut primitives = Vec::new();
    let mut bar = |x: f32, width: f32, depth: u32, counts: Option<[u32; 5]>| {
        let Some(counts) = counts else {
            let size = scale(depth);
            primitives.push(Primitive::Rect {
                x,
                y: height - size,
                width,
                height: size,
                paint: Paint::Coverage,
            });
            return;
        };
        let mut bottom = height;
        for (count, base) in counts.into_iter().zip(BASES) {
            if count > 0 {
                let size = scale(count);
                bottom -= size;
                primitives.push(Primitive::Rect {
                    x,
                    y: bottom,
                    width,
                    height: size,
                    paint: Paint::Base(base),
                });
            }
        }
    };
    let variant = |at: u64| -> Option<[u32; 5]> {
        let counts = coverage.counts(at)?;
        let depth: u32 = counts.iter().sum();
        let own = counts[bucket(reference(at)?)?];
        (depth > 0 && (depth - own) as f32 / depth as f32 >= VARIANT).then_some(counts)
    };
    if pixels >= 1.0 {
        for at in positions {
            let depth = coverage.depth(at);
            if depth > 0 {
                bar(px(at), width, depth, variant(at));
            }
        }
    } else {
        let first = px(shown.start()).floor().max(0.0) as u64;
        let last = px(shown.end()).ceil().min(viewport.width() as f32) as u64;
        for column in first..last {
            let from = viewport.to_bp(column as f64).floor().max(0.0) as u64;
            let to = viewport.to_bp((column + 1) as f64).ceil() as u64;
            let depth = (from.max(shown.start())..to.min(shown.end()))
                .map(|at| coverage.depth(at))
                .max()
                .unwrap_or(0);
            if depth > 0 {
                bar(column as f32, 1.0, depth, None);
            }
        }
        for at in positions {
            if let Some(counts) = variant(at) {
                bar(px(at).floor(), 1.0, coverage.depth(at), Some(counts));
            }
        }
    }
    primitives.push(Primitive::Label {
        x: viewport.width() as f32 - LABEL_INSET,
        y: RANGE_SIZE / 2.0 + 1.0,
        content: Cow::Owned(format!("0-{max}")),
        size: RANGE_SIZE,
    });
    primitives
}

fn pair_hue(pairing: Pairing) -> Option<Hue> {
    match pairing {
        Pairing::Normal => None,
        Pairing::Large => Some(Hue::Vermillion),
        Pairing::Small => Some(Hue::Blue),
        Pairing::SameStrand => Some(Hue::Green),
        Pairing::Outward => Some(Hue::Purple),
        Pairing::Elsewhere => Some(Hue::Orange),
    }
}

pub const METHYLATION_HEIGHT: f32 = 20.0;
const PERCENT_RANGE: &str = "0-100 %";

pub type MethylationLane = (Option<String>, Vec<Site>);

pub fn methylation_height(lanes: usize) -> f32 {
    lanes as f32 * (METHYLATION_HEIGHT + 1.0)
}

pub fn methylation(viewport: Viewport, lanes: &[MethylationLane]) -> Vec<Primitive> {
    let pixels = 1.0 / viewport.scale();
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    let mut primitives = Vec::new();
    for (index, (label, sites)) in lanes.iter().enumerate() {
        let top = index as f32 * (METHYLATION_HEIGHT + 1.0);
        let shown = sites.iter().filter(|site| shows(viewport, site.position));
        if pixels >= 1.0 {
            let width = cell(pixels);
            for site in shown {
                share_bar(&mut primitives, top, px(site.position), width, site.share());
            }
        } else {
            let mut column: Option<(i64, f32, f32)> = None;
            for site in shown {
                let here = px(site.position).floor() as i64;
                match &mut column {
                    Some((at, sum, count)) if *at == here => {
                        *sum += site.share();
                        *count += 1.0;
                    }
                    _ => {
                        if let Some((at, sum, count)) = column {
                            share_bar(&mut primitives, top, at as f32, 1.0, sum / count);
                        }
                        column = Some((here, site.share(), 1.0));
                    }
                }
            }
            if let Some((at, sum, count)) = column {
                share_bar(&mut primitives, top, at as f32, 1.0, sum / count);
            }
        }
        if let Some(label) = label {
            primitives.push(Primitive::Text {
                x: 4.0,
                y: top + GROUP_SIZE / 2.0 + 1.0,
                content: Cow::Owned(label.clone()),
                size: GROUP_SIZE,
                paint: Paint::Muted,
                font: Font::Ui,
                align: Align::Start,
            });
        }
    }
    if !lanes.is_empty() {
        primitives.push(Primitive::Label {
            x: viewport.width() as f32 - LABEL_INSET,
            y: RANGE_SIZE / 2.0 + 1.0,
            content: Cow::Borrowed(PERCENT_RANGE),
            size: RANGE_SIZE,
        });
    }
    primitives
}

fn shows(viewport: Viewport, position: u64) -> bool {
    let visible = viewport.visible();
    position >= visible.start() && position < visible.end()
}

fn share_bar(primitives: &mut Vec<Primitive>, top: f32, x: f32, width: f32, share: f32) {
    let usable = METHYLATION_HEIGHT - 1.0;
    let modified = share * usable;
    let rest = usable - modified;
    if rest > 0.0 {
        primitives.push(Primitive::Rect {
            x,
            y: top + 1.0,
            width,
            height: rest,
            paint: Paint::Plain(Shade::Half),
        });
    }
    if modified > 0.0 {
        primitives.push(Primitive::Rect {
            x,
            y: top + 1.0 + rest,
            width,
            height: modified,
            paint: Paint::Modified(u8::MAX),
        });
    }
}

pub fn methylation_hit(
    viewport: Viewport,
    lanes: &[MethylationLane],
    x: f32,
    y: f32,
) -> Option<(usize, Site)> {
    if y < 0.0 {
        return None;
    }
    let index = (y / (METHYLATION_HEIGHT + 1.0)) as usize;
    let (_, sites) = lanes.get(index)?;
    let found = if 1.0 / viewport.scale() >= 1.0 {
        let at = viewport.to_bp(f64::from(x)).floor().max(0.0) as u64;
        sites.iter().find(|site| site.position == at)
    } else {
        let column = x.floor() as i64;
        sites
            .iter()
            .find(|site| viewport.to_px(site.position as f64).floor() as i64 == column)
    };
    found.map(|site| (index, *site))
}

pub fn height(count: usize, groups: &[(usize, String)], mode: Mode) -> f32 {
    count as f32 * row_height(mode) + groups.len() as f32 * GROUP_GAP
}

fn bands(row: usize, groups: &[(usize, String)]) -> f32 {
    groups.partition_point(|(first, _)| *first <= row) as f32 * GROUP_GAP
}

fn row_at(y: f32, row_height: f32, groups: &[(usize, String)]) -> Option<usize> {
    if y < 0.0 {
        return None;
    }
    if groups.is_empty() {
        return Some((y / row_height) as usize);
    }
    let top = |index: usize| groups[index].0 as f32 * row_height + (index + 1) as f32 * GROUP_GAP;
    let index = (0..groups.len()).rev().find(|&index| y >= top(index))?;
    let row = groups[index].0 + ((y - top(index)) / row_height) as usize;
    let end = groups.get(index + 1).map_or(usize::MAX, |(next, _)| *next);
    (row < end).then_some(row)
}

const ARC_BASE: f32 = JUNCTION_HEIGHT - 2.0;
const LOW_RISE: f32 = 6.0;
const HIGH_RISE: f32 = JUNCTION_HEIGHT - 6.0;
const COUNT_GAP: f32 = 6.0;

fn arc_shape(viewport: Viewport, junction: &Junction, most: u32) -> (f32, f32, f32) {
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    let (from, to) = (px(junction.span.start()), px(junction.span.end()));
    let share = junction.count as f32 / most.max(1) as f32;
    let rise = LOW_RISE + (HIGH_RISE - LOW_RISE) * share;
    (from, to, rise.min(((to - from) / 2.0).max(4.0)))
}

fn arc_width(count: u32) -> f32 {
    (1.0 + (count as f32).log2() / 2.0).clamp(1.0, 3.0)
}

fn most(junctions: &[Junction]) -> u32 {
    junctions
        .iter()
        .map(|junction| junction.count)
        .max()
        .unwrap_or(1)
}

pub fn arcs(viewport: Viewport, junctions: &[Junction]) -> Vec<Primitive> {
    let visible = viewport.visible();
    let most = most(junctions);
    let mut shown: Vec<&Junction> = junctions
        .iter()
        .filter(|junction| {
            junction.span.end() >= visible.start() && junction.span.start() <= visible.end()
        })
        .collect();
    shown.sort_by_key(|junction| junction.count);
    let mut primitives: Vec<Primitive> = shown
        .iter()
        .map(|junction| {
            let (from, to, rise) = arc_shape(viewport, junction, most);
            let paint = match junction.strand {
                Strand::Forward => Paint::Hue(Hue::Blue),
                Strand::Reverse => Paint::Hue(Hue::Vermillion),
                Strand::Unknown => Paint::Muted,
            };
            Primitive::Arc {
                from,
                to,
                base: ARC_BASE,
                rise,
                width: arc_width(junction.count),
                paint,
            }
        })
        .collect();
    let mut placed: Vec<(f32, f32, f32)> = Vec::new();
    for junction in shown.into_iter().rev() {
        let (from, to, rise) = arc_shape(viewport, junction, most);
        let words = junction.count.to_string();
        let half = words.len() as f32 * COUNT_SIZE * 0.3;
        let peak = ARC_BASE - rise;
        let y = if peak - COUNT_GAP - COUNT_SIZE / 2.0 >= 0.0 {
            peak - COUNT_GAP
        } else {
            peak + COUNT_GAP
        };
        let x = f32::midpoint(from, to);
        let crowded = placed.iter().any(|&(left, top, other)| {
            (left - x).abs() < half + other + 2.0 && (top - y).abs() < COUNT_SIZE
        });
        if crowded || to - from < 2.0 * half + 4.0 {
            continue;
        }
        placed.push((x, y, half));
        primitives.push(Primitive::Text {
            x,
            y,
            content: Cow::Owned(words),
            size: COUNT_SIZE,
            paint: Paint::Text,
            font: Font::Ui,
            align: Align::Center,
        });
    }
    primitives
}

pub fn arc_hit(viewport: Viewport, junctions: &[Junction], x: f32, y: f32) -> Option<usize> {
    let most = most(junctions);
    junctions
        .iter()
        .enumerate()
        .filter_map(|(index, junction)| {
            let (from, to, rise) = arc_shape(viewport, junction, most);
            if to <= from || x < from || x > to {
                return None;
            }
            let t = (x - from) / (to - from);
            let curve = ARC_BASE - 4.0 * rise * t * (1.0 - t);
            let distance = (curve - y).abs();
            (distance <= REACH.max(arc_width(junction.count))).then_some((index, distance))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(index, _)| index)
}

fn clipped_end(alignment: &Alignment) -> Option<u64> {
    if alignment.tags.parts.is_empty() {
        return None;
    }
    let clip = |ops: &mut dyn Iterator<Item = &(Op, u32)>| -> u32 {
        ops.take_while(|(op, _)| matches!(op, Op::SoftClip | Op::HardClip))
            .map(|(_, length)| *length)
            .sum()
    };
    let left = clip(&mut alignment.cigar.iter());
    let right = clip(&mut alignment.cigar.iter().rev());
    match (left, right) {
        (0, 0) => None,
        (left, right) if right >= left => Some(alignment.end()),
        _ => Some(alignment.start),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Look {
    pub pairs: bool,
    pub mods: bool,
}

fn tint(read: &Read, look: Look) -> Paint {
    let strand = read.alignment.strand();
    let faint = read.alignment.mapq == 0;
    if look.mods {
        return Paint::Plain(if faint { Shade::Faded } else { Shade::Full });
    }
    if faint {
        return Paint::Faint(strand);
    }
    pair_hue(read.pairing)
        .filter(|_| look.pairs)
        .map_or(Paint::Strand(strand), Paint::Pair)
}

fn modified(viewport: Viewport, read: &Read, (top, size): (f32, f32)) -> Vec<Primitive> {
    let visible = viewport.visible();
    let pixels = 1.0 / viewport.scale();
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    let shown = read
        .parts
        .modified
        .iter()
        .filter(|(position, _)| *position >= visible.start() && *position < visible.end());
    let mark = |x: f32, width: f32, chance: u8| Primitive::Rect {
        x,
        y: top,
        width,
        height: size,
        paint: Paint::Modified(chance),
    };
    if pixels >= 1.0 {
        let width = cell(pixels).max(1.0);
        return shown
            .map(|&(position, chance)| mark(px(position), width, chance))
            .collect();
    }
    let mut columns: Vec<(i64, u32, u32)> = Vec::new();
    for &(position, chance) in shown {
        let column = px(position).floor() as i64;
        match columns.last_mut() {
            Some((at, sum, count)) if *at == column => {
                *sum += u32::from(chance);
                *count += 1;
            }
            _ => columns.push((column, u32::from(chance), 1)),
        }
    }
    columns
        .into_iter()
        .map(|(column, sum, count)| {
            let chance = u8::try_from((sum + count / 2) / count).unwrap_or(u8::MAX);
            mark(column as f32, 1.0, chance)
        })
        .collect()
}

pub fn reads(
    viewport: Viewport,
    reads: &[Read],
    rows: &[usize],
    groups: &[(usize, String)],
    mode: Mode,
    look: Look,
) -> Vec<Primitive> {
    let (row_height, offset, size) = shape(mode);
    let visible = viewport.visible();
    let width = viewport.width() as f32;
    let pixels = 1.0 / viewport.scale();
    let cell = cell(pixels);
    let letters = pixels >= LETTERS_FROM && mode == Mode::Expanded;
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    let bar = |region: Region, y: f32, height: f32, paint: Paint| {
        let left = px(region.start()).max(-1.0);
        let right = px(region.end()).min(width + 1.0);
        Primitive::Rect {
            x: left,
            y,
            width: (right - left).max(1.0),
            height,
            paint,
        }
    };
    let mut primitives = Vec::new();
    let mut marks = Vec::new();
    for (read, &row) in reads.iter().zip(rows) {
        let span = read.extent;
        if span.start() > visible.end() || span.end() < visible.start() {
            continue;
        }
        let top = row as f32 * row_height + offset + bands(row, groups);
        let tint = tint(read, look);
        for block in &read.parts.blocks {
            primitives.push(bar(*block, top, size, tint));
        }
        marks.extend(clipped_end(&read.alignment).map(|edge| Primitive::Rect {
            x: px(edge) - 1.0,
            y: top,
            width: 2.0,
            height: size,
            paint: Paint::Text,
        }));
        for (gap, kind) in &read.parts.gaps {
            let paint = match kind {
                Gap::Deletion => Paint::Text,
                Gap::Skip => Paint::Muted,
            };
            primitives.push(bar(*gap, top + size / 2.0 - 0.5, 1.0, paint));
        }
        if look.mods {
            primitives.extend(modified(viewport, read, (top, size)));
        }
        for mismatch in read.parts.mismatches.iter().chain(&read.parts.clips) {
            let base = Base::from_byte(mismatch.base);
            let paint = if mismatch.quality < FADED_BELOW {
                Paint::Faded(base)
            } else {
                Paint::Base(base)
            };
            let x = px(mismatch.position);
            primitives.push(Primitive::Rect {
                x,
                y: top,
                width: cell.max(1.0),
                height: size,
                paint,
            });
            if letters {
                marks.push(Primitive::Text {
                    x: x + cell / 2.0,
                    y: top + size / 2.0,
                    content: Cow::Owned(char::from(mismatch.base).to_string()),
                    size: LETTER_SIZE,
                    paint: Paint::OnBase(base),
                    font: Font::MonoStrong,
                    align: Align::Center,
                });
            }
        }
        if pixels >= 1.0 {
            for (position, _) in &read.parts.insertions {
                primitives.push(Primitive::Rect {
                    x: px(*position) - 1.0,
                    y: top,
                    width: 2.0,
                    height: size,
                    paint: Paint::Insertion,
                });
            }
        }
    }
    for (index, (first, label)) in groups.iter().enumerate() {
        marks.push(Primitive::Text {
            x: 4.0,
            y: *first as f32 * row_height + index as f32 * GROUP_GAP + GROUP_GAP / 2.0,
            content: Cow::Owned(label.clone()),
            size: GROUP_SIZE,
            paint: Paint::Muted,
            font: Font::Ui,
            align: Align::Start,
        });
    }
    primitives.extend(marks);
    primitives
}

pub fn hit(
    viewport: Viewport,
    reads: &[Read],
    rows: &[usize],
    groups: &[(usize, String)],
    mode: Mode,
    x: f32,
    y: f32,
) -> Option<usize> {
    let row = row_at(y, row_height(mode), groups)?;
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    reads
        .iter()
        .zip(rows)
        .enumerate()
        .filter(|(_, (_, own))| **own == row)
        .map(|(index, (read, _))| {
            let span = read.extent;
            let distance = (px(span.start()) - x).max(x - px(span.end())).max(0.0);
            (index, distance)
        })
        .filter(|(_, distance)| *distance <= REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

fn cell(pixels: f64) -> f32 {
    if pixels >= GAPS_FROM {
        (pixels - 1.0) as f32
    } else {
        pixels as f32
    }
}

fn bucket(base: u8) -> Option<usize> {
    match base.to_ascii_uppercase() {
        b'A' => Some(0),
        b'C' => Some(1),
        b'G' => Some(2),
        b'T' => Some(3),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use genie_core::alignment::{Alignment, Op, Pairing, Part, REVERSE};
    use genie_core::pileup::{Coverage, Junction, Read};
    use genie_core::{Region, Strand, Viewport};

    use genie_core::methylation::Site;

    use super::{
        LIMIT, Look, arc_hit, arcs, coverage, fit, hit, methylation, methylation_height,
        methylation_hit, reads,
    };
    use crate::features::Mode;
    use crate::scene::{Base, Hue, Paint, Primitive, Shade};

    fn site(position: u64, modified: u32, total: u32) -> Site {
        Site {
            position,
            modified,
            total,
        }
    }

    fn lanes() -> Vec<(Option<String>, Vec<Site>)> {
        vec![
            (
                Some("Haplotype 1".to_owned()),
                vec![site(10, 3, 4), site(20, 0, 2)],
            ),
            (Some("Haplotype 2".to_owned()), vec![site(10, 1, 1)]),
        ]
    }

    #[test]
    fn methylation_lanes_show_the_modified_share_of_each_site() {
        let primitives = methylation(view(0, 100), &lanes());
        assert_eq!(
            rects(&primitives),
            [
                ([10.0, 1.0, 1.0, 4.75], Paint::Plain(Shade::Half)),
                ([10.0, 5.75, 1.0, 14.25], Paint::Modified(255)),
                ([20.0, 1.0, 1.0, 19.0], Paint::Plain(Shade::Half)),
                ([10.0, 22.0, 1.0, 19.0], Paint::Modified(255)),
            ]
        );
        assert_eq!(
            texts(&primitives),
            [
                ("Haplotype 1".to_owned(), 4.0),
                ("Haplotype 2".to_owned(), 4.0)
            ]
        );
        assert_eq!(labels(&primitives), [("0-100 %".to_owned(), 88.0)]);
        assert!((methylation_height(2) - 42.0).abs() < f32::EPSILON);
    }

    #[test]
    fn methylation_lanes_average_the_sites_of_each_pixel() {
        let lanes = vec![(None, vec![site(10, 3, 4), site(15, 1, 4)])];
        let primitives = methylation(view(0, 1_000), &lanes);
        assert_eq!(
            rects(&primitives),
            [
                ([1.0, 1.0, 1.0, 9.5], Paint::Plain(Shade::Half)),
                ([1.0, 10.5, 1.0, 9.5], Paint::Modified(255)),
            ]
        );
        assert_eq!(texts(&primitives), []);
    }

    #[test]
    fn methylation_sites_are_found_under_the_pointer() {
        let lanes = lanes();
        let at = |x, y| methylation_hit(view(0, 100), &lanes, x, y);
        assert_eq!(at(10.5, 5.0), Some((0, site(10, 3, 4))));
        assert_eq!(at(10.5, 25.0), Some((1, site(10, 1, 1))));
        assert_eq!(at(50.0, 5.0), None);
        assert_eq!(at(10.5, 50.0), None);
    }

    const PAIRS: Look = Look {
        pairs: true,
        mods: false,
    };

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    fn view(start: u64, end: u64) -> Viewport {
        Viewport::new(1_000_000, 100.0).show(region(start, end))
    }

    fn alignment(start: u64, flags: u16, cigar: Vec<(Op, u32)>, bases: &[u8]) -> Alignment {
        Alignment {
            name: format!("r{start}"),
            start,
            flags,
            mapq: 60,
            cigar,
            bases: bases.to_vec(),
            qualities: vec![30; bases.len()],
            mate: None,
            tags: genie_core::alignment::Tags::default(),
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

    fn texts(primitives: &[Primitive]) -> Vec<(String, f32)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text { content, x, .. } => Some((content.to_string(), *x)),
                _ => None,
            })
            .collect()
    }

    fn labels(primitives: &[Primitive]) -> Vec<(String, f32)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Label { content, x, .. } => Some((content.to_string(), *x)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn coverage_bars_scale_to_the_deepest_base() {
        let mut pile = Coverage::new(region(0, 100));
        for start in [10, 10, 30, 30, 30, 30] {
            pile.add(&alignment(start, 0, vec![(Op::Match, 10)], &[b'A'; 10]));
        }
        let primitives = coverage(view(0, 100), &pile, |_| Some(b'A'), 41.0);
        let bars = rects(&primitives);
        assert_eq!(bars.len(), 20);
        assert_eq!(bars[0], ([10.0, 21.0, 1.0, 20.0], Paint::Coverage));
        assert_eq!(bars[10], ([30.0, 1.0, 1.0, 40.0], Paint::Coverage));
        assert_eq!(labels(&primitives), [("0-4".to_owned(), 88.0)]);
        assert_eq!(texts(&primitives), []);
    }

    #[test]
    fn variant_positions_split_into_base_colors() {
        let mut pile = Coverage::new(region(0, 100));
        for base in *b"AAAAAAGGGG" {
            pile.add(&alignment(50, 0, vec![(Op::Match, 1)], &[base]));
        }
        for base in *b"AAAAAAAAAG" {
            pile.add(&alignment(60, 0, vec![(Op::Match, 1)], &[base]));
        }
        let bars = rects(&coverage(view(0, 100), &pile, |_| Some(b'A'), 41.0));
        assert_eq!(
            bars,
            [
                ([50.0, 17.0, 1.0, 24.0], Paint::Base(Base::A)),
                ([50.0, 1.0, 1.0, 16.0], Paint::Base(Base::G)),
                ([60.0, 1.0, 1.0, 40.0], Paint::Coverage),
            ]
        );
    }

    #[test]
    fn zoomed_out_columns_show_the_deepest_base() {
        let mut pile = Coverage::new(region(0, 1_000));
        pile.add(&alignment(0, 0, vec![(Op::Match, 5)], &[b'A'; 5]));
        pile.add(&alignment(3, 0, vec![(Op::Match, 5)], &[b'A'; 5]));
        let bars = rects(&coverage(view(0, 1_000), &pile, |_| Some(b'A'), 41.0));
        assert_eq!(bars, [([0.0, 1.0, 1.0, 40.0], Paint::Coverage)]);
    }

    fn read(start: u64, flags: u16, cigar: Vec<(Op, u32)>, bases: &[u8]) -> Read {
        Read::new(
            alignment(start, flags, cigar, bases),
            &[b'A'; 100],
            0,
            false,
        )
    }

    #[test]
    fn groups_leave_a_labeled_band_above_their_rows() {
        let shown = [
            read(10, 0, vec![(Op::Match, 10)], &[b'A'; 10]),
            read(10, 0, vec![(Op::Match, 10)], &[b'A'; 10]),
        ];
        let rows = [0, 1];
        let groups = [(0, "Haplotype 1".to_owned()), (1, "Haplotype 2".to_owned())];
        let primitives = reads(view(0, 100), &shown, &rows, &groups, Mode::Expanded, PAIRS);
        let tops: Vec<f32> = rects(&primitives).iter().map(|(rect, _)| rect[1]).collect();
        assert_eq!(tops, [15.0, 39.0]);
        let labels: Vec<(String, f32, f32)> = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text { content, x, y, .. } => Some((content.to_string(), *x, *y)),
                _ => None,
            })
            .collect();
        assert_eq!(
            labels,
            [
                ("Haplotype 1".to_owned(), 4.0, 7.0),
                ("Haplotype 2".to_owned(), 4.0, 31.0),
            ]
        );
        let at = |y| {
            hit(
                view(0, 100),
                &shown,
                &rows,
                &groups,
                Mode::Expanded,
                15.0,
                y,
            )
        };
        assert_eq!(at(20.0), Some(0));
        assert_eq!(at(43.0), Some(1));
        assert_eq!(at(5.0), None);
        assert_eq!(at(30.0), None);
        let height = super::height(2, &groups, Mode::Expanded);
        assert!((height - 48.0).abs() < f32::EPSILON, "{height}");
    }

    #[test]
    fn unusual_pairs_take_their_color() {
        let mut shown: Vec<Read> = [Pairing::Large, Pairing::Small, Pairing::SameStrand]
            .into_iter()
            .chain([Pairing::Outward, Pairing::Elsewhere, Pairing::Normal])
            .enumerate()
            .map(|(index, pairing)| {
                let mut read = read(index as u64 * 15, 0, vec![(Op::Match, 10)], &[b'A'; 10]);
                read.pairing = pairing;
                read
            })
            .collect();
        shown[5].alignment.flags = REVERSE;
        let rows = [0; 6];
        let paints = |pairs| -> Vec<Paint> {
            rects(&reads(
                view(0, 100),
                &shown,
                &rows,
                &[],
                Mode::Expanded,
                Look { pairs, mods: false },
            ))
            .into_iter()
            .map(|(_, paint)| paint)
            .collect()
        };
        assert_eq!(
            paints(true),
            [
                Paint::Pair(Hue::Vermillion),
                Paint::Pair(Hue::Blue),
                Paint::Pair(Hue::Green),
                Paint::Pair(Hue::Purple),
                Paint::Pair(Hue::Orange),
                Paint::Strand(Strand::Reverse),
            ]
        );
        let plain = paints(false);
        assert_eq!(plain[0], Paint::Strand(Strand::Forward));
        assert_eq!(plain[5], Paint::Strand(Strand::Reverse));
    }

    #[test]
    fn junctions_draw_arcs_with_their_counts() {
        let junction = |start, end, count, strand| Junction {
            span: region(start, end),
            count,
            strand,
        };
        let junctions = [
            junction(10, 60, 8, Strand::Forward),
            junction(70, 72, 1, Strand::Unknown),
            junction(500, 600, 16, Strand::Reverse),
        ];
        let primitives = arcs(view(0, 100), &junctions);
        let found: Vec<(f32, f32, f32, f32, f32, Paint)> = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Arc {
                    from,
                    to,
                    base,
                    rise,
                    width,
                    paint,
                } => Some((*from, *to, *base, *rise, *width, *paint)),
                _ => None,
            })
            .collect();
        assert_eq!(
            found,
            [
                (70.0, 72.0, 30.0, 4.0, 1.0, Paint::Muted),
                (10.0, 60.0, 30.0, 16.0, 2.5, Paint::Hue(Hue::Blue)),
            ]
        );
        assert_eq!(texts(&primitives), [("8".to_owned(), 35.0)]);
        let at = |x, y| arc_hit(view(0, 100), &junctions, x, y);
        assert_eq!(at(35.0, 14.0), Some(0));
        assert_eq!(at(71.0, 26.0), Some(1));
        assert_eq!(at(35.0, 30.0), None);
        assert_eq!(at(5.0, 29.0), None);
    }

    #[test]
    fn junction_counts_skip_crowded_places() {
        let junction = |start, end, count, strand| Junction {
            span: region(start, end),
            count,
            strand,
        };
        let junctions = [
            junction(0, 40, 10, Strand::Forward),
            junction(0, 40, 5, Strand::Reverse),
            junction(60, 90, 1, Strand::Unknown),
        ];
        let primitives = arcs(view(0, 100), &junctions);
        assert_eq!(
            texts(&primitives),
            [("10".to_owned(), 20.0), ("1".to_owned(), 75.0)]
        );
    }

    #[test]
    fn split_reads_mark_their_clipped_end() {
        let part = Part {
            contig: "chr2".to_owned(),
            start: 99,
            reverse: false,
            mapq: 60,
        };
        let mut right = read(
            10,
            0,
            vec![(Op::Match, 10), (Op::SoftClip, 30)],
            &[b'A'; 40],
        );
        right.alignment.tags.parts = vec![part.clone()];
        let mut left = read(
            50,
            0,
            vec![(Op::HardClip, 30), (Op::Match, 10)],
            &[b'A'; 10],
        );
        left.alignment.tags.parts = vec![part];
        let plain = read(
            70,
            0,
            vec![(Op::Match, 10), (Op::SoftClip, 30)],
            &[b'A'; 40],
        );
        let found = rects(&reads(
            view(0, 100),
            &[right, left, plain],
            &[0, 0, 0],
            &[],
            Mode::Expanded,
            PAIRS,
        ));
        let marks: Vec<[f32; 4]> = found
            .iter()
            .filter(|(_, paint)| *paint == Paint::Text)
            .map(|(rect, _)| *rect)
            .collect();
        assert_eq!(marks, [[19.0, 1.0, 2.0, 8.0], [49.0, 1.0, 2.0, 8.0]]);
    }

    #[test]
    fn modifications_color_plain_reads() {
        let mut marked = read(10, 0, vec![(Op::Match, 10)], b"ACGTACGTAC");
        marked.parts.modified = vec![(11, 250), (15, 5), (19, 128)];
        let mut repeat = read(30, 0, vec![(Op::Match, 10)], &[b'A'; 10]);
        repeat.alignment.mapq = 0;
        let shown = [marked, repeat];
        let mods = Look {
            pairs: true,
            mods: true,
        };
        let found = rects(&reads(
            view(0, 100),
            &shown,
            &[0, 1],
            &[],
            Mode::Expanded,
            mods,
        ));
        assert_eq!(
            found[0],
            ([10.0, 1.0, 10.0, 8.0], Paint::Plain(Shade::Full))
        );
        assert!(found.contains(&([30.0, 11.0, 10.0, 8.0], Paint::Plain(Shade::Faded))));
        let marks: Vec<([f32; 4], Paint)> = found
            .iter()
            .filter(|(_, paint)| matches!(paint, Paint::Modified(_)))
            .copied()
            .collect();
        assert_eq!(
            marks,
            [
                ([11.0, 1.0, 1.0, 8.0], Paint::Modified(250)),
                ([15.0, 1.0, 1.0, 8.0], Paint::Modified(5)),
                ([19.0, 1.0, 1.0, 8.0], Paint::Modified(128)),
            ]
        );
        let at_11: Vec<Paint> = found
            .iter()
            .filter(|(rect, _)| (rect[0] - 11.0).abs() < f32::EPSILON)
            .map(|(_, paint)| *paint)
            .collect();
        assert_eq!(at_11, [Paint::Modified(250), Paint::Base(Base::C)]);
        let zoomed = rects(&reads(
            view(0, 1_000),
            &shown,
            &[0, 1],
            &[],
            Mode::Expanded,
            mods,
        ));
        let merged: Vec<([f32; 4], Paint)> = zoomed
            .into_iter()
            .filter(|(_, paint)| matches!(paint, Paint::Modified(_)))
            .collect();
        assert_eq!(merged, [([1.0, 1.0, 1.0, 8.0], Paint::Modified(128))]);
        let plain = rects(&reads(
            view(0, 100),
            &shown,
            &[0, 1],
            &[],
            Mode::Expanded,
            PAIRS,
        ));
        assert!(
            plain
                .iter()
                .all(|(_, paint)| !matches!(paint, Paint::Modified(_)))
        );
        assert_eq!(plain[0].1, Paint::Strand(Strand::Forward));
    }

    #[test]
    fn reads_placed_in_more_than_one_spot_are_faint() {
        let mut repeat = read(10, REVERSE, vec![(Op::Match, 10)], &[b'A'; 10]);
        repeat.alignment.mapq = 0;
        repeat.pairing = Pairing::Large;
        let primitives = reads(view(0, 100), &[repeat], &[0], &[], Mode::Expanded, PAIRS);
        assert_eq!(
            rects(&primitives),
            [([10.0, 1.0, 10.0, 8.0], Paint::Faint(Strand::Reverse))]
        );
    }

    #[test]
    fn reads_take_their_strand_tint() {
        let forward = read(10, 0, vec![(Op::Match, 10)], &[b'A'; 10]);
        let reverse = read(10, REVERSE, vec![(Op::Match, 10)], &[b'A'; 10]);
        let primitives = reads(
            view(0, 100),
            &[forward, reverse],
            &[0, 1],
            &[],
            Mode::Expanded,
            PAIRS,
        );
        assert_eq!(
            rects(&primitives),
            [
                ([10.0, 1.0, 10.0, 8.0], Paint::Strand(Strand::Forward)),
                ([10.0, 11.0, 10.0, 8.0], Paint::Strand(Strand::Reverse)),
            ]
        );
    }

    #[test]
    fn gaps_draw_lines_and_insertions_draw_marks() {
        let cigar = vec![
            (Op::Match, 5),
            (Op::Insertion, 2),
            (Op::Match, 5),
            (Op::Deletion, 4),
            (Op::Match, 5),
            (Op::Skip, 20),
            (Op::Match, 5),
        ];
        let spliced = read(10, 0, cigar, &[b'A'; 22]);
        let primitives = reads(view(0, 100), &[spliced], &[0], &[], Mode::Expanded, PAIRS);
        let found = rects(&primitives);
        assert!(found.contains(&([20.0, 4.5, 4.0, 1.0], Paint::Text)));
        assert!(found.contains(&([29.0, 4.5, 20.0, 1.0], Paint::Muted)));
        assert!(found.contains(&([14.0, 1.0, 2.0, 8.0], Paint::Insertion)));
    }

    #[test]
    fn mismatches_use_base_colors_and_fade_with_low_quality() {
        let mut low = alignment(10, 0, vec![(Op::Match, 4)], b"AGAT");
        low.qualities = vec![30, 30, 30, 10];
        let primitives = reads(
            view(0, 100),
            &[Read::new(low, &[b'A'; 100], 0, false)],
            &[0],
            &[],
            Mode::Expanded,
            PAIRS,
        );
        let found = rects(&primitives);
        assert!(found.contains(&([11.0, 1.0, 1.0, 8.0], Paint::Base(Base::G))));
        assert!(found.contains(&([13.0, 1.0, 1.0, 8.0], Paint::Faded(Base::T))));
        assert_eq!(texts(&primitives), []);
    }

    #[test]
    fn mismatch_letters_appear_when_zoomed_in() {
        let mismatch = read(2, 0, vec![(Op::Match, 4)], b"AGAA");
        let primitives = reads(view(0, 10), &[mismatch], &[0], &[], Mode::Expanded, PAIRS);
        assert_eq!(texts(&primitives), [("G".to_owned(), 34.5)]);
    }

    #[test]
    fn squished_rows_are_three_pixels() {
        let pair = [
            read(10, 0, vec![(Op::Match, 10)], &[b'A'; 10]),
            read(10, 0, vec![(Op::Match, 10)], &[b'A'; 10]),
        ];
        let tops: Vec<f32> = rects(&reads(
            view(0, 100),
            &pair,
            &[0, 1],
            &[],
            Mode::Squished,
            PAIRS,
        ))
        .iter()
        .map(|(rect, _)| rect[1])
        .collect();
        assert_eq!(tops, [0.0, 3.0]);
    }

    #[test]
    fn fit_squishes_deep_piles() {
        assert_eq!(fit(LIMIT), Mode::Expanded);
        assert_eq!(fit(LIMIT + 1), Mode::Squished);
    }

    #[test]
    fn hits_the_read_under_the_pointer() {
        let pair = [
            read(10, 0, vec![(Op::Match, 10)], &[b'A'; 10]),
            read(40, 0, vec![(Op::Match, 10)], &[b'A'; 10]),
        ];
        let rows = [0, 1];
        assert_eq!(
            hit(view(0, 100), &pair, &rows, &[], Mode::Expanded, 15.0, 5.0),
            Some(0)
        );
        assert_eq!(
            hit(view(0, 100), &pair, &rows, &[], Mode::Expanded, 45.0, 15.0),
            Some(1)
        );
        assert_eq!(
            hit(view(0, 100), &pair, &rows, &[], Mode::Expanded, 45.0, 5.0),
            None
        );
    }

    fn clipped(start: u64, cigar: Vec<(Op, u32)>, bases: &[u8]) -> Read {
        Read::new(alignment(start, 0, cigar, bases), &[b'A'; 200], 0, true)
    }

    #[test]
    fn soft_clips_draw_as_bases_beside_the_read() {
        let read = clipped(10, vec![(Op::SoftClip, 2), (Op::Match, 4)], b"GTAAAA");
        let found = rects(&reads(
            view(0, 100),
            &[read],
            &[0],
            &[],
            Mode::Expanded,
            PAIRS,
        ));
        assert!(found.contains(&([8.0, 1.0, 1.0, 8.0], Paint::Base(Base::G))));
        assert!(found.contains(&([9.0, 1.0, 1.0, 8.0], Paint::Base(Base::T))));
    }

    #[test]
    fn soft_clips_count_for_drawing_and_hits() {
        let read = clipped(103, vec![(Op::SoftClip, 6), (Op::Match, 4)], b"GGGGGGAAAA");
        let found = rects(&reads(
            view(0, 100),
            &[read],
            &[0],
            &[],
            Mode::Expanded,
            PAIRS,
        ));
        assert!(found.contains(&([97.0, 1.0, 1.0, 8.0], Paint::Base(Base::G))));
        let read = clipped(50, vec![(Op::SoftClip, 10), (Op::Match, 4)], &[b'C'; 14]);
        assert_eq!(
            hit(view(0, 100), &[read], &[0], &[], Mode::Expanded, 42.0, 5.0),
            Some(0)
        );
    }
}
