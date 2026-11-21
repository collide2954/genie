//! Splicing differences of two groups of reads. The PSI (percent spliced in) of a splice junction
//! is its share of the reads of all junctions that share its start or its end, as `LeafCutter`
//! and MAJIQ group junctions. The difference is the PSI of the first group minus the PSI of the
//! second group, in percentage points, and Fisher's exact test of the reads of the junction
//! against the reads of the other junctions of its sites gives p.

use std::collections::{BTreeMap, HashMap};

use crate::Region;
use crate::isoform::Chain;
use crate::methylation::fisher;

/// The largest p of a junction that differs.
pub const MOST_P: f64 = 0.05;
/// The least difference of a junction that differs, in percentage points.
pub const LEAST_POINTS: f64 = 10.0;

/// A junction in two groups: its reads, the reads of the junctions that share its start or its
/// end with its own reads, and the p of Fisher's exact test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Difference {
    pub span: Region,
    pub reads: [u32; 2],
    pub totals: [u32; 2],
    pub p: f64,
}

impl Difference {
    /// The PSI of the junction in a group (0 or 1) in percent, if the group has reads at its
    /// sites.
    pub fn psi(&self, group: usize) -> Option<f64> {
        let total = *self.totals.get(group)?;
        (total > 0).then(|| f64::from(self.reads[group]) * 100.0 / f64::from(total))
    }

    /// The PSI of the first group minus the PSI of the second group, in percentage points.
    pub fn points(&self) -> Option<f64> {
        Some(self.psi(0)? - self.psi(1)?)
    }

    /// True when p is less than `MOST_P` and the difference is `LEAST_POINTS` or more.
    pub fn differs(&self) -> bool {
        self.p < MOST_P
            && self
                .points()
                .is_some_and(|points| points.abs() >= LEAST_POINTS)
    }
}

/// The differences of the junctions of two groups, each a list of junctions with their reads, in
/// the order of the junctions on the contig.
pub fn differences(first: &[(Region, u32)], second: &[(Region, u32)]) -> Vec<Difference> {
    let mut reads: BTreeMap<(u64, u64), [u32; 2]> = BTreeMap::new();
    for (group, junctions) in [first, second].into_iter().enumerate() {
        for &(span, count) in junctions {
            reads.entry((span.start(), span.end())).or_default()[group] += count;
        }
    }
    let mut starts: HashMap<u64, [u32; 2]> = HashMap::new();
    let mut ends: HashMap<u64, [u32; 2]> = HashMap::new();
    for (&(start, end), counts) in &reads {
        let add = |sums: &mut [u32; 2]| {
            sums[0] += counts[0];
            sums[1] += counts[1];
        };
        add(starts.entry(start).or_default());
        add(ends.entry(end).or_default());
    }
    reads
        .into_iter()
        .map(|((start, end), counts)| {
            // Another junction cannot share both the start and the end, so only the junction
            // itself is in both sums.
            let total = |group: usize| starts[&start][group] + ends[&end][group] - counts[group];
            let totals = [total(0), total(1)];
            Difference {
                span: Region::between(start, end),
                reads: counts,
                totals,
                p: fisher([
                    [counts[0], totals[0] - counts[0]],
                    [counts[1], totals[1] - counts[1]],
                ]),
            }
        })
        .collect()
}

/// The junctions of the reads of a group in chains of junctions, with their reads.
pub fn group_reads<G: PartialEq>(chains: &[Chain<G>], group: &G) -> Vec<(Region, u32)> {
    let mut found: BTreeMap<(u64, u64), u32> = BTreeMap::new();
    for chain in chains {
        let reads = chain
            .reads
            .iter()
            .filter(|read| read.group == *group)
            .count();
        if reads == 0 {
            continue;
        }
        let reads = u32::try_from(reads).unwrap_or(u32::MAX);
        for junction in &chain.junctions {
            *found.entry((junction.start(), junction.end())).or_default() += reads;
        }
    }
    found
        .into_iter()
        .map(|((start, end), reads)| (Region::between(start, end), reads))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Difference, differences, group_reads};
    use crate::isoform::{Chain, SplicedRead};
    use crate::methylation::fisher;
    use crate::{Region, Strand};

    fn region(start: u64, end: u64) -> Region {
        Region::between(start, end)
    }

    fn near(value: f64, known: f64) -> bool {
        (value - known).abs() < 1e-9
    }

    /// A skipped exon: the skip junction shares its start with the first inclusion junction and
    /// its end with the second one. A junction of one group alone has no difference.
    #[test]
    fn junctions_share_the_reads_of_their_sites() {
        let (first, second, skip) = (region(100, 200), region(300, 400), region(100, 400));
        let alone = region(500, 600);
        let a = [(first, 30), (second, 30), (skip, 10), (alone, 8)];
        let b = [(skip, 40), (first, 5), (second, 5)];
        let found = differences(&a, &b);
        let spans: Vec<Region> = found.iter().map(|difference| difference.span).collect();
        assert_eq!(spans, [first, skip, second, alone]);
        let skipped = found[1];
        assert_eq!(skipped.reads, [10, 40]);
        assert_eq!(skipped.totals, [70, 50]);
        assert!(near(skipped.psi(0).unwrap(), 100.0 / 7.0));
        assert!(near(skipped.psi(1).unwrap(), 80.0));
        assert!(near(skipped.points().unwrap(), 100.0 / 7.0 - 80.0));
        assert!(near(skipped.p, fisher([[10, 60], [40, 10]])));
        assert!(skipped.differs());
        assert_eq!(found[0].totals, [40, 45]);
        assert_eq!(found[2].totals, [40, 45]);
        assert!(found[0].points().unwrap() > 60.0);
        let lone = found[3];
        assert_eq!((lone.reads, lone.totals), ([8, 0], [8, 0]));
        assert_eq!(lone.psi(1), None);
        assert_eq!(lone.points(), None);
        assert!(!lone.differs());
    }

    /// A junction differs with p under 0.05 and 10 points or more.
    #[test]
    fn small_or_unsure_changes_do_not_differ() {
        let change = |reads: [u32; 2], totals: [u32; 2], p: f64| Difference {
            span: region(0, 10),
            reads,
            totals,
            p,
        };
        assert!(change([50, 40], [100, 100], 0.001).differs());
        assert!(!change([50, 45], [100, 100], 0.001).differs());
        assert!(!change([50, 40], [100, 100], 0.05).differs());
        assert!(change([40, 50], [100, 100], 0.01).differs());
    }

    #[test]
    fn groups_count_the_reads_of_each_junction_in_chains() {
        let read = |group: u8| SplicedRead {
            span: region(0, 1_000),
            group,
        };
        let chains = [
            Chain {
                junctions: vec![region(100, 200), region(300, 400)],
                reads: vec![read(1), read(1), read(2)],
                strand: Strand::Forward,
            },
            Chain {
                junctions: vec![region(100, 400)],
                reads: vec![read(2), read(1)],
                strand: Strand::Forward,
            },
        ];
        assert_eq!(
            group_reads(&chains, &1),
            [
                (region(100, 200), 2),
                (region(100, 400), 1),
                (region(300, 400), 2)
            ]
        );
        assert_eq!(
            group_reads(&chains, &2),
            [
                (region(100, 200), 1),
                (region(100, 400), 1),
                (region(300, 400), 1)
            ]
        );
        assert_eq!(group_reads(&chains, &3), []);
    }
}
