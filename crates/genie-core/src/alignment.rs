use crate::modification::Modifications;
use crate::{Region, Strand};

pub const PAIRED: u16 = 0x1;
pub const PROPER: u16 = 0x2;
pub const UNMAPPED: u16 = 0x4;
pub const MATE_UNMAPPED: u16 = 0x8;
pub const REVERSE: u16 = 0x10;
pub const MATE_REVERSE: u16 = 0x20;
pub const FIRST: u16 = 0x40;
pub const SECOND: u16 = 0x80;
pub const SECONDARY: u16 = 0x100;
pub const QC_FAIL: u16 = 0x200;
pub const DUPLICATE: u16 = 0x400;
pub const SUPPLEMENTARY: u16 = 0x800;

const MIN_PAIRS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Match,
    Insertion,
    Deletion,
    Skip,
    SoftClip,
    HardClip,
    Padding,
    Equal,
    Diff,
}

impl Op {
    pub const fn consumes_reference(self) -> bool {
        matches!(
            self,
            Self::Match | Self::Deletion | Self::Skip | Self::Equal | Self::Diff
        )
    }

    pub const fn letter(self) -> char {
        match self {
            Self::Match => 'M',
            Self::Insertion => 'I',
            Self::Deletion => 'D',
            Self::Skip => 'N',
            Self::SoftClip => 'S',
            Self::HardClip => 'H',
            Self::Padding => 'P',
            Self::Equal => '=',
            Self::Diff => 'X',
        }
    }

    pub const fn consumes_read(self) -> bool {
        matches!(
            self,
            Self::Match | Self::Insertion | Self::SoftClip | Self::Equal | Self::Diff
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mate {
    pub contig: usize,
    pub start: u64,
    pub insert: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags {
    pub phase: Option<i64>,
    pub group: Option<String>,
    pub parts: Vec<Part>,
    pub xs: Option<Strand>,
    pub ts: Option<Strand>,
    pub mods: Modifications,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub contig: String,
    pub start: u64,
    pub reverse: bool,
    pub mapq: u8,
}

pub fn parts(text: &str) -> Vec<Part> {
    text.split(';')
        .filter_map(|entry| {
            let mut fields = entry.split(',');
            let contig = fields.next()?.trim();
            let position: u64 = fields.next()?.parse().ok()?;
            let reverse = match fields.next()? {
                "+" => false,
                "-" => true,
                _ => return None,
            };
            let _cigar = fields.next()?;
            let mapq: u8 = fields.next()?.parse().ok()?;
            (!contig.is_empty()).then(|| Part {
                contig: contig.to_owned(),
                start: position.saturating_sub(1),
                reverse,
                mapq,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alignment {
    pub name: String,
    pub start: u64,
    pub flags: u16,
    pub mapq: u8,
    pub cigar: Vec<(Op, u32)>,
    pub bases: Vec<u8>,
    pub qualities: Vec<u8>,
    pub mate: Option<Mate>,
    pub tags: Tags,
}

impl Alignment {
    pub fn end(&self) -> u64 {
        let length: u64 = self
            .cigar
            .iter()
            .filter(|(op, _)| op.consumes_reference())
            .map(|(_, length)| u64::from(*length))
            .sum();
        self.start + length
    }

    pub fn span(&self) -> Region {
        Region::between(self.start, self.end())
    }

    pub fn strand(&self) -> Strand {
        if self.has(REVERSE) {
            Strand::Reverse
        } else {
            Strand::Forward
        }
    }

    pub fn has(&self, flag: u16) -> bool {
        self.flags & flag != 0
    }

    pub fn splice_strand(&self) -> Strand {
        match (self.tags.xs, self.tags.ts) {
            (Some(strand), _) if strand != Strand::Unknown => strand,
            (_, Some(Strand::Forward)) => self.strand(),
            (_, Some(Strand::Reverse)) => match self.strand() {
                Strand::Forward => Strand::Reverse,
                _ => Strand::Forward,
            },
            _ => Strand::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gap {
    Deletion,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mismatch {
    pub position: u64,
    pub base: u8,
    pub quality: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parts {
    pub blocks: Vec<Region>,
    pub gaps: Vec<(Region, Gap)>,
    pub insertions: Vec<(u64, u32)>,
    pub mismatches: Vec<Mismatch>,
    pub clips: Vec<Mismatch>,
    pub modified: Vec<(u64, u8)>,
}

impl Alignment {
    pub fn parts(&self, reference: &[u8], offset: u64) -> Parts {
        let mut parts = Parts::default();
        let (mut position, mut index) = (self.start, 0_usize);
        for &(op, length) in &self.cigar {
            let size = u64::from(length);
            let count = length as usize;
            match op {
                Op::Match | Op::Equal | Op::Diff => {
                    let block = Region::between(position, position + size);
                    match parts.blocks.last_mut() {
                        Some(last) if last.end() == position => *last = last.hull(block),
                        _ => parts.blocks.push(block),
                    }
                    for step in 0..count {
                        let at = position + step as u64;
                        let Some(&base) = self.bases.get(index + step) else {
                            break;
                        };
                        let expected = at
                            .checked_sub(offset)
                            .and_then(|delta| reference.get(usize::try_from(delta).ok()?));
                        let differs = match op {
                            Op::Diff => true,
                            Op::Equal => false,
                            _ => expected.is_some_and(|expected| {
                                base != b'='
                                    && !expected.eq_ignore_ascii_case(&b'N')
                                    && !base.eq_ignore_ascii_case(expected)
                            }),
                        };
                        if differs {
                            parts.mismatches.push(Mismatch {
                                position: at,
                                base,
                                quality: self
                                    .qualities
                                    .get(index + step)
                                    .copied()
                                    .unwrap_or(u8::MAX),
                            });
                        }
                    }
                    position += size;
                    index += count;
                }
                Op::Insertion => {
                    parts.insertions.push((position, length));
                    index += count;
                }
                Op::Deletion | Op::Skip => {
                    let gap = if op == Op::Deletion {
                        Gap::Deletion
                    } else {
                        Gap::Skip
                    };
                    parts
                        .gaps
                        .push((Region::between(position, position + size), gap));
                    position += size;
                }
                Op::SoftClip => {
                    let leading = parts.blocks.is_empty() && parts.gaps.is_empty();
                    for step in 0..count {
                        let Some(&base) = self.bases.get(index + step) else {
                            break;
                        };
                        let at = position + step as u64;
                        let at = if leading {
                            at.checked_sub(size)
                        } else {
                            Some(at)
                        };
                        if let Some(at) = at {
                            parts.clips.push(Mismatch {
                                position: at,
                                base,
                                quality: self
                                    .qualities
                                    .get(index + step)
                                    .copied()
                                    .unwrap_or(u8::MAX),
                            });
                        }
                    }
                    index += count;
                }
                Op::HardClip | Op::Padding => {}
            }
        }
        parts.modified = self.modified();
        parts
    }

    fn modified(&self) -> Vec<(u64, u8)> {
        let mut found: Vec<(u64, u8)> = Vec::new();
        for placed in self.placed_calls() {
            match found.last_mut() {
                Some((last, chance)) if *last == placed.position => {
                    *chance = chance.saturating_add(placed.chance);
                }
                _ => found.push((placed.position, placed.chance)),
            }
        }
        found
    }

    pub fn placed_calls(&self) -> Vec<Placed> {
        let calls = &self.tags.mods.calls;
        let mut found = Vec::new();
        let (mut position, mut index, mut next) = (self.start, 0_u64, 0);
        for &(op, length) in &self.cigar {
            let size = u64::from(length);
            if matches!(op, Op::Match | Op::Equal | Op::Diff) {
                while let Some(call) = calls.get(next) {
                    let at = u64::from(call.at);
                    if at >= index + size {
                        break;
                    }
                    next += 1;
                    if at >= index {
                        found.push(Placed {
                            position: position + (at - index),
                            code: call.code,
                            chance: call.chance,
                        });
                    }
                }
            }
            if op.consumes_reference() {
                position += size;
            }
            if op.consumes_read() {
                index += size;
            }
        }
        found
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub position: u64,
    pub code: u8,
    pub chance: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filter {
    hidden: u16,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            hidden: UNMAPPED | SECONDARY | QC_FAIL | DUPLICATE,
        }
    }
}

impl Filter {
    #[must_use]
    pub fn showing(self, flags: u16) -> Self {
        Self {
            hidden: self.hidden & !flags,
        }
    }

    pub fn keeps(self, alignment: &Alignment) -> bool {
        !alignment.has(self.hidden)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pairing {
    #[default]
    Normal,
    Large,
    Small,
    SameStrand,
    Outward,
    Elsewhere,
}

impl Alignment {
    fn paired_mate(&self) -> Option<Mate> {
        if self.has(PAIRED) && !self.has(MATE_UNMAPPED) {
            self.mate
        } else {
            None
        }
    }

    pub fn pairing(&self, contig: usize, range: Option<(u64, u64)>) -> Pairing {
        let Some(mate) = self.paired_mate() else {
            return Pairing::Normal;
        };
        if mate.contig != contig {
            return Pairing::Elsewhere;
        }
        let reverse = self.has(REVERSE);
        if reverse == self.has(MATE_REVERSE) {
            return Pairing::SameStrand;
        }
        let inward = if reverse {
            mate.start <= self.start
        } else {
            self.start <= mate.start
        };
        if !inward {
            return Pairing::Outward;
        }
        let size = mate.insert.unsigned_abs();
        match range {
            Some((_, high)) if size > high => Pairing::Large,
            Some((low, _)) if size > 0 && size < low => Pairing::Small,
            _ => Pairing::Normal,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Inserts {
    contig: usize,
    proper: Vec<u64>,
    inward: Vec<u64>,
}

impl Inserts {
    pub fn new(contig: usize) -> Self {
        Self {
            contig,
            ..Self::default()
        }
    }

    pub fn add(&mut self, alignment: &Alignment) {
        let Some(mate) = alignment.paired_mate() else {
            return;
        };
        let size = mate.insert.unsigned_abs();
        if size == 0 || alignment.pairing(self.contig, None) != Pairing::Normal {
            return;
        }
        self.inward.push(size);
        if alignment.has(PROPER) {
            self.proper.push(size);
        }
    }

    pub fn expected(self) -> Option<(u64, u64)> {
        let mut sizes = if self.proper.len() >= MIN_PAIRS {
            self.proper
        } else {
            self.inward
        };
        if sizes.len() < MIN_PAIRS {
            return None;
        }
        sizes.sort_unstable();
        let count = sizes.len();
        Some((
            sizes[count * 5 / 1000],
            sizes[(count * 995).div_ceil(1000) - 1],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Alignment, DUPLICATE, Filter, Gap, Inserts, MATE_REVERSE, MATE_UNMAPPED, Mate, Mismatch,
        Op, PAIRED, PROPER, Pairing, Part, Parts, Placed, QC_FAIL, REVERSE, SECONDARY,
        SUPPLEMENTARY, Tags, UNMAPPED, parts,
    };
    use crate::{Region, Strand};

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    #[test]
    fn split_parts_come_from_the_sa_tag() {
        let found = parts("chr2,1234,+,60M40S,60,0;chr5,99,-,40S60M,7,1;bad;chr1,x,+,1M,1,0;");
        let part = |contig: &str, start, reverse, mapq| Part {
            contig: contig.to_owned(),
            start,
            reverse,
            mapq,
        };
        assert_eq!(
            found,
            [part("chr2", 1233, false, 60), part("chr5", 98, true, 7)]
        );
        assert_eq!(parts(""), []);
    }

    fn pair(flags: u16, start: u64, contig: usize, mate_start: u64, insert: i64) -> Alignment {
        Alignment {
            start,
            mate: Some(Mate {
                contig,
                start: mate_start,
                insert,
            }),
            ..alignment(PAIRED | flags, vec![(Op::Match, 50)])
        }
    }

    #[test]
    fn pairs_fall_into_classes() {
        let range = Some((200, 600));
        let class = |read: &Alignment| read.pairing(0, range);
        assert_eq!(
            class(&pair(MATE_REVERSE, 100, 0, 400, 350)),
            Pairing::Normal
        );
        assert_eq!(class(&pair(REVERSE, 400, 0, 100, -350)), Pairing::Normal);
        assert_eq!(
            class(&pair(MATE_REVERSE, 100, 0, 5_000, 4_950)),
            Pairing::Large
        );
        assert_eq!(class(&pair(MATE_REVERSE, 100, 0, 120, 70)), Pairing::Small);
        assert_eq!(class(&pair(0, 100, 0, 400, 350)), Pairing::SameStrand);
        assert_eq!(
            class(&pair(REVERSE | MATE_REVERSE, 100, 0, 400, 350)),
            Pairing::SameStrand
        );
        assert_eq!(class(&pair(REVERSE, 100, 0, 400, 350)), Pairing::Outward);
        assert_eq!(
            class(&pair(MATE_REVERSE, 100, 1, 400, 0)),
            Pairing::Elsewhere
        );
        assert_eq!(class(&pair(MATE_UNMAPPED, 100, 0, 100, 0)), Pairing::Normal);
        let single = alignment(0, vec![(Op::Match, 50)]);
        assert_eq!(class(&single), Pairing::Normal);
        let large = pair(MATE_REVERSE, 100, 0, 5_000, 4_950);
        assert_eq!(large.pairing(0, None), Pairing::Normal);
    }

    #[test]
    fn expected_inserts_come_from_proper_pairs() {
        let mut reads: Vec<Alignment> = (0..300)
            .map(|index| pair(PROPER | MATE_REVERSE, 100, 0, 400, 300 + index))
            .collect();
        reads.extend((0..20).map(|_| pair(MATE_REVERSE, 100, 0, 9_000, 8_950)));
        let expected = |reads: &[Alignment]| {
            let mut inserts = Inserts::new(0);
            for read in reads {
                inserts.add(read);
            }
            inserts.expected()
        };
        assert_eq!(expected(&reads), Some((301, 598)));
        assert_eq!(expected(&reads[..50]), None);
    }

    #[test]
    fn parts_follow_the_cigar_and_the_reference() {
        let read = Alignment {
            bases: b"TTACGTAGGACGCGTTA".to_vec(),
            qualities: (0..17).collect(),
            ..alignment(
                0,
                vec![
                    (Op::SoftClip, 2),
                    (Op::Match, 5),
                    (Op::Insertion, 2),
                    (Op::Match, 3),
                    (Op::Deletion, 2),
                    (Op::Equal, 3),
                    (Op::Skip, 100),
                    (Op::Diff, 2),
                ],
            )
        };
        let reference = b"ACGTTACGxxGTAxxxxx";
        assert_eq!(
            read.parts(reference, 100),
            Parts {
                blocks: vec![region(100, 108), region(110, 113), region(213, 215)],
                gaps: vec![
                    (region(108, 110), Gap::Deletion),
                    (region(113, 213), Gap::Skip)
                ],
                insertions: vec![(105, 2)],
                mismatches: vec![
                    Mismatch {
                        position: 104,
                        base: b'A',
                        quality: 6
                    },
                    Mismatch {
                        position: 213,
                        base: b'T',
                        quality: 15
                    },
                    Mismatch {
                        position: 214,
                        base: b'A',
                        quality: 16
                    },
                ],
                clips: vec![
                    Mismatch {
                        position: 98,
                        base: b'T',
                        quality: 0
                    },
                    Mismatch {
                        position: 99,
                        base: b'T',
                        quality: 1
                    },
                ],
                modified: Vec::new(),
            }
        );
    }

    #[test]
    fn parts_place_modifications_on_the_reference() {
        use crate::modification::{Call, Modifications};
        let call = |at, code, chance| Call { at, code, chance };
        let calls = vec![
            call(0, 0, 90),
            call(3, 0, 100),
            call(3, 1, 50),
            call(4, 0, 200),
            call(4, 1, 100),
            call(8, 0, 255),
            call(10, 0, 7),
            call(16, 0, 1),
        ];
        let read = Alignment {
            bases: b"TTACGTAGGACGCGTTA".to_vec(),
            tags: Tags {
                mods: Modifications {
                    codes: Vec::new(),
                    calls,
                },
                ..Tags::default()
            },
            ..alignment(
                0,
                vec![
                    (Op::SoftClip, 2),
                    (Op::Match, 5),
                    (Op::Insertion, 2),
                    (Op::Match, 3),
                    (Op::Deletion, 2),
                    (Op::Equal, 3),
                    (Op::Skip, 100),
                    (Op::Diff, 2),
                ],
            )
        };
        assert_eq!(
            read.parts(b"", 100).modified,
            [(101, 150), (102, 255), (106, 7), (214, 1)]
        );
        let placed = |position, code, chance| Placed {
            position,
            code,
            chance,
        };
        assert_eq!(
            read.placed_calls(),
            [
                placed(101, 0, 100),
                placed(101, 1, 50),
                placed(102, 0, 200),
                placed(102, 1, 100),
                placed(106, 0, 7),
                placed(214, 0, 1),
            ]
        );
    }

    fn clip(position: u64, base: u8, quality: u8) -> Mismatch {
        Mismatch {
            position,
            base,
            quality,
        }
    }

    #[test]
    fn parts_keep_soft_clipped_bases_beside_the_read() {
        let read = Alignment {
            bases: b"ACGTAC".to_vec(),
            qualities: vec![30, 31, 32, 33, 34, 35],
            ..alignment(
                0,
                vec![
                    (Op::HardClip, 5),
                    (Op::SoftClip, 2),
                    (Op::Match, 3),
                    (Op::SoftClip, 1),
                ],
            )
        };
        assert_eq!(
            read.parts(b"", 0).clips,
            [clip(98, b'A', 30), clip(99, b'C', 31), clip(103, b'C', 35)]
        );
    }

    #[test]
    fn clips_before_the_contig_start_are_dropped() {
        let read = Alignment {
            start: 1,
            bases: b"GTCAA".to_vec(),
            ..alignment(0, vec![(Op::SoftClip, 3), (Op::Match, 2)])
        };
        assert_eq!(read.parts(b"", 0).clips, [clip(0, b'C', u8::MAX)]);
    }

    #[test]
    fn filters_can_show_hidden_flags() {
        let filter = Filter::default().showing(DUPLICATE);
        assert!(filter.keeps(&alignment(DUPLICATE, Vec::new())));
        assert!(!filter.keeps(&alignment(QC_FAIL, Vec::new())));
    }

    #[test]
    fn parts_skip_mismatches_without_bases_or_reference() {
        let read = alignment(0, vec![(Op::Match, 4)]);
        assert_eq!(read.parts(b"", 0).mismatches, []);
        let read = Alignment {
            bases: b"ACGN".to_vec(),
            ..alignment(0, vec![(Op::Match, 4)])
        };
        let mismatches = read.parts(b"TTTT", 102).mismatches;
        let positions: Vec<u64> = mismatches
            .iter()
            .map(|mismatch| mismatch.position)
            .collect();
        assert_eq!(positions, [102, 103]);
        assert_eq!(mismatches[1].base, b'N');
    }

    fn alignment(flags: u16, cigar: Vec<(Op, u32)>) -> Alignment {
        Alignment {
            name: "read".to_owned(),
            start: 100,
            flags,
            mapq: 60,
            cigar,
            bases: Vec::new(),
            qualities: Vec::new(),
            mate: None,
            tags: crate::alignment::Tags::default(),
        }
    }

    #[test]
    fn spans_the_reference_operations() {
        let cigar = vec![
            (Op::SoftClip, 6),
            (Op::Match, 10),
            (Op::Insertion, 2),
            (Op::Match, 5),
            (Op::Deletion, 3),
            (Op::Skip, 4),
            (Op::Equal, 1),
            (Op::Diff, 1),
            (Op::HardClip, 9),
        ];
        let read = alignment(0, cigar);
        assert_eq!(read.end(), 124);
        assert_eq!(read.span(), Region::new(100, 124).unwrap());
    }

    #[test]
    fn letters_follow_the_sam_specification() {
        let ops = [
            Op::Match,
            Op::Insertion,
            Op::Deletion,
            Op::Skip,
            Op::SoftClip,
            Op::HardClip,
            Op::Padding,
            Op::Equal,
            Op::Diff,
        ];
        let letters: String = ops.into_iter().map(Op::letter).collect();
        assert_eq!(letters, "MIDNSHP=X");
    }

    #[test]
    fn strand_comes_from_the_reverse_flag() {
        assert_eq!(alignment(0, Vec::new()).strand(), Strand::Forward);
        assert_eq!(alignment(REVERSE, Vec::new()).strand(), Strand::Reverse);
    }

    #[test]
    fn default_filter_hides_duplicates_failures_and_secondary_reads() {
        let filter = Filter::default();
        for flags in [DUPLICATE, QC_FAIL, SECONDARY, UNMAPPED] {
            assert!(!filter.keeps(&alignment(flags, Vec::new())), "{flags:#x}");
        }
        for flags in [0, REVERSE, SUPPLEMENTARY] {
            assert!(filter.keeps(&alignment(flags, Vec::new())), "{flags:#x}");
        }
    }
}
