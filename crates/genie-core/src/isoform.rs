//! Isoforms of long RNA reads: the reads that share a chain of splice junctions.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::slice;

use crate::Region;
use crate::Strand;
use crate::alignment::{Alignment, Op};

/// A read with splice junctions: its first and its last base, and its group, for example its
/// haplotype.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplicedRead<G> {
    pub span: Region,
    pub group: G,
}

/// The reads of one exact chain of splice junctions: the junctions in their order, the reads,
/// and the strand of the splice sites if a read gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain<G = ()> {
    pub junctions: Vec<Region>,
    pub reads: Vec<SplicedRead<G>>,
    pub strand: Strand,
}

impl<G> Chain<G> {
    /// The part of the contig from the first base of the reads to the last.
    pub fn span(&self) -> Region {
        hull(&self.reads)
    }
}

fn hull<G>(reads: &[SplicedRead<G>]) -> Region {
    reads
        .iter()
        .map(|read| read.span)
        .reduce(Region::hull)
        .unwrap_or_else(|| Region::between(0, 0))
}

/// Counts the chains of splice junctions of reads, each read with its group, and keeps the first
/// and the last base of each read without a junction.
#[derive(Debug)]
pub struct Chains<G = ()> {
    found: HashMap<Vec<Region>, (Vec<SplicedRead<G>>, Strand)>,
    unspliced: Vec<Region>,
}

impl<G> Default for Chains<G> {
    fn default() -> Self {
        Self {
            found: HashMap::new(),
            unspliced: Vec::new(),
        }
    }
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

impl<G> Chains<G> {
    pub fn add(&mut self, alignment: &Alignment, group: G) {
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
        entry.0.push(SplicedRead {
            span: alignment.span(),
            group,
        });
        if entry.1 == Strand::Unknown {
            entry.1 = strand;
        }
    }

    /// The chains, the one with the most reads first, and the reads without a junction.
    pub fn finish(self) -> (Vec<Chain<G>>, Vec<Region>) {
        let mut chains: Vec<Chain<G>> = self
            .found
            .into_iter()
            .map(|(junctions, (reads, strand))| Chain {
                junctions,
                reads,
                strand,
            })
            .collect();
        let places = |chain: &Chain<G>| {
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
pub struct Isoform<G = ()> {
    pub junctions: Vec<Region>,
    pub reads: Vec<SplicedRead<G>>,
    pub span: Region,
    pub strand: Strand,
}

impl<G> Isoform<G> {
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
pub fn snap<G: Clone>(chains: &[Chain<G>], introns: &[Region], window: u64) -> Vec<Chain<G>> {
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
pub fn isoforms<G: Clone>(
    chains: &[Chain<G>],
    slack: u64,
    least: usize,
) -> (Vec<Isoform<G>>, (usize, usize)) {
    let mut ordered: Vec<&Chain<G>> = chains.iter().collect();
    ordered.sort_by_key(|chain| Reverse(chain.reads.len()));
    let mut joined: Vec<Chain<G>> = Vec::new();
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
    let mut kept: Vec<Chain<G>> = Vec::new();
    for mut part in joined {
        let count = part.junctions.len();
        let runs: Vec<Option<usize>> = kept
            .iter()
            .map(|isoform| run(&isoform.junctions, &part.junctions, slack))
            .collect();
        let choices: Vec<Vec<usize>> = part
            .reads
            .iter()
            .map(|read| {
                (0..kept.len())
                    .filter(|&index| {
                        runs[index].is_some_and(|at| {
                            fits(&kept[index].junctions, (at, count), read.span, slack)
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
    let mut found: Vec<Isoform<G>> = kept
        .into_iter()
        .map(|isoform| Isoform {
            span: hull(&isoform.reads),
            reads: isoform.reads,
            junctions: isoform.junctions,
            strand: isoform.strand,
        })
        .collect();
    found.sort_by_key(|isoform| Reverse(isoform.reads.len()));
    let (kept, few): (Vec<Isoform<G>>, Vec<Isoform<G>>) = found
        .into_iter()
        .partition(|isoform| isoform.reads.len() >= least);
    let others = (
        few.len(),
        few.iter().map(|isoform| isoform.reads.len()).sum(),
    );
    (kept, others)
}

/// How the junctions of an isoform compare with the introns of known transcripts, as the
/// structural classes of SQANTI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// The junctions agree with all the introns of a transcript: a full splice match.
    Known,
    /// The junctions agree with a run of introns next to each other of a transcript: an
    /// incomplete splice match.
    Part,
    /// Each start and each end of the junctions is a start or an end of a known intron, but no
    /// transcript has this chain: novel in catalog.
    Combination,
    /// A start or an end of a junction is not a start or an end of a known intron: novel not in
    /// catalog.
    Junction,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Known => "known",
            Self::Part => "part of a known",
            Self::Combination => "new combination",
            Self::Junction => "new junction",
        }
    }
}

/// The kind of an isoform with these junctions for the introns of each known transcript, when
/// the ends agree within `slack` bases.
pub fn kind(junctions: &[Region], transcripts: &[Vec<Region>], slack: u64) -> Kind {
    if transcripts
        .iter()
        .any(|introns| agree(introns, junctions, slack))
    {
        return Kind::Known;
    }
    if transcripts
        .iter()
        .any(|introns| run(introns, junctions, slack).is_some())
    {
        return Kind::Part;
    }
    let introns = || transcripts.iter().flatten();
    let known = |junction: &Region| {
        introns().any(|intron| intron.start().abs_diff(junction.start()) <= slack)
            && introns().any(|intron| intron.end().abs_diff(junction.end()) <= slack)
    };
    if junctions.iter().all(known) {
        Kind::Combination
    } else {
        Kind::Junction
    }
}

/// The number of junctions that agree within `slack` bases with an intron of a transcript.
pub fn shared(introns: &[Region], junctions: &[Region], slack: u64) -> usize {
    junctions
        .iter()
        .filter(|junction| {
            introns
                .iter()
                .any(|intron| agree(slice::from_ref(intron), slice::from_ref(junction), slack))
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::{Chain, Chains, Isoform, Kind, SplicedRead, agree, isoforms, junctions, kind};
    use super::{shared, snap};
    use crate::alignment::{Alignment, Op, Tags};
    use crate::{Region, Strand};

    fn ungrouped(spans: Vec<Region>) -> Vec<SplicedRead<()>> {
        spans
            .into_iter()
            .map(|span| SplicedRead { span, group: () })
            .collect()
    }

    /// The junctions, the number of reads and the span of each isoform.
    fn summary<G>(found: &[Isoform<G>]) -> Vec<(Vec<Region>, usize, Region)> {
        found
            .iter()
            .map(|isoform| (isoform.junctions.clone(), isoform.reads.len(), isoform.span))
            .collect()
    }

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
        chains.add(&read(1_000, &spliced), ());
        chains.add(&read(990, &spliced[..]), ());
        let mut other = read(1_010, &[(Op::Match, 40), (Op::Skip, 100), (Op::Match, 50)]);
        other.tags.ts = Some(Strand::Forward);
        chains.add(&other, ());
        chains.add(&read(1_000, &[(Op::Match, 200)]), ());
        let (found, unspliced) = chains.finish();
        assert_eq!(unspliced, [region(1_000, 1_200)]);
        assert_eq!(
            found,
            [
                Chain {
                    junctions: vec![region(1_050, 1_150)],
                    reads: ungrouped(vec![region(1_000, 1_200), region(1_010, 1_200)]),
                    strand: Strand::Forward,
                },
                Chain {
                    junctions: vec![region(1_040, 1_140)],
                    reads: ungrouped(vec![region(990, 1_190)]),
                    strand: Strand::Unknown,
                },
            ]
        );
        assert_eq!(found[0].span(), region(1_000, 1_200));
    }

    fn chain(junctions: Vec<Region>, reads: Vec<Region>) -> Chain {
        Chain {
            junctions,
            reads: ungrouped(reads),
            strand: Strand::Unknown,
        }
    }

    #[test]
    fn reads_keep_their_groups_in_their_isoforms() {
        let grouped = |junctions: Vec<Region>, reads: &[(Region, u8)]| Chain {
            junctions,
            reads: reads
                .iter()
                .map(|&(span, group)| SplicedRead { span, group })
                .collect(),
            strand: Strand::Unknown,
        };
        let chains = [
            grouped(
                vec![region(100, 200), region(300, 400)],
                &[(region(50, 450), 1), (region(50, 450), 2)],
            ),
            grouped(
                vec![region(102, 200), region(300, 400)],
                &[(region(60, 450), 2)],
            ),
            grouped(vec![region(300, 400)], &[(region(250, 450), 1)]),
        ];
        let (found, _) = isoforms(&chains, 5, 1);
        assert_eq!(found.len(), 1, "the chains join and the part folds in");
        let mut groups: Vec<u8> = found[0].reads.iter().map(|read| read.group).collect();
        groups.sort_unstable();
        assert_eq!(groups, [1, 1, 2, 2]);
    }

    #[test]
    fn kinds_compare_junctions_with_the_introns_of_transcripts() {
        let transcripts = [
            vec![region(100, 200), region(300, 400), region(500, 600)],
            vec![region(100, 250)],
        ];
        let of = |junctions: &[Region]| kind(junctions, &transcripts, 5);
        assert_eq!(
            of(&[region(100, 200), region(302, 400), region(500, 600)]),
            Kind::Known
        );
        assert_eq!(of(&[region(300, 400), region(500, 600)]), Kind::Part);
        assert_eq!(
            of(&[region(100, 400), region(500, 600)]),
            Kind::Combination,
            "a known start and a known end in a new junction"
        );
        assert_eq!(of(&[region(100, 250), region(300, 400)]), Kind::Combination);
        assert_eq!(of(&[region(100, 350)]), Kind::Junction);
        assert_eq!(Kind::Combination.label(), "new combination");
        assert_eq!(
            shared(&transcripts[0], &[region(100, 200), region(700, 800)], 5),
            1
        );
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
            summary(&found),
            [
                (
                    vec![region(100, 200), region(300, 400)],
                    13,
                    region(50, 500)
                ),
                (vec![region(100, 200)], 5, region(50, 350)),
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
            summary(&found),
            [
                (
                    vec![region(100, 200), region(300, 400), region(500, 600)],
                    6,
                    region(50, 700)
                ),
                (vec![region(100, 400), region(500, 600)], 2, region(50, 700)),
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
            .map(|isoform| (isoform.junctions.len(), isoform.reads.len()))
            .collect();
        assert_eq!(
            reads,
            [(1, 3), (2, 2)],
            "two reads go into the next intron of the longer isoform, so the shorter chain is an \
             isoform of its own, and it keeps the read that fits both"
        );
    }
}
