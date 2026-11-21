//! Epialleles: groups of reads with the same pattern of methylation at the CpGs that they cover.
//! genie finds them by k-modes on the calls of the reads: the distance of a read to the center of
//! a group is the share of its calls that differ from the center, at the CpGs that the two share.
//! The first centers are reads, picked the same way each time, so the groups do not jump.

use std::cmp::Reverse;
use std::collections::BTreeSet;

use crate::alignment::{Alignment, REVERSE};
use crate::modification::{self, MODIFIED_FROM};

/// The fewest calls at CpGs of a read with an epiallele.
pub const LEAST_CPGS: usize = 3;
/// The most rounds of k-modes.
const ROUNDS: usize = 20;

/// The calls of a modification of a read at CpGs: the place of the C of each CpG, and whether
/// the read has the modification there. A call of the reverse strand on the G of a CpG moves to
/// its C. Calls off CpGs do not count.
pub fn cpg_calls(
    read: &Alignment,
    name: &str,
    base: impl Fn(u64) -> Option<u8>,
) -> Vec<(u64, bool)> {
    if read.tags.mods.calls.is_empty() {
        return Vec::new();
    }
    let upper = |position: u64| base(position).map(|letter| letter.to_ascii_uppercase());
    let reverse = read.has(REVERSE);
    let mut calls: Vec<(u64, bool)> = read
        .placed_calls()
        .into_iter()
        .filter_map(|placed| {
            let code = read.tags.mods.codes.get(usize::from(placed.code))?;
            if modification::name(&code.code) != name {
                return None;
            }
            let at = if reverse && code.base == b'C' {
                placed.position.checked_sub(1)?
            } else {
                placed.position
            };
            let cpg = upper(at) == Some(b'C') && upper(at + 1) == Some(b'G');
            cpg.then_some((at, placed.chance >= MODIFIED_FROM))
        })
        .collect();
    calls.sort_unstable();
    calls.dedup_by_key(|(position, _)| *position);
    calls
}

/// The epialleles of reads: the epiallele of each read, from 0 for the most methylated, or none
/// for a read with fewer than `LEAST_CPGS` calls; the reads of each epiallele; and the share of
/// the methylated calls of each epiallele, from 0 to 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Epialleles {
    pub of: Vec<Option<usize>>,
    pub reads: Vec<usize>,
    pub methylated: Vec<f64>,
}

/// The calls of a read at the places of the CpGs in a list of all CpGs.
type Calls = Vec<(usize, bool)>;

/// The share of the calls of a read that differ from a center, at the CpGs where the center has
/// a value, or nothing when they share no CpG.
fn distance(read: &[(usize, bool)], center: &[Option<bool>]) -> Option<f64> {
    let (mut shared, mut wrong) = (0_u32, 0_u32);
    for &(site, methylated) in read {
        if let Some(value) = center[site] {
            shared += 1;
            wrong += u32::from(value != methylated);
        }
    }
    (shared > 0).then(|| f64::from(wrong) / f64::from(shared))
}

/// The share of the calls of two reads that differ, at the CpGs that both have. Reads that share
/// fewer than `LEAST_CPGS` CpGs are 0 apart, so that a read far from the others is not a center.
fn apart(first: &[(usize, bool)], second: &[(usize, bool)]) -> f64 {
    let (mut shared, mut wrong) = (0_u32, 0_u32);
    let (mut one, mut two) = (first.iter().peekable(), second.iter().peekable());
    while let (Some(&&(a, x)), Some(&&(b, y))) = (one.peek(), two.peek()) {
        match a.cmp(&b) {
            std::cmp::Ordering::Less => {
                one.next();
            }
            std::cmp::Ordering::Greater => {
                two.next();
            }
            std::cmp::Ordering::Equal => {
                shared += 1;
                wrong += u32::from(x != y);
                one.next();
                two.next();
            }
        }
    }
    if (shared as usize) < LEAST_CPGS {
        0.0
    } else {
        f64::from(wrong) / f64::from(shared)
    }
}

/// The reads that start the centers: the read with the most calls, then each time the read
/// farthest from the centers that it shares CpGs with. Ties go to the read with more calls, then
/// to the first read.
fn seeds(reads: &[Calls], count: usize) -> Vec<usize> {
    let Some(first) = (0..reads.len()).max_by_key(|&at| (reads[at].len(), Reverse(at))) else {
        return Vec::new();
    };
    let mut seeds = vec![first];
    while seeds.len() < count {
        let best = (0..reads.len())
            .filter(|at| !seeds.contains(at))
            .map(|at| {
                let nearest = seeds
                    .iter()
                    .map(|&seed| apart(&reads[at], &reads[seed]))
                    .fold(f64::INFINITY, f64::min);
                (at, nearest)
            })
            .filter(|(_, nearest)| *nearest > 0.0)
            .max_by(|a, b| {
                (a.1.total_cmp(&b.1))
                    .then(reads[a.0].len().cmp(&reads[b.0].len()))
                    .then(b.0.cmp(&a.0))
            });
        match best {
            Some((at, _)) => seeds.push(at),
            None => break,
        }
    }
    seeds
}

/// The epialleles of the calls of reads, at most `count` of them.
pub fn epialleles(patterns: &[Vec<(u64, bool)>], count: usize) -> Epialleles {
    let usable: Vec<usize> = (0..patterns.len())
        .filter(|&at| patterns[at].len() >= LEAST_CPGS)
        .collect();
    let sites: Vec<u64> = usable
        .iter()
        .flat_map(|&at| patterns[at].iter().map(|(position, _)| *position))
        .collect::<BTreeSet<u64>>()
        .into_iter()
        .collect();
    let place = |position: u64| sites.binary_search(&position).unwrap_or(0);
    let reads: Vec<Calls> = usable
        .iter()
        .map(|&at| {
            let calls = patterns[at].iter();
            calls
                .map(|&(position, methylated)| (place(position), methylated))
                .collect()
        })
        .collect();
    let mut centers: Vec<Vec<Option<bool>>> = seeds(&reads, count.max(1))
        .into_iter()
        .map(|seed| {
            let mut center = vec![None; sites.len()];
            for &(site, methylated) in &reads[seed] {
                center[site] = Some(methylated);
            }
            center
        })
        .collect();
    let mut of: Vec<Option<usize>> = vec![None; reads.len()];
    for _ in 0..ROUNDS {
        let next: Vec<Option<usize>> = reads
            .iter()
            .map(|read| {
                let distances = centers.iter().enumerate();
                let near = distances.filter_map(|(at, center)| Some((at, distance(read, center)?)));
                near.min_by(|a, b| a.1.total_cmp(&b.1)).map(|(at, _)| at)
            })
            .collect();
        let changed = next != of;
        of = next;
        for (at, center) in centers.iter_mut().enumerate() {
            let mut counts = vec![(0_u32, 0_u32); sites.len()];
            let members = reads
                .iter()
                .zip(&of)
                .filter(|(_, owner)| **owner == Some(at));
            for (read, _) in members {
                for &(site, methylated) in read {
                    let entry = &mut counts[site];
                    if methylated {
                        entry.0 += 1;
                    } else {
                        entry.1 += 1;
                    }
                }
            }
            for (value, (methylated, other)) in center.iter_mut().zip(counts) {
                if methylated != other {
                    *value = Some(methylated > other);
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut groups: Vec<(usize, usize, f64)> = (0..centers.len())
        .filter_map(|at| {
            let members: Vec<&Calls> = reads
                .iter()
                .zip(&of)
                .filter(|(_, owner)| **owner == Some(at))
                .map(|(read, _)| read)
                .collect();
            let calls: usize = members.iter().map(|read| read.len()).sum();
            let methylated = members
                .iter()
                .flat_map(|read| read.iter())
                .filter(|(_, on)| *on);
            let share = methylated.count() as f64 / calls.max(1) as f64;
            (!members.is_empty()).then_some((at, members.len(), share))
        })
        .collect();
    groups.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
    let order = |center: usize| groups.iter().position(|(at, _, _)| *at == center);
    let mut found = vec![None; patterns.len()];
    for (&at, owner) in usable.iter().zip(&of) {
        found[at] = owner.and_then(order);
    }
    Epialleles {
        of: found,
        reads: groups.iter().map(|(_, reads, _)| *reads).collect(),
        methylated: groups.iter().map(|(_, _, share)| *share).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{cpg_calls, epialleles};
    use crate::alignment::{Alignment, Op, REVERSE, Tags};
    use crate::modification::{Call, Code, Modifications};

    const REFERENCE: &[u8] = b"ACGTCGAGG";

    fn base(position: u64) -> Option<u8> {
        REFERENCE
            .get(usize::try_from(position.checked_sub(100)?).ok()?)
            .copied()
    }

    fn read(flags: u16, calls: &[(u32, u8, u8)]) -> Alignment {
        Alignment {
            name: String::new(),
            start: 100,
            flags,
            mapq: 60,
            cigar: vec![(Op::Match, 6)],
            bases: b"ACGTCG".to_vec(),
            qualities: vec![30; 6],
            mate: None,
            tags: Tags {
                mods: Modifications {
                    codes: vec![
                        Code {
                            base: b'C',
                            code: "m".to_owned(),
                        },
                        Code {
                            base: b'C',
                            code: "h".to_owned(),
                        },
                    ],
                    calls: calls
                        .iter()
                        .map(|&(at, code, chance)| Call { at, code, chance })
                        .collect(),
                },
                ..Tags::default()
            },
        }
    }

    /// Calls of the reverse strand on the G of a CpG move to its C. Calls off CpGs and calls of
    /// another modification do not count.
    #[test]
    fn reads_give_their_calls_at_cpgs() {
        let forward = read(0, &[(1, 0, 200), (2, 0, 200), (4, 1, 255), (4, 0, 20)]);
        assert_eq!(
            cpg_calls(&forward, "5mC", base),
            [(101, true), (104, false)]
        );
        assert_eq!(cpg_calls(&forward, "5hmC", base), [(104, true)]);
        let reverse = read(REVERSE, &[(2, 0, 220), (5, 0, 10)]);
        assert_eq!(
            cpg_calls(&reverse, "5mC", base),
            [(101, true), (104, false)]
        );
        assert_eq!(cpg_calls(&read(0, &[]), "5mC", base), []);
    }

    /// The calls of a read at CpGs 0, 10, 20 and so on.
    fn pattern(calls: &[bool]) -> Vec<(u64, bool)> {
        (0_u64..).step_by(10).zip(calls.iter().copied()).collect()
    }

    /// Methylated and unmethylated reads make two epialleles, the methylated one first. A read
    /// with fewer than 3 calls gets none.
    #[test]
    fn methylated_and_unmethylated_reads_make_two_epialleles() {
        let mut patterns = vec![pattern(&[false; 4]); 5];
        patterns.extend(vec![pattern(&[true; 4]); 5]);
        patterns.push(pattern(&[true, true]));
        let found = epialleles(&patterns, 2);
        let mut of = vec![Some(1); 5];
        of.extend([Some(0); 5]);
        of.push(None);
        assert_eq!(found.of, of);
        assert_eq!(found.reads, [5, 5]);
        assert_eq!(found.methylated, [1.0, 0.0]);
    }

    /// Two patterns with the same mean methylation are two epialleles, and a read that covers a
    /// part of a pattern and one more CpG joins it.
    #[test]
    fn patterns_with_the_same_mean_stay_apart() {
        let mut patterns = vec![pattern(&[true, true, false, false]); 4];
        patterns.extend(vec![pattern(&[false, false, true, true]); 4]);
        patterns.push(vec![(10, false), (20, true), (30, true), (40, true)]);
        let found = epialleles(&patterns, 2);
        let mut of = vec![Some(1); 4];
        of.extend([Some(0); 5]);
        assert_eq!(found.of, of);
        assert_eq!(found.reads, [5, 4]);
        assert!((found.methylated[0] - 11.0 / 20.0).abs() < 1e-9);
        assert!((found.methylated[1] - 0.5).abs() < 1e-9);
    }

    /// With 3 epialleles, a third pattern gets its own, and the same reads give the same
    /// epialleles each time. Reads that are all the same make one epiallele.
    #[test]
    fn more_epialleles_split_more_patterns() {
        let (high, low) = (pattern(&[true; 4]), pattern(&[false; 4]));
        let mixed = pattern(&[true, false, true, false]);
        let patterns: Vec<Vec<(u64, bool)>> = (0..3)
            .flat_map(|_| [high.clone(), low.clone(), mixed.clone()])
            .collect();
        let found = epialleles(&patterns, 3);
        assert_eq!(found.reads, [3, 3, 3]);
        assert_eq!(found.methylated, [1.0, 0.5, 0.0]);
        assert_eq!(found.of[..3], [Some(0), Some(2), Some(1)]);
        assert_eq!(epialleles(&patterns, 3), found);
        let same = vec![high; 4];
        assert_eq!(epialleles(&same, 2).reads, [4]);
        assert_eq!(epialleles(&[], 2).reads, [] as [usize; 0]);
    }
}
