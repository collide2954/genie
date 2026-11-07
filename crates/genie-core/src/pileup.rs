use crate::Region;
use crate::alignment::{Alignment, Op, Parts};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    pub alignment: Alignment,
    pub parts: Parts,
}

impl Read {
    pub fn new(alignment: Alignment, reference: &[u8], offset: u64) -> Self {
        let parts = alignment.parts(reference, offset);
        Self { alignment, parts }
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

pub fn downsample(alignments: Vec<Alignment>, window: u64, limit: usize) -> Vec<Alignment> {
    let window = window.max(1);
    let mut kept = Vec::with_capacity(alignments.len());
    let mut group = Vec::new();
    let mut current = None;
    for alignment in alignments {
        let bucket = alignment.start / window;
        if current != Some(bucket) {
            sample(&mut group, &mut kept, limit, current.unwrap_or(0));
            current = Some(bucket);
        }
        group.push(alignment);
    }
    sample(&mut group, &mut kept, limit, current.unwrap_or(0));
    kept
}

fn sample(group: &mut Vec<Alignment>, kept: &mut Vec<Alignment>, limit: usize, seed: u64) {
    if group.len() <= limit {
        kept.append(group);
        return;
    }
    let mut chosen: Vec<usize> = (0..limit).collect();
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for index in limit..group.len() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let pick = (state % (index as u64 + 1)) as usize;
        if pick < limit {
            chosen[pick] = index;
        }
    }
    let mut keep = vec![false; group.len()];
    for index in chosen {
        keep[index] = true;
    }
    kept.extend(
        group
            .drain(..)
            .zip(keep)
            .filter_map(|(alignment, keep)| keep.then_some(alignment)),
    );
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

#[cfg(test)]
mod tests {
    use super::{Coverage, downsample};
    use crate::Region;
    use crate::alignment::{Alignment, Op};

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
    fn reads_without_bases_count_as_unknown() {
        let mut coverage = Coverage::new(region(0, 4));
        coverage.add(&read(0, vec![(Op::Match, 2)], b""));
        assert_eq!(coverage.counts(1), Some([0, 0, 0, 0, 1]));
    }

    #[test]
    fn downsampling_keeps_windows_under_the_limit() {
        let reads: Vec<Alignment> = (0..5)
            .map(|index| read(index * 10, vec![(Op::Match, 10)], b""))
            .collect();
        assert_eq!(downsample(reads.clone(), 50, 100), reads);
    }

    #[test]
    fn downsampling_caps_each_window_and_keeps_order() {
        let reads: Vec<Alignment> = (0..300)
            .map(|index| read(index / 3, vec![(Op::Match, 10)], b""))
            .collect();
        let kept = downsample(reads.clone(), 50, 100);
        assert_eq!(kept.len(), 200);
        assert!(kept.windows(2).all(|pair| pair[0].start <= pair[1].start));
        assert_eq!(kept.iter().filter(|read| read.start < 50).count(), 100);
        assert_eq!(downsample(reads, 50, 100), kept);
    }
}
