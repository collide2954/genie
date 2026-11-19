//! Isoforms of long RNA reads: the reads that share a chain of splice junctions.

use std::collections::HashMap;

use crate::Region;
use crate::Strand;
use crate::alignment::{Alignment, Op};

/// The reads of one exact chain of splice junctions: the junctions in their order, the reads,
/// the part of the contig from the first base of the reads to the last, and the strand of the
/// splice sites if a read gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub junctions: Vec<Region>,
    pub reads: usize,
    pub span: Region,
    pub strand: Strand,
}

/// Counts the chains of splice junctions of reads, and the reads without a junction.
#[derive(Debug, Default)]
pub struct Chains {
    found: HashMap<Vec<Region>, (usize, Region, Strand)>,
    unspliced: usize,
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
            self.unspliced += 1;
            return;
        }
        let span = alignment.span();
        let strand = alignment.splice_strand();
        let entry = self
            .found
            .entry(junctions)
            .or_insert((0, span, Strand::Unknown));
        entry.0 += 1;
        entry.1 = entry.1.hull(span);
        if entry.2 == Strand::Unknown {
            entry.2 = strand;
        }
    }

    /// The chains, the one with the most reads first, and the reads without a junction.
    pub fn finish(self) -> (Vec<Chain>, usize) {
        let mut chains: Vec<Chain> = self
            .found
            .into_iter()
            .map(|(junctions, (reads, span, strand))| Chain {
                junctions,
                reads,
                span,
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
                .cmp(&a.reads)
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

/// The isoforms of chains, the most reads first: chains whose junctions agree within `slack`
/// bases join, and the chain with the most reads gives the junctions. Isoforms with fewer than
/// `least` reads leave the list, and the second value counts them and their reads.
pub fn isoforms(chains: &[Chain], slack: u64, least: usize) -> (Vec<Isoform>, (usize, usize)) {
    let mut ordered: Vec<&Chain> = chains.iter().collect();
    ordered.sort_by_key(|chain| std::cmp::Reverse(chain.reads));
    let mut found: Vec<Isoform> = Vec::new();
    for chain in ordered {
        match found
            .iter_mut()
            .find(|isoform| agree(&isoform.junctions, &chain.junctions, slack))
        {
            Some(isoform) => {
                isoform.reads += chain.reads;
                isoform.span = isoform.span.hull(chain.span);
                if isoform.strand == Strand::Unknown {
                    isoform.strand = chain.strand;
                }
            }
            None => found.push(Isoform {
                junctions: chain.junctions.clone(),
                reads: chain.reads,
                span: chain.span,
                strand: chain.strand,
            }),
        }
    }
    found.sort_by_key(|isoform| std::cmp::Reverse(isoform.reads));
    let (kept, few): (Vec<Isoform>, Vec<Isoform>) = found
        .into_iter()
        .partition(|isoform| isoform.reads >= least);
    let others = (few.len(), few.iter().map(|isoform| isoform.reads).sum());
    (kept, others)
}

#[cfg(test)]
mod tests {
    use super::{Chain, Chains, Isoform, agree, isoforms, junctions};
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
        assert_eq!(unspliced, 1);
        assert_eq!(
            found,
            [
                Chain {
                    junctions: vec![region(1_050, 1_150)],
                    reads: 2,
                    span: region(1_000, 1_200),
                    strand: Strand::Forward,
                },
                Chain {
                    junctions: vec![region(1_040, 1_140)],
                    reads: 1,
                    span: region(990, 1_190),
                    strand: Strand::Unknown,
                },
            ]
        );
    }

    #[test]
    fn chains_that_agree_within_the_slack_join() {
        let chain = |junctions: Vec<Region>, reads, span| Chain {
            junctions,
            reads,
            span,
            strand: Strand::Unknown,
        };
        let chains = [
            chain(
                vec![region(100, 200), region(300, 400)],
                10,
                region(50, 450),
            ),
            chain(vec![region(103, 200), region(300, 396)], 3, region(60, 500)),
            chain(vec![region(100, 200)], 5, region(50, 250)),
            chain(vec![region(150, 200), region(300, 400)], 1, region(50, 450)),
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
                    span: region(50, 250),
                    strand: Strand::Unknown,
                },
            ]
        );
        assert_eq!(others, (1, 1));
        assert_eq!(
            found[0].exons(),
            [region(50, 100), region(200, 300), region(400, 500)]
        );
    }
}
