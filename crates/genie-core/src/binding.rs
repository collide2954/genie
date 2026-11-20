//! Where primers bind in long sequences, to check that a pair of primers makes only its own
//! product. A primer binds where the 8 bases at its 3′ end match, and its other bases have at
//! most 2 mismatches.

use crate::Region;
use crate::primer::reverse_complement;

/// The bases at the 3′ end of a primer that must match.
pub const SEED: usize = 8;
/// The most mismatches in the other bases of a primer.
pub const MISMATCHES: usize = 2;
/// The number of seeds of 8 bases.
const SEEDS: usize = 1 << (2 * SEED);

/// A place where a primer binds: the primer, its bases on the reference, and whether the
/// primer reads as the top strand, so that it extends to the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bind {
    pub primer: usize,
    pub span: Region,
    pub forward: bool,
}

/// The primers and their seeds: for each seed of 8 bases, the primers that bind with it and
/// in which direction.
#[derive(Debug, Clone)]
pub struct Binder {
    primers: Vec<Vec<u8>>,
    reversed: Vec<Vec<u8>>,
    seeds: Vec<Vec<(usize, bool)>>,
    longest: usize,
}

fn code(base: u8) -> Option<usize> {
    match base.to_ascii_uppercase() {
        b'A' => Some(0),
        b'C' => Some(1),
        b'G' => Some(2),
        b'T' => Some(3),
        _ => None,
    }
}

/// The number of a seed of 8 bases, or None when it has a base that is not A, C, G or T.
fn key(bases: &[u8]) -> Option<usize> {
    bases
        .iter()
        .try_fold(0, |key, base| Some((key << 2) | code(*base)?))
}

impl Binder {
    pub fn new(primers: &[&str]) -> Self {
        let primers: Vec<Vec<u8>> = primers
            .iter()
            .map(|primer| {
                primer
                    .bytes()
                    .map(|base| base.to_ascii_uppercase())
                    .collect()
            })
            .collect();
        let reversed: Vec<Vec<u8>> = primers
            .iter()
            .map(|primer| reverse_complement(primer))
            .collect();
        let mut seeds = vec![Vec::new(); SEEDS];
        for (index, primer) in primers.iter().enumerate() {
            if primer.len() < SEED {
                continue;
            }
            if let Some(found) = key(&primer[primer.len() - SEED..]) {
                seeds[found].push((index, true));
            }
            if let Some(found) = key(&reversed[index][..SEED]) {
                seeds[found].push((index, false));
            }
        }
        let longest = primers.iter().map(Vec::len).max().unwrap_or(0);
        Self {
            primers,
            reversed,
            seeds,
            longest,
        }
    }

    /// The bases that two parts of a long sequence must share, so that each place where a
    /// primer binds is whole in one part.
    pub fn overlap(&self) -> u64 {
        self.longest as u64
    }

    /// The places where the primers bind in a sequence that starts at `start`. A place counts
    /// only when its seed starts in `own`, so that two parts of a long sequence that overlap
    /// give each place once.
    pub fn scan(&self, bases: &[u8], start: u64, own: Region) -> Vec<Bind> {
        let mut found = Vec::new();
        let (mut current, mut known) = (0, 0);
        for (at, base) in bases.iter().enumerate() {
            let Some(value) = code(*base) else {
                known = 0;
                continue;
            };
            current = ((current << 2) | value) & (SEEDS - 1);
            known += 1;
            if known < SEED {
                continue;
            }
            let seed = at + 1 - SEED;
            if !own.contains(start + seed as u64) {
                continue;
            }
            for &(primer, forward) in &self.seeds[current] {
                let length = self.primers[primer].len();
                let (first, wanted, from) = if forward {
                    let Some(first) = seed.checked_sub(length - SEED) else {
                        continue;
                    };
                    (first, &self.primers[primer][..length - SEED], first)
                } else {
                    (seed, &self.reversed[primer][SEED..], seed + SEED)
                };
                let Some(part) = bases.get(from..from + wanted.len()) else {
                    continue;
                };
                let wrong = part
                    .iter()
                    .zip(wanted)
                    .filter(|(base, want)| base.to_ascii_uppercase() != **want)
                    .count();
                if wrong <= MISMATCHES {
                    let span =
                        Region::between(start + first as u64, start + (first + length) as u64);
                    found.push(Bind {
                        primer,
                        span,
                        forward,
                    });
                }
            }
        }
        found.sort_by_key(|bind| (bind.span.start(), bind.primer));
        found
    }
}

/// The products of a pair of primers among the places where they bind on one contig: from a
/// place of either primer that extends to the right to a place of either primer that extends
/// to the left, at most `longest` bases long.
pub fn products(binds: &[Bind], (one, other): (usize, usize), longest: u64) -> Vec<Region> {
    let ours = |bind: &&Bind| bind.primer == one || bind.primer == other;
    let lefts: Vec<&Bind> = binds
        .iter()
        .filter(ours)
        .filter(|bind| bind.forward)
        .collect();
    let mut rights: Vec<&Bind> = binds
        .iter()
        .filter(ours)
        .filter(|bind| !bind.forward)
        .collect();
    rights.sort_by_key(|bind| bind.span.end());
    let mut found = Vec::new();
    for left in lefts {
        let first = rights.partition_point(|right| right.span.end() <= left.span.start());
        for right in &rights[first..] {
            if right.span.end() - left.span.start() > longest {
                break;
            }
            if right.span.start() >= left.span.start() {
                found.push(Region::between(left.span.start(), right.span.end()));
            }
        }
    }
    found.sort_by_key(|span| (span.start(), span.end()));
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::{Bind, Binder, products};
    use crate::Region;
    use crate::primer::reverse_complement;

    const LEFT: &str = "AGCTGACTGACGTAGCTAGC";
    const RIGHT: &str = "TTGCAGGTCAACTGGATCCA";

    fn text(parts: &[&str]) -> Vec<u8> {
        parts.concat().into_bytes()
    }

    fn reversed(primer: &str) -> String {
        String::from_utf8(reverse_complement(primer.as_bytes())).unwrap()
    }

    #[test]
    fn primers_bind_on_both_strands() {
        let binder = Binder::new(&[LEFT, RIGHT]);
        assert_eq!(binder.overlap(), 20);
        let right = reversed(RIGHT);
        let bases = text(&["TTTTT", LEFT, &"T".repeat(100), &right, "TTTTT"]);
        let binds = binder.scan(&bases, 1_000, Region::between(0, 10_000));
        assert_eq!(
            binds,
            [
                Bind {
                    primer: 0,
                    span: Region::between(1_005, 1_025),
                    forward: true
                },
                Bind {
                    primer: 1,
                    span: Region::between(1_125, 1_145),
                    forward: false
                },
            ]
        );
        assert_eq!(
            products(&binds, (0, 1), 4_000),
            [Region::between(1_005, 1_145)]
        );
        assert_eq!(products(&binds, (0, 1), 100), []);
    }

    #[test]
    fn mismatches_count_only_away_from_the_three_prime_end() {
        let binder = Binder::new(&[LEFT]);
        let two = format!("TC{}", &LEFT[2..]);
        let three = format!("TCA{}", &LEFT[3..]);
        let seed = format!("{}T", &LEFT[..19]);
        for (bases, binds) in [(two, 1), (three, 0), (seed, 0)] {
            let found = binder.scan(bases.as_bytes(), 0, Region::between(0, 100));
            assert_eq!(found.len(), binds, "{bases}");
        }
        let unknown = format!("{}N{}", &LEFT[..15], &LEFT[16..]);
        assert_eq!(
            binder
                .scan(unknown.as_bytes(), 0, Region::between(0, 100))
                .len(),
            0
        );
        let lower = LEFT.to_lowercase();
        assert_eq!(
            binder
                .scan(lower.as_bytes(), 0, Region::between(0, 100))
                .len(),
            1
        );
    }

    #[test]
    fn parts_that_overlap_give_each_place_once() {
        let binder = Binder::new(&[LEFT]);
        let bases = text(&[&"A".repeat(50), LEFT, &"A".repeat(50)]);
        let first = binder.scan(&bases[..80], 0, Region::between(0, 60));
        let second = binder.scan(&bases[40..], 40, Region::between(60, 120));
        assert_eq!(first.len() + second.len(), 1);
    }

    #[test]
    fn one_primer_can_make_a_product_alone() {
        let binder = Binder::new(&[LEFT, RIGHT]);
        let left = reversed(LEFT);
        let bases = text(&[LEFT, &"G".repeat(60), &left]);
        let binds = binder.scan(&bases, 0, Region::between(0, 200));
        assert_eq!(binds.len(), 2);
        assert_eq!(products(&binds, (0, 1), 4_000), [Region::between(0, 100)]);
        assert_eq!(products(&binds, (1, 1), 4_000), []);
    }
}
