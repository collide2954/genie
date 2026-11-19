//! Isoforms of long RNA reads: the reads that share a chain of splice junctions.

use std::cmp::Reverse;
use std::collections::HashMap;

use crate::Region;
use crate::Strand;
use crate::alignment::{Alignment, Op};

/// The reads of one exact chain of splice junctions: the junctions in their order, the first and
/// the last base of each read, and the strand of the splice sites if a read gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub junctions: Vec<Region>,
    pub reads: Vec<Region>,
    pub strand: Strand,
}

impl Chain {
    /// The part of the contig from the first base of the reads to the last.
    pub fn span(&self) -> Region {
        hull(&self.reads)
    }
}

fn hull(reads: &[Region]) -> Region {
    reads
        .iter()
        .copied()
        .reduce(Region::hull)
        .unwrap_or_else(|| Region::between(0, 0))
}

/// Counts the chains of splice junctions of reads, and keeps the first and the last base of each
/// read without a junction.
#[derive(Debug, Default)]
pub struct Chains {
    found: HashMap<Vec<Region>, (Vec<Region>, Strand)>,
    unspliced: Vec<Region>,
}

/// The splice junctions of a read, in their order on the contig.
pub fn junctions(alignment: &Alignment) -> Vec<Region> {
    let mut found = Vec::new();
    let mut position = alignment.start;
    for &(op, length) in &alignment.cigar {
        let length = u64::from(length);
        if op == Op::Skip {
            found.push(Region::between(position, position + length));
        }
        if op.consumes_reference() {
            position += length;
        }
    }
    found
}

impl Chains {
    pub fn add(&mut self, alignment: &Alignment) {
        let junctions = junctions(alignment);
        if junctions.is_empty() {
            self.unspliced.push(alignment.span());
            return;
        }
        let strand = alignment.splice_strand();
        let entry = self
            .found
            .entry(junctions)
            .or_insert((Vec::new(), Strand::Unknown));
        entry.0.push(alignment.span());
        if entry.1 == Strand::Unknown {
            entry.1 = strand;
        }
    }

    /// The chains, the one with the most reads first, and the reads without a junction.
    pub fn finish(self) -> (Vec<Chain>, Vec<Region>) {
        let mut chains: Vec<Chain> = self
            .found
            .into_iter()
            .map(|(junctions, (reads, strand))| Chain {
                junctions,
                reads,
                strand,
            })
            .collect();
        let places = |chain: &Chain| {
            chain
                .junctions
                .iter()
                .map(|junction| (junction.start(), junction.end()))
                .collect::<Vec<_>>()
        };
        chains.sort_by(|a, b| {
            b.reads
                .len()
                .cmp(&a.reads.len())
                .then_with(|| places(a).cmp(&places(b)))
        });
        (chains, self.unspliced)
    }
}

/// An isoform: the junctions of its most common chain, its reads, the part of the contig from
/// the first base of its reads to the last, and its strand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Isoform {
    pub junctions: Vec<Region>,
    pub reads: usize,
    pub span: Region,
    pub strand: Strand,
}

impl Isoform {
    /// The exons of the isoform: the parts of its span between its junctions.
    pub fn exons(&self) -> Vec<Region> {
        let mut found = Vec::new();
        let mut from = self.span.start();
        for junction in &self.junctions {
            if junction.start() > from {
                found.push(Region::between(from, junction.start()));
            }
            from = from.max(junction.end());
        }
        if self.span.end() > from {
            found.push(Region::between(from, self.span.end()));
        }
        found
    }
}

/// True when two chains of junctions have the same number of junctions, and each start and end
/// agree within `slack` bases.
pub fn agree(first: &[Region], second: &[Region], slack: u64) -> bool {
    first.len() == second.len()
        && first.iter().zip(second).all(|(one, other)| {
            one.start().abs_diff(other.start()) <= slack && one.end().abs_diff(other.end()) <= slack
        })
}

/// Moves each end of each junction to the nearest end of the same kind of an annotated intron
/// within `window` bases, because an aligner can put a splice site of a noisy read some bases off.
/// A junction keeps its ends when the moved start is not before the moved end.
pub fn snap(chains: &[Chain], introns: &[Region], window: u64) -> Vec<Chain> {
    let mut starts: Vec<u64> = introns.iter().map(|intron| intron.start()).collect();
    let mut ends: Vec<u64> = introns.iter().map(|intron| intron.end()).collect();
    for places in [&mut starts, &mut ends] {
        places.sort_unstable();
        places.dedup();
    }
    chains
        .iter()
        .map(|chain| Chain {
            junctions: chain
                .junctions
                .iter()
                .map(|&junction| {
                    let start = nearest(&starts, junction.start(), window);
                    let end = nearest(&ends, junction.end(), window);
                    if start < end {
                        Region::between(start, end)
                    } else {
                        junction
                    }
                })
                .collect(),
            reads: chain.reads.clone(),
            strand: chain.strand,
        })
        .collect()
}

/// The place of a sorted list nearest to `place`, when it is at most `window` bases away, or
/// else `place`.
fn nearest(sorted: &[u64], place: u64, window: u64) -> u64 {
    let at = sorted.partition_point(|&known| known < place);
    [at.checked_sub(1), Some(at)]
        .into_iter()
        .flatten()
        .filter_map(|index| sorted.get(index).copied())
        .filter(|known| known.abs_diff(place) <= window)
        .min_by_key(|known| known.abs_diff(place))
        .unwrap_or(place)
}

/// Where a shorter chain of junctions starts in a longer chain, when it agrees within `slack`
/// bases with a run of junctions next to each other of the longer chain.
fn run(longer: &[Region], shorter: &[Region], slack: u64) -> Option<usize> {
    if shorter.is_empty() || shorter.len() >= longer.len() {
        return None;
    }
    (0..=longer.len() - shorter.len())
        .find(|&at| agree(&longer[at..at + shorter.len()], shorter, slack))
}

/// True when a read with `count` junctions that agree with the junctions of an isoform from
/// `at` stays in the exons at the two ends of that run: it does not start in the intron before
/// the run, and it does not end in the intron after it.
fn fits(junctions: &[Region], (at, count): (usize, usize), read: Region, slack: u64) -> bool {
    let starts = at == 0 || read.start() + slack >= junctions[at - 1].end();
    let ends = at + count == junctions.len() || read.end() <= junctions[at + count].start() + slack;
    starts && ends
}

/// The isoforms of chains, the most reads first. Chains whose junctions agree within `slack`
/// bases join, and the chain with the most reads gives the junctions. Then a read of a shorter
/// chain that is a part of a longer isoform, as a read that starts after the first exon, counts
/// for the longer isoform: the one with the most reads, if more than one fits. A shorter chain
/// with reads that fit no longer isoform is an isoform of its own, and it keeps a read that fits
/// both when it has as many such reads as the longer isoform has reads. Isoforms with fewer than
/// `least` reads leave the list, and the second value counts them and their reads.
pub fn isoforms(chains: &[Chain], slack: u64, least: usize) -> (Vec<Isoform>, (usize, usize)) {
    let mut ordered: Vec<&Chain> = chains.iter().collect();
    ordered.sort_by_key(|chain| Reverse(chain.reads.len()));
    let mut joined: Vec<Chain> = Vec::new();
    for chain in ordered {
        match joined
            .iter_mut()
            .find(|isoform| agree(&isoform.junctions, &chain.junctions, slack))
        {
            Some(isoform) => {
                isoform.reads.extend_from_slice(&chain.reads);
                if isoform.strand == Strand::Unknown {
                    isoform.strand = chain.strand;
                }
            }
            None => joined.push(chain.clone()),
        }
    }
    joined.sort_by_key(|isoform| {
        (
            Reverse(isoform.junctions.len()),
            Reverse(isoform.reads.len()),
        )
    });
    let mut kept: Vec<Chain> = Vec::new();
    for mut part in joined {
        let count = part.junctions.len();
        let runs: Vec<Option<usize>> = kept
            .iter()
            .map(|isoform| run(&isoform.junctions, &part.junctions, slack))
            .collect();
        let choices: Vec<Vec<usize>> = part
            .reads
            .iter()
            .map(|&read| {
                (0..kept.len())
                    .filter(|&index| {
                        runs[index].is_some_and(|at| {
                            fits(&kept[index].junctions, (at, count), read, slack)
                        })
                    })
                    .collect()
            })
            .collect();
        let own = choices.iter().filter(|choice| choice.is_empty()).count();
        let mut left = Vec::new();
        for (read, choice) in part.reads.drain(..).zip(choices) {
            let target = choice
                .into_iter()
                .max_by_key(|&index| (kept[index].reads.len(), Reverse(index)))
                .filter(|&index| own == 0 || kept[index].reads.len() > own);
            match target {
                Some(index) => kept[index].reads.push(read),
                None => left.push(read),
            }
        }
        if !left.is_empty() {
            part.reads = left;
            kept.push(part);
        }
    }
    let mut found: Vec<Isoform> = kept
        .into_iter()
        .map(|isoform| Isoform {
            reads: isoform.reads.len(),
            span: hull(&isoform.reads),
            junctions: isoform.junctions,
            strand: isoform.strand,
        })
        .collect();
    found.sort_by_key(|isoform| Reverse(isoform.reads));
    let (kept, few): (Vec<Isoform>, Vec<Isoform>) = found
        .into_iter()
        .partition(|isoform| isoform.reads >= least);
    let others = (few.len(), few.iter().map(|isoform| isoform.reads).sum());
    (kept, others)
}

#[cfg(test)]
mod tests {
    use super::{Chain, Chains, Isoform, agree, isoforms, junctions, snap};
    use crate::alignment::{Alignment, Op, Tags};
    use crate::{Region, Strand};

    fn read(start: u64, cigar: &[(Op, u32)]) -> Alignment {
        Alignment {
            name: "r".to_owned(),
            start,
            flags: 0,
            mapq: 60,
            cigar: cigar.to_vec(),
            bases: Vec::new(),
            qualities: Vec::new(),
            mate: None,
            tags: Tags::default(),
        }
    }

    fn region(start: u64, end: u64) -> Region {
        Region::between(start, end)
    }

    #[test]
    fn reads_count_by_their_exact_chains_of_junctions() {
        let spliced = [(Op::Match, 50), (Op::Skip, 100), (Op::Match, 50)];
        assert_eq!(junctions(&read(1_000, &spliced)), [region(1_050, 1_150)]);
        let mut chains = Chains::default();
        chains.add(&read(1_000, &spliced));
        chains.add(&read(990, &spliced[..]));
        let mut other = read(1_010, &[(Op::Match, 40), (Op::Skip, 100), (Op::Match, 50)]);
        other.tags.ts = Some(Strand::Forward);
        chains.add(&other);
        chains.add(&read(1_000, &[(Op::Match, 200)]));
        let (found, unspliced) = chains.finish();
        assert_eq!(unspliced, [region(1_000, 1_200)]);
        assert_eq!(
            found,
            [
                Chain {
                    junctions: vec![region(1_050, 1_150)],
                    reads: vec![region(1_000, 1_200), region(1_010, 1_200)],
                    strand: Strand::Forward,
                },
                Chain {
                    junctions: vec![region(1_040, 1_140)],
                    reads: vec![region(990, 1_190)],
                    strand: Strand::Unknown,
                },
            ]
        );
        assert_eq!(found[0].span(), region(1_000, 1_200));
    }

    fn chain(junctions: Vec<Region>, reads: Vec<Region>) -> Chain {
        Chain {
            junctions,
            reads,
            strand: Strand::Unknown,
        }
    }

    #[test]
    fn chains_that_agree_within_the_slack_join() {
        let chains = [
            chain(
                vec![region(100, 200), region(300, 400)],
                vec![region(50, 450); 10],
            ),
            chain(
                vec![region(103, 200), region(300, 396)],
                vec![region(60, 500); 3],
            ),
            chain(vec![region(100, 200)], vec![region(50, 350); 5]),
            chain(
                vec![region(150, 200), region(300, 400)],
                vec![region(50, 450)],
            ),
        ];
        assert!(agree(&chains[0].junctions, &chains[1].junctions, 5));
        assert!(!agree(&chains[0].junctions, &chains[1].junctions, 2));
        let (found, others) = isoforms(&chains, 5, 2);
        assert_eq!(
            found,
            [
                Isoform {
                    junctions: vec![region(100, 200), region(300, 400)],
                    reads: 13,
                    span: region(50, 500),
                    strand: Strand::Unknown,
                },
                Isoform {
                    junctions: vec![region(100, 200)],
                    reads: 5,
                    span: region(50, 350),
                    strand: Strand::Unknown,
                },
            ],
            "reads that go into the next intron do not fold into the longer isoform"
        );
        assert_eq!(others, (1, 1));
        assert_eq!(
            found[0].exons(),
            [region(50, 100), region(200, 300), region(400, 500)]
        );
    }

    #[test]
    fn partial_reads_fold_into_the_longer_isoform_they_fit() {
        let chains = [
            chain(
                vec![region(100, 200), region(300, 400), region(500, 600)],
                vec![region(50, 700); 3],
            ),
            chain(
                vec![region(100, 400), region(500, 600)],
                vec![region(50, 700); 2],
            ),
            chain(
                vec![region(302, 400), region(500, 604)],
                vec![region(250, 650), region(198, 700)],
            ),
            chain(
                vec![region(500, 600)],
                vec![region(450, 680), region(350, 650)],
            ),
        ];
        let (found, others) = isoforms(&chains, 5, 2);
        assert_eq!(
            found,
            [
                Isoform {
                    junctions: vec![region(100, 200), region(300, 400), region(500, 600)],
                    reads: 6,
                    span: region(50, 700),
                    strand: Strand::Unknown,
                },
                Isoform {
                    junctions: vec![region(100, 400), region(500, 600)],
                    reads: 2,
                    span: region(50, 700),
                    strand: Strand::Unknown,
                },
            ],
            "a part fits the isoform with the most reads, and a read that starts in an intron \
             of the isoform does not fit"
        );
        assert_eq!(others, (1, 1));
    }

    #[test]
    fn junctions_snap_to_the_nearest_annotated_intron_ends() {
        let introns = [region(100, 200), region(300, 400), region(310, 420)];
        let chains = [
            chain(
                vec![region(107, 200), region(300, 418)],
                vec![region(50, 450)],
            ),
            chain(vec![region(125, 180)], vec![region(50, 250)]),
        ];
        let snapped = snap(&chains, &introns, 20);
        assert_eq!(
            snapped[0].junctions,
            [region(100, 200), region(300, 420)],
            "each end takes the nearest annotated end within 20 bases"
        );
        assert_eq!(
            snapped[1].junctions,
            [region(125, 200)],
            "an end more than 20 bases away stays"
        );
        assert_eq!(snapped[0].reads, chains[0].reads);
        let tiny = snap(
            &[chain(vec![region(5, 8)], vec![region(0, 20)])],
            &[region(9, 30)],
            20,
        );
        assert_eq!(
            tiny[0].junctions,
            [region(5, 8)],
            "a junction keeps its ends when the snapped start is not before the snapped end"
        );
    }

    #[test]
    fn a_chain_with_reads_of_its_own_keeps_the_reads_that_fit_both() {
        let chains = [
            chain(
                vec![region(100, 200), region(300, 400)],
                vec![region(50, 450); 2],
            ),
            chain(
                vec![region(100, 200)],
                vec![region(50, 350), region(50, 360), region(60, 300)],
            ),
        ];
        let (found, _) = isoforms(&chains, 5, 2);
        let reads: Vec<(usize, usize)> = found
            .iter()
            .map(|isoform| (isoform.junctions.len(), isoform.reads))
            .collect();
        assert_eq!(
            reads,
            [(1, 3), (2, 2)],
            "two reads go into the next intron of the longer isoform, so the shorter chain is an \
             isoform of its own, and it keeps the read that fits both"
        );
    }
}
