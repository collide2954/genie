use std::collections::HashMap;

use crate::alignment::{Alignment, Gap, Op, Pairing, Parts};
use crate::{Region, Strand};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    pub alignment: Alignment,
    pub parts: Parts,
    pub extent: Region,
    pub pairing: Pairing,
}

impl Read {
    pub fn new(alignment: Alignment, reference: &[u8], offset: u64, clips: bool) -> Self {
        let mut parts = alignment.parts(reference, offset);
        let mut extent = alignment.span();
        if clips {
            for clip in &parts.clips {
                extent = extent.hull(Region::between(clip.position, clip.position + 1));
            }
        } else {
            parts.clips.clear();
        }
        Self {
            alignment,
            parts,
            extent,
            pairing: Pairing::Normal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    region: Region,
    counts: Vec<[u32; 5]>,
}

impl Coverage {
    pub fn new(region: Region) -> Self {
        Self {
            region,
            counts: vec![[0; 5]; region.len() as usize],
        }
    }

    pub fn region(&self) -> Region {
        self.region
    }

    pub fn add(&mut self, alignment: &Alignment) {
        let (mut position, mut index) = (alignment.start, 0_usize);
        for &(op, length) in &alignment.cigar {
            if matches!(op, Op::Match | Op::Equal | Op::Diff) {
                for step in 0..length as usize {
                    if let Some(slot) = self.slot(position + step as u64) {
                        let base = alignment.bases.get(index + step).copied().unwrap_or(b'N');
                        self.counts[slot][bucket(base)] += 1;
                    }
                }
            }
            if op.consumes_reference() {
                position += u64::from(length);
            }
            if op.consumes_read() {
                index += length as usize;
            }
        }
    }

    pub fn counts(&self, position: u64) -> Option<[u32; 5]> {
        self.slot(position).map(|slot| self.counts[slot])
    }

    pub fn depth(&self, position: u64) -> u32 {
        self.counts(position)
            .map_or(0, |counts| counts.iter().sum())
    }

    fn slot(&self, position: u64) -> Option<usize> {
        self.region
            .contains(position)
            .then(|| (position - self.region.start()) as usize)
    }
}

#[derive(Debug)]
pub struct Sampler {
    window: u64,
    limit: usize,
    bucket: Option<u64>,
    seen: usize,
    state: u64,
    sample: Vec<(usize, Alignment)>,
    kept: Vec<Alignment>,
}

impl Sampler {
    pub fn new(window: u64, limit: usize) -> Self {
        Self {
            window: window.max(1),
            limit,
            bucket: None,
            seen: 0,
            state: 1,
            sample: Vec::new(),
            kept: Vec::new(),
        }
    }

    pub fn push(&mut self, alignment: Alignment) {
        let bucket = alignment.start / self.window;
        if self.bucket != Some(bucket) {
            self.flush();
            self.bucket = Some(bucket);
            self.state = bucket.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        }
        if self.sample.len() < self.limit {
            self.sample.push((self.seen, alignment));
        } else {
            self.state ^= self.state << 13;
            self.state ^= self.state >> 7;
            self.state ^= self.state << 17;
            let pick = (self.state % (self.seen as u64 + 1)) as usize;
            if pick < self.limit {
                self.sample[pick] = (self.seen, alignment);
            }
        }
        self.seen += 1;
    }

    pub fn held(&self) -> usize {
        self.sample.len() + self.kept.len()
    }

    pub fn finish(mut self) -> Vec<Alignment> {
        self.flush();
        self.kept
    }

    fn flush(&mut self) {
        self.sample.sort_unstable_by_key(|(seen, _)| *seen);
        self.kept
            .extend(self.sample.drain(..).map(|(_, alignment)| alignment));
        self.seen = 0;
    }
}

fn bucket(base: u8) -> usize {
    match base.to_ascii_uppercase() {
        b'A' => 0,
        b'C' => 1,
        b'G' => 2,
        b'T' => 3,
        _ => 4,
    }
}

fn key(read: &Read, position: u64) -> (u8, u8) {
    let parts = &read.parts;
    if let Some(mismatch) = parts
        .mismatches
        .iter()
        .find(|mismatch| mismatch.position == position)
    {
        return (0, bucket(mismatch.base) as u8);
    }
    if parts
        .gaps
        .iter()
        .any(|(gap, kind)| *kind == Gap::Deletion && gap.contains(position))
    {
        return (1, 0);
    }
    if parts.blocks.iter().any(|block| block.contains(position)) {
        return (2, 0);
    }
    (3, 0)
}

pub fn sort_rows(reads: &[Read], rows: &[usize], starts: &[usize], position: u64) -> Vec<usize> {
    let count = rows.iter().max().map_or(0, |row| row + 1);
    let mut keys = vec![(3, 0); count];
    for (read, &row) in reads.iter().zip(rows) {
        if read.extent.contains(position) {
            keys[row] = keys[row].min(key(read, position));
        }
    }
    let group = |row: usize| starts.partition_point(|start| *start <= row);
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by_key(|&row| (group(row), keys[row]));
    let mut placed = vec![0; count];
    for (new, &old) in order.iter().enumerate() {
        placed[old] = new;
    }
    rows.iter().map(|&row| placed[row]).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Junction {
    pub span: Region,
    pub count: u32,
    pub strand: Strand,
}

#[derive(Debug, Default)]
pub struct Junctions {
    found: HashMap<(u64, u64), (u32, Strand)>,
}

impl Junctions {
    pub fn add(&mut self, alignment: &Alignment) {
        let mut position = alignment.start;
        let mut splice = None;
        for &(op, length) in &alignment.cigar {
            let length = u64::from(length);
            if op == Op::Skip {
                let strand = *splice.get_or_insert_with(|| alignment.splice_strand());
                let entry = self
                    .found
                    .entry((position, position + length))
                    .or_insert((0, Strand::Unknown));
                entry.0 += 1;
                if entry.1 == Strand::Unknown {
                    entry.1 = strand;
                }
            }
            if op.consumes_reference() {
                position += length;
            }
        }
    }

    pub fn finish(self) -> Vec<Junction> {
        let mut found: Vec<Junction> = self
            .found
            .into_iter()
            .map(|((start, end), (count, strand))| Junction {
                span: Region::between(start, end),
                count,
                strand,
            })
            .collect();
        found.sort_unstable_by_key(|junction| (junction.span.start(), junction.span.end()));
        found
    }
}

pub fn motif_strand(reference: &[u8], offset: u64, intron: Region) -> Strand {
    let pair = |at: u64| -> Option<[u8; 2]> {
        let index = usize::try_from(at.checked_sub(offset)?).ok()?;
        let bases = reference.get(index..index.checked_add(2)?)?;
        Some([bases[0].to_ascii_uppercase(), bases[1].to_ascii_uppercase()])
    };
    let donor = pair(intron.start());
    let acceptor = intron.end().checked_sub(2).and_then(pair);
    match (donor, acceptor) {
        (Some(donor), Some(acceptor)) => match (&donor, &acceptor) {
            (b"GT" | b"GC", b"AG") | (b"AT", b"AC") => Strand::Forward,
            (b"CT", b"AC" | b"GC") | (b"GT", b"AT") => Strand::Reverse,
            _ => Strand::Unknown,
        },
        _ => Strand::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::{Coverage, Junction, Junctions, Read, Sampler, motif_strand, sort_rows};
    use crate::alignment::{Alignment, Op, REVERSE};
    use crate::{Region, Strand};

    #[test]
    fn junctions_count_every_read_that_skips() {
        let spliced = |start: u64, xs: Option<Strand>| {
            let mut alignment = read(
                start,
                vec![(Op::Match, 10), (Op::Skip, 100), (Op::Match, 10)],
                &[b'A'; 20],
            );
            alignment.tags.xs = xs;
            alignment
        };
        let mut junctions = Junctions::default();
        junctions.add(&spliced(0, None));
        junctions.add(&spliced(0, Some(Strand::Reverse)));
        junctions.add(&spliced(5, Some(Strand::Forward)));
        let mut minimap = read(
            300,
            vec![
                (Op::Match, 5),
                (Op::Skip, 50),
                (Op::Match, 5),
                (Op::Skip, 20),
                (Op::Match, 5),
            ],
            &[b'A'; 15],
        );
        minimap.flags = REVERSE;
        minimap.tags.ts = Some(Strand::Forward);
        junctions.add(&minimap);
        junctions.add(&read(500, vec![(Op::Match, 10)], &[b'A'; 10]));
        let junction = |start, end, count, strand| Junction {
            span: Region::between(start, end),
            count,
            strand,
        };
        assert_eq!(
            junctions.finish(),
            [
                junction(10, 110, 2, Strand::Reverse),
                junction(15, 115, 1, Strand::Forward),
                junction(305, 355, 1, Strand::Reverse),
                junction(360, 380, 1, Strand::Reverse),
            ]
        );
    }

    #[test]
    fn splice_motifs_give_the_strand() {
        let mut reference = vec![b'A'; 80];
        for (at, text) in [
            (5, &b"gt"[..]),
            (13, b"ag"),
            (20, b"CT"),
            (28, b"AC"),
            (35, b"GC"),
            (43, b"AG"),
            (50, b"GT"),
            (58, b"AT"),
            (65, b"GG"),
            (73, b"CC"),
        ] {
            reference[at..at + 2].copy_from_slice(text);
        }
        let strand = |start, end| motif_strand(&reference, 100, Region::between(start, end));
        assert_eq!(strand(105, 115), Strand::Forward);
        assert_eq!(strand(120, 130), Strand::Reverse);
        assert_eq!(strand(135, 145), Strand::Forward);
        assert_eq!(strand(150, 160), Strand::Reverse);
        assert_eq!(strand(165, 175), Strand::Unknown);
        assert_eq!(strand(170, 190), Strand::Unknown);
        assert_eq!(strand(90, 110), Strand::Unknown);
    }

    #[test]
    fn sorting_puts_mismatches_first_then_deletions_then_reference() {
        let reference = [b'A'; 100];
        let read = |start, cigar, bases: &[u8]| {
            Read::new(self::read(start, cigar, bases), &reference, 0, false)
        };
        let reads = [
            read(10, vec![(Op::Match, 10)], b"AAAAAAAAAA"),
            read(10, vec![(Op::Match, 10)], b"AAAAAGAAAA"),
            read(
                10,
                vec![(Op::Match, 4), (Op::Deletion, 2), (Op::Match, 4)],
                b"AAAAAAAA",
            ),
            read(30, vec![(Op::Match, 10)], b"AAAAAAAAAA"),
            read(12, vec![(Op::Match, 6)], b"AAACAA"),
            read(40, vec![(Op::Match, 10)], b"AAAAAAAAAA"),
        ];
        let rows = [0, 1, 2, 3, 4, 3];
        assert_eq!(sort_rows(&reads, &rows, &[], 15), [3, 1, 2, 4, 0, 4]);
        assert_eq!(sort_rows(&reads, &rows, &[], 90), rows);
        assert_eq!(sort_rows(&reads, &rows, &[0, 2], 15), [1, 0, 3, 4, 2, 4]);
    }

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    fn read(start: u64, cigar: Vec<(Op, u32)>, bases: &[u8]) -> Alignment {
        Alignment {
            name: format!("r{start}"),
            start,
            flags: 0,
            mapq: 60,
            cigar,
            bases: bases.to_vec(),
            qualities: Vec::new(),
            mate: None,
            tags: crate::alignment::Tags::default(),
        }
    }

    #[test]
    fn counts_aligned_bases_inside_the_region() {
        let mut coverage = Coverage::new(region(100, 110));
        coverage.add(&read(98, vec![(Op::Match, 4)], b"ACGT"));
        coverage.add(&read(
            100,
            vec![(Op::Match, 2), (Op::Deletion, 2), (Op::Match, 2)],
            b"AAGG",
        ));
        coverage.add(&read(104, vec![(Op::SoftClip, 1), (Op::Match, 2)], b"TCC"));
        assert_eq!(coverage.depth(100), 2);
        assert_eq!(coverage.counts(100), Some([1, 0, 1, 0, 0]));
        assert_eq!(coverage.depth(102), 0);
        assert_eq!(coverage.counts(104), Some([0, 1, 1, 0, 0]));
        assert_eq!(coverage.counts(110), None);
        assert_eq!(coverage.depth(99), 0);
    }

    #[test]
    fn reads_reach_over_clips_only_when_shown() {
        let cigar = vec![(Op::SoftClip, 2), (Op::Match, 3), (Op::SoftClip, 1)];
        let alignment = read(100, cigar, b"ACGTAC");
        let shown = Read::new(alignment.clone(), b"", 0, true);
        assert_eq!(shown.extent, region(98, 104));
        assert_eq!(shown.parts.clips.len(), 3);
        let hidden = Read::new(alignment, b"", 0, false);
        assert_eq!(hidden.extent, region(100, 103));
        assert_eq!(hidden.parts.clips.len(), 0);
    }

    #[test]
    fn reads_without_bases_count_as_unknown() {
        let mut coverage = Coverage::new(region(0, 4));
        coverage.add(&read(0, vec![(Op::Match, 2)], b""));
        assert_eq!(coverage.counts(1), Some([0, 0, 0, 0, 1]));
    }

    fn downsample(reads: Vec<Alignment>) -> Vec<Alignment> {
        let mut sampler = Sampler::new(50, 100);
        for read in reads {
            sampler.push(read);
        }
        sampler.finish()
    }

    #[test]
    fn downsampling_keeps_windows_under_the_limit() {
        let reads: Vec<Alignment> = (0..5)
            .map(|index| read(index * 10, vec![(Op::Match, 10)], b""))
            .collect();
        assert_eq!(downsample(reads.clone()), reads);
    }

    #[test]
    fn downsampling_caps_each_window_and_keeps_order() {
        let reads: Vec<Alignment> = (0..300)
            .map(|index| read(index / 3, vec![(Op::Match, 10)], b""))
            .collect();
        let kept = downsample(reads.clone());
        assert_eq!(kept.len(), 200);
        assert!(kept.windows(2).all(|pair| pair[0].start <= pair[1].start));
        assert_eq!(kept.iter().filter(|read| read.start < 50).count(), 100);
        assert_eq!(downsample(reads), kept);
    }

    #[test]
    fn downsampling_holds_one_sample_for_each_window() {
        let mut sampler = Sampler::new(50, 100);
        for index in 0..10_000 {
            sampler.push(read(index / 100, vec![(Op::Match, 10)], b""));
            assert!(sampler.held() <= 200, "{}", sampler.held());
        }
        assert_eq!(sampler.finish().len(), 200);
    }
}
