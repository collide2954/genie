//! Tandem repeats: runs of copies of a short motif, found in the reference or read from a
//! catalog, and the length of a repeat in each read that spans it, in copies of its motif, as
//! TRGT gives repeat lengths. The reads of a repeat make one or two alleles.

use std::cmp::Ordering;

use crate::Region;
use crate::alignment::{Alignment, Op};

/// The fewest copies of the motif of a tandem repeat in the reference.
pub const LEAST_COPIES: usize = 4;
/// The fewest bases of a tandem repeat in the reference.
pub const LEAST_BASES: usize = 12;
/// The longest motif of a tandem repeat that genie finds in the reference.
pub const MOST_MOTIF: usize = 6;
/// The bases that a read needs on each side of a repeat to measure it.
pub const FLANK: u64 = 20;

/// A tandem repeat: its bases on the reference and its motif, in upper case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repeat {
    pub span: Region,
    pub motif: String,
}

impl Repeat {
    /// The copies of the motif in the reference.
    pub fn copies(&self) -> f64 {
        self.span.len() as f64 / self.motif.len().max(1) as f64
    }
}

fn upper(base: u8) -> u8 {
    base.to_ascii_uppercase()
}

/// Whether a motif is not itself copies of a shorter motif, as AT is and ATAT is not.
fn primitive(motif: &[u8]) -> bool {
    (1..motif.len())
        .filter(|&size| motif.len().is_multiple_of(size))
        .all(|size| (size..motif.len()).any(|at| motif[at] != motif[at - size]))
}

/// The perfect tandem repeats of a sequence that starts at `start`: runs of `LEAST_COPIES`
/// copies or more of a motif of 1 to `MOST_MOTIF` bases, and `LEAST_BASES` bases or more. Each
/// repeat has its shortest motif, as the run starts. Lower-case bases count as upper-case bases,
/// and N ends a run.
pub fn tandem(bases: &[u8], start: u64) -> Vec<Repeat> {
    let mut found = Vec::new();
    for period in 1..=MOST_MOTIF {
        let mut at = 0;
        while at + period < bases.len() {
            let same = |index: usize| {
                let (a, b) = (upper(bases[index]), upper(bases[index + period]));
                a == b && a != b'N'
            };
            if !same(at) {
                at += 1;
                continue;
            }
            let first = at;
            while at + period < bases.len() && same(at) {
                at += 1;
            }
            let length = at - first + period;
            let motif: Vec<u8> = bases[first..first + period]
                .iter()
                .map(|&base| upper(base))
                .collect();
            if length >= LEAST_BASES.max(LEAST_COPIES * period) && primitive(&motif) {
                found.push(Repeat {
                    span: Region::between(start + first as u64, start + (first + length) as u64),
                    motif: String::from_utf8_lossy(&motif).into_owned(),
                });
            }
        }
    }
    found.sort_by_key(|repeat| (repeat.span.start(), repeat.span.end(), repeat.motif.len()));
    found
}

/// The place in the bases of a read of the base on a reference position, or of the next base
/// after it when the read has a deletion there.
fn query_at(read: &Alignment, position: u64) -> Option<usize> {
    let (mut reference, mut query) = (read.start, 0_usize);
    for &(op, length) in &read.cigar {
        let size = u64::from(length);
        let consumes_query = matches!(
            op,
            Op::Match | Op::Equal | Op::Diff | Op::Insertion | Op::SoftClip
        );
        if op.consumes_reference() {
            if position < reference + size {
                let inside = if consumes_query {
                    usize::try_from(position - reference).ok()?
                } else {
                    0
                };
                return Some(query + inside);
            }
            reference += size;
        }
        if consumes_query {
            query += length as usize;
        }
    }
    None
}

/// The length of a repeat in a read: its bases between the two flanks, and the copies of the
/// motif in them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Length {
    pub bases: u64,
    pub copies: u32,
}

/// The copies of a motif in bases: a copy is a run of the length of the motif that differs from
/// a rotation of the motif in at most one base, or in none for a motif of 1 or 2 bases. After a
/// part that is not a copy, the count goes on one base later.
pub fn copies(bases: &[u8], motif: &[u8]) -> u32 {
    let size = motif.len();
    if size == 0 {
        return 0;
    }
    let motif: Vec<u8> = motif.iter().map(|&base| upper(base)).collect();
    let rotations: Vec<Vec<u8>> = (0..size)
        .map(|at| [&motif[at..], &motif[..at]].concat())
        .collect();
    let allowed = usize::from(size >= 3);
    let (mut count, mut at) = (0, 0);
    while at + size <= bases.len() {
        let unit = &bases[at..at + size];
        let fewest = rotations
            .iter()
            .map(|rotation| {
                let pairs = rotation.iter().zip(unit);
                pairs.filter(|(a, b)| **a != upper(**b)).count()
            })
            .min()
            .unwrap_or(size);
        if fewest <= allowed {
            count += 1;
            at += size;
        } else {
            at += 1;
        }
    }
    count
}

/// The length of a repeat in a read that spans it with `FLANK` bases or more on each side: the
/// bases of the read between the two flanks, from the alignment, and the copies of the motif in
/// them.
pub fn measure(read: &Alignment, span: Region, motif: &[u8]) -> Option<Length> {
    let (from, to) = (span.start().checked_sub(FLANK)?, span.end() + FLANK);
    if read.start > from || read.end() < to {
        return None;
    }
    let first = query_at(read, from)? + FLANK as usize;
    let last = query_at(read, to - 1)?.checked_sub(FLANK as usize - 1)?;
    let middle = read.bases.get(first..last.max(first))?;
    Some(Length {
        bases: middle.len() as u64,
        copies: copies(middle, motif),
    })
}

/// An allele of a repeat in reads: the middle copies of its reads, the fewest and the most, and
/// its reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allele {
    pub copies: u32,
    pub least: u32,
    pub most: u32,
    pub reads: usize,
}

fn middle(sorted: &[u32]) -> u32 {
    sorted[(sorted.len() - 1) / 2]
}

fn allele(sorted: &[u32]) -> Allele {
    Allele {
        copies: middle(sorted),
        least: sorted[0],
        most: sorted[sorted.len() - 1],
        reads: sorted.len(),
    }
}

/// The alleles of the copies of a repeat in reads, the shorter first, and the allele of each
/// read. Two alleles when one split of the sorted copies leaves the least sum of squares, each
/// side has 2 reads and a tenth of the reads or more, and their middle copies differ by 2 copies
/// and a twentieth or more; else one.
pub fn alleles(copies: &[u32]) -> (Vec<Allele>, Vec<usize>) {
    if copies.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut sorted = copies.to_vec();
    sorted.sort_unstable();
    let squares = |part: &[u32]| {
        let mean = part.iter().map(|&value| f64::from(value)).sum::<f64>() / part.len() as f64;
        part.iter()
            .map(|&value| (f64::from(value) - mean).powi(2))
            .sum::<f64>()
    };
    let least = 2.max(sorted.len().div_ceil(10));
    let split = (least..=sorted.len().saturating_sub(least))
        .filter(|&at| at > 0 && at < sorted.len())
        .min_by(|&a, &b| {
            let cost = |at: usize| squares(&sorted[..at]) + squares(&sorted[at..]);
            cost(a).partial_cmp(&cost(b)).unwrap_or(Ordering::Equal)
        })
        .filter(|&at| {
            let (low, high) = (middle(&sorted[..at]), middle(&sorted[at..]));
            high - low >= 2.max(high.div_ceil(20))
        });
    let Some(at) = split else {
        return (vec![allele(&sorted)], vec![0; copies.len()]);
    };
    let (low, high) = (allele(&sorted[..at]), allele(&sorted[at..]));
    let boundary = sorted[at];
    let of = copies
        .iter()
        .map(|&value| usize::from(value >= boundary))
        .collect();
    (vec![low, high], of)
}

/// A line of bars for the reads of each copy number from `least` to `most`, the highest bar for
/// the most reads, and a dot for none.
pub fn histogram(copies: &[u32], least: u32, most: u32) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let counts: Vec<usize> = (least..=most)
        .map(|value| copies.iter().filter(|&&known| known == value).count())
        .collect();
    let top = counts.iter().copied().max().unwrap_or(0).max(1);
    counts
        .iter()
        .map(|&count| {
            if count == 0 {
                '·'
            } else {
                BARS[(count * BARS.len()).div_ceil(top).clamp(1, BARS.len()) - 1]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Allele, Length, Repeat, alleles, copies, histogram, measure, tandem};
    use crate::Region;
    use crate::alignment::{Alignment, Op, Tags};

    fn region(start: u64, end: u64) -> Region {
        Region::between(start, end)
    }

    /// Whole runs of 4 copies or more and 12 bases or more, with their shortest motif as the run
    /// starts: ATAT is AT. Short runs and runs through N do not count, and lower-case bases do.
    #[test]
    fn the_reference_has_perfect_tandem_repeats() {
        let bases = b"GGCAGCAGCAGCAGTTatatatatatatGGAAAAAAAAAAAAACCNACNACNACNACNT";
        let found = tandem(bases, 100);
        assert_eq!(
            found,
            [
                Repeat {
                    span: region(101, 114),
                    motif: "GCA".to_owned(),
                },
                Repeat {
                    span: region(115, 128),
                    motif: "TA".to_owned(),
                },
                Repeat {
                    span: region(130, 143),
                    motif: "A".to_owned(),
                },
            ]
        );
        assert!((found[0].copies() - 13.0 / 3.0).abs() < 1e-9);
        assert_eq!(tandem(b"CAGCAGCAG", 0), []);
        assert_eq!(tandem(b"", 0), []);
    }

    /// Copies of a rotation of the motif count, with one base that differs for a motif of 3
    /// bases or more. An extra base or a part of another motif does not stop the count.
    #[test]
    fn reads_count_the_copies_of_the_motif() {
        assert_eq!(copies(b"CAGCAGCAGCAG", b"CAG"), 4);
        assert_eq!(copies(b"AGCAGCAGCAGC", b"CAG"), 4);
        assert_eq!(copies(b"CAGCAACAGCAG", b"CAG"), 4);
        assert_eq!(copies(b"CAGCAGTCAGCAG", b"CAG"), 4);
        assert_eq!(copies(b"cagcag", b"CAG"), 2);
        assert_eq!(copies(b"ATATGTAT", b"AT"), 3);
        assert_eq!(copies(b"", b"CAG"), 0);
    }

    /// A read with 20 flanking bases on each side of a repeat of 4 CAG, and 2 more CAG as an
    /// insertion in the middle of the repeat.
    fn spanning(extra: u32, start: u64) -> Alignment {
        let left = b"TTGACTTGACTTGACTTGAC";
        let right = b"GTTCAGTTCAGTTCAGTTCA";
        let mut bases = left.to_vec();
        bases.extend(b"CAGCAG");
        bases.extend(b"CAG".repeat(extra as usize));
        bases.extend(b"CAGCAG");
        bases.extend(right);
        let cigar = if extra == 0 {
            vec![(Op::Match, 52)]
        } else {
            vec![(Op::Match, 26), (Op::Insertion, 3 * extra), (Op::Match, 26)]
        };
        Alignment {
            name: String::new(),
            start,
            flags: 0,
            mapq: 60,
            cigar,
            qualities: vec![30; bases.len()],
            bases,
            mate: None,
            tags: Tags::default(),
        }
    }

    #[test]
    fn spanning_reads_measure_the_repeat_between_its_flanks() {
        let span = region(120, 132);
        assert_eq!(
            measure(&spanning(0, 100), span, b"CAG"),
            Some(Length {
                bases: 12,
                copies: 4,
            })
        );
        assert_eq!(
            measure(&spanning(2, 100), span, b"CAG"),
            Some(Length {
                bases: 18,
                copies: 6,
            })
        );
        assert_eq!(measure(&spanning(0, 101), span, b"CAG"), None);
        assert_eq!(measure(&spanning(0, 100), region(10, 20), b"CAG"), None);
    }

    #[test]
    fn reads_make_one_or_two_alleles() {
        let two = [20, 21, 20, 42, 41, 43, 20, 42];
        let (found, of) = alleles(&two);
        assert_eq!(
            found,
            [
                Allele {
                    copies: 20,
                    least: 20,
                    most: 21,
                    reads: 4,
                },
                Allele {
                    copies: 42,
                    least: 41,
                    most: 43,
                    reads: 4,
                },
            ]
        );
        assert_eq!(of, [0, 0, 0, 1, 1, 1, 0, 1]);
        let one = [20, 21, 20, 21, 22];
        assert_eq!(alleles(&one).0.len(), 1);
        assert_eq!(alleles(&one).1, [0; 5]);
        let lone = [20, 20, 20, 20, 20, 20, 50];
        assert_eq!(alleles(&lone).0.len(), 1, "one read is not an allele");
        assert_eq!(alleles(&[]).0, []);
    }

    #[test]
    fn histograms_draw_a_bar_for_each_copy_number() {
        assert_eq!(histogram(&[20, 20, 21, 23], 20, 23), "█▄·▄");
        assert_eq!(histogram(&[5], 5, 5), "█");
    }
}
