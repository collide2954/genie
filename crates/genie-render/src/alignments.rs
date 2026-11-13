use std::borrow::Cow;

use genie_core::alignment::{Alignment, Gap, Op, Pairing};
use genie_core::pileup::{Coverage, Read};
use genie_core::{Region, Viewport};

use crate::features::Mode;
use crate::scene::{Align, Base, Font, Hue, Paint, Primitive};

pub const COVERAGE_HEIGHT: f32 = 36.0;
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
        x: viewport.width() as f32 - 4.0,
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

fn tint(read: &Read, pairs: bool) -> Paint {
    let strand = read.alignment.strand();
    if read.alignment.mapq == 0 {
        return Paint::Faint(strand);
    }
    pair_hue(read.pairing)
        .filter(|_| pairs)
        .map_or(Paint::Strand(strand), Paint::Pair)
}

pub fn reads(
    viewport: Viewport,
    reads: &[Read],
    rows: &[usize],
    groups: &[(usize, String)],
    mode: Mode,
    pairs: bool,
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
        let tint = tint(read, pairs);
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
    use genie_core::pileup::{Coverage, Read};
    use genie_core::{Region, Strand, Viewport};

    use super::{LIMIT, coverage, fit, hit, reads};
    use crate::features::Mode;
    use crate::scene::{Base, Hue, Paint, Primitive};

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
        assert_eq!(labels(&primitives), [("0-4".to_owned(), 96.0)]);
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
        let primitives = reads(view(0, 100), &shown, &rows, &groups, Mode::Expanded, true);
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
                pairs,
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
            true,
        ));
        let marks: Vec<[f32; 4]> = found
            .iter()
            .filter(|(_, paint)| *paint == Paint::Text)
            .map(|(rect, _)| *rect)
            .collect();
        assert_eq!(marks, [[19.0, 1.0, 2.0, 8.0], [49.0, 1.0, 2.0, 8.0]]);
    }

    #[test]
    fn reads_placed_in_more_than_one_spot_are_faint() {
        let mut repeat = read(10, REVERSE, vec![(Op::Match, 10)], &[b'A'; 10]);
        repeat.alignment.mapq = 0;
        repeat.pairing = Pairing::Large;
        let primitives = reads(view(0, 100), &[repeat], &[0], &[], Mode::Expanded, true);
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
            true,
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
        let primitives = reads(view(0, 100), &[spliced], &[0], &[], Mode::Expanded, true);
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
            true,
        );
        let found = rects(&primitives);
        assert!(found.contains(&([11.0, 1.0, 1.0, 8.0], Paint::Base(Base::G))));
        assert!(found.contains(&([13.0, 1.0, 1.0, 8.0], Paint::Faded(Base::T))));
        assert_eq!(texts(&primitives), []);
    }

    #[test]
    fn mismatch_letters_appear_when_zoomed_in() {
        let mismatch = read(2, 0, vec![(Op::Match, 4)], b"AGAA");
        let primitives = reads(view(0, 10), &[mismatch], &[0], &[], Mode::Expanded, true);
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
            true,
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
            true,
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
            true,
        ));
        assert!(found.contains(&([97.0, 1.0, 1.0, 8.0], Paint::Base(Base::G))));
        let read = clipped(50, vec![(Op::SoftClip, 10), (Op::Match, 4)], &[b'C'; 14]);
        assert_eq!(
            hit(view(0, 100), &[read], &[0], &[], Mode::Expanded, 42.0, 5.0),
            Some(0)
        );
    }
}
