//! In-silico PCR: the products of a pair of primers on both strands of a sequence.

use std::fmt;

use crate::search::{self, Found, Hit, Pattern, PatternError, Reach, Scanned};
use crate::{Region, Strand};

/// The longest product, as the default of UCSC In-Silico PCR.
pub const LONGEST: u64 = 4_000;
/// The fewest bases of a primer.
pub const SHORTEST: usize = 12;
/// The bases at the 3′ end of a primer that must match, because the polymerase extends from there.
const CLAMP: u32 = 5;
/// The most matches of one primer that genie pairs.
const MATCHES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimerError {
    /// Both primers are empty.
    Empty,
    /// The forward primer (true) or the reverse primer (false) is empty.
    Missing(bool),
    /// The forward primer (true) or the reverse primer (false) is too short.
    Short(bool),
    /// The forward primer (true) or the reverse primer (false) is not valid.
    Wrong(bool, PatternError),
}

fn primer(forward: bool) -> &'static str {
    if forward { "forward" } else { "reverse" }
}

impl fmt::Display for PrimerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Type the forward primer and the reverse primer."),
            Self::Missing(forward) => write!(f, "Type the {} primer too.", primer(*forward)),
            Self::Short(forward) => write!(
                f,
                "Type {SHORTEST} bases or more for the {} primer.",
                primer(*forward)
            ),
            Self::Wrong(true, error) => write!(f, "Forward primer: {error}"),
            Self::Wrong(false, error) => write!(f, "Reverse primer: {error}"),
        }
    }
}

/// A forward primer and a reverse primer, each from its 5′ end to its 3′ end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Primers {
    pub forward: Pattern,
    pub reverse: Pattern,
}

impl Primers {
    /// The primers, when each one has enough bases for the mismatches.
    pub fn parse(forward: &str, reverse: &str, mismatches: u8) -> Result<Self, PrimerError> {
        let one = |text: &str, which: bool| match Pattern::parse(text) {
            Err(PatternError::Empty) => Ok(None),
            Err(error) => Err(PrimerError::Wrong(which, error)),
            Ok(pattern) if pattern.len() < SHORTEST => Err(PrimerError::Short(which)),
            Ok(pattern) => pattern
                .check(mismatches)
                .map(|()| Some(pattern))
                .map_err(|error| PrimerError::Wrong(which, error)),
        };
        match (one(forward, true)?, one(reverse, false)?) {
            (Some(forward), Some(reverse)) => Ok(Self { forward, reverse }),
            (None, None) => Err(PrimerError::Empty),
            (Some(_), None) => Err(PrimerError::Missing(false)),
            (None, Some(_)) => Err(PrimerError::Missing(true)),
        }
    }

    /// The lengths of the primers at the left end and at the right end of a product on a strand.
    pub fn ends(&self, strand: Strand) -> (u32, u32) {
        let (forward, reverse) = (self.forward.len() as u32, self.reverse.len() as u32);
        if strand == Strand::Reverse {
            (reverse, forward)
        } else {
            (forward, reverse)
        }
    }
}

/// The matches of a primer whose 3′ end matches. The 3′ end is at the right of a match on the
/// forward strand, and at the left of a match on the reverse strand.
fn extending(found: Found, length: u32) -> (Vec<Hit>, bool) {
    let clamp = CLAMP.min(length);
    let hits = found
        .hits
        .into_iter()
        .filter(|hit| {
            hit.mismatches.iter().all(|&offset| {
                let right = offset < length - clamp;
                let left = offset >= clamp;
                match hit.strand {
                    Strand::Forward => right,
                    Strand::Reverse => left,
                    Strand::Unknown => left && right,
                }
            })
        })
        .collect();
    (hits, found.complete)
}

/// The matches of a primer on a strand, with the matches of a palindrome on both strands.
fn side(hits: &[Hit], strand: Strand) -> Vec<&Hit> {
    hits.iter()
        .filter(|hit| hit.strand == strand || hit.strand == Strand::Unknown)
        .collect()
}

/// The products between a primer that reads forward at their left end and a primer that reads
/// back at their right end.
fn pairs(lefts: &[&Hit], rights: &[&Hit], strand: Strand, products: &mut Vec<Hit>) {
    for left in lefts {
        let start = left.span.start();
        let first = rights.partition_point(|right| right.span.start() < start);
        for right in rights[first..]
            .iter()
            .take_while(|right| right.span.start() < start + LONGEST)
        {
            let end = right.span.end();
            if end < left.span.end() || end - start > LONGEST {
                continue;
            }
            let shift = (right.span.start() - start) as u32;
            let mut mismatches = left.mismatches.clone();
            mismatches.extend(right.mismatches.iter().map(|offset| shift + offset));
            products.push(Hit {
                span: Region::between(start, end),
                strand,
                mismatches,
            });
        }
    }
}

/// The products of the primers in the bases from `start`, as hits over each whole product. The
/// strand of a product is forward when the forward primer is at its left end. The mismatches are
/// the wrong bases of the primers, as offsets in the product.
pub fn products(
    primers: &Primers,
    bases: &[u8],
    start: u64,
    mismatches: u8,
    limit: usize,
) -> Found {
    let find = |pattern: &Pattern| {
        let found = search::find(pattern, bases, start, mismatches, MATCHES);
        extending(found, pattern.len() as u32)
    };
    let (forward, forward_complete) = find(&primers.forward);
    let (reverse, reverse_complete) = find(&primers.reverse);
    let mut found = Vec::new();
    let (on, back) = (Strand::Forward, Strand::Reverse);
    pairs(&side(&forward, on), &side(&reverse, back), on, &mut found);
    pairs(&side(&reverse, on), &side(&forward, back), back, &mut found);
    found.sort_by_key(|hit| (hit.span.start(), hit.span.end()));
    found.dedup_by_key(|hit| hit.span);
    let complete = forward_complete && reverse_complete && found.len() <= limit;
    found.truncate(limit);
    Found {
        hits: found,
        complete,
    }
}

/// Reads the bases chunk by chunk from `reach.from` and gives the nearest product: forward, the
/// first one that starts at `from` or after; backward, the last one that starts before `from`.
/// Each chunk reads up to `LONGEST` bases more, so that a product that starts in it ends in it.
pub fn scan<E>(
    primers: &Primers,
    mismatches: u8,
    reach: Reach,
    mut fetch: impl FnMut(Region) -> Result<Vec<u8>, E>,
) -> Result<Scanned, E> {
    let chunk = reach.chunk.max(1);
    let mut at = reach.from.min(reach.length);
    let mut look = |from: u64, to: u64| -> Result<Vec<Hit>, E> {
        let window = Region::between(from, (to + LONGEST).min(reach.length));
        let bases = fetch(window)?;
        let found = products(primers, &bases, from, mismatches, usize::MAX);
        let starting = |hit: &Hit| from <= hit.span.start() && hit.span.start() < to;
        Ok(found.hits.into_iter().filter(starting).collect())
    };
    if reach.forward {
        let stop = reach.from.saturating_add(reach.budget).min(reach.length);
        while at < stop {
            let end = (at + chunk).min(stop);
            if let Some(hit) = look(at, end)?.into_iter().next() {
                return Ok(Scanned {
                    reached: hit.span.start(),
                    hit: Some(hit),
                });
            }
            at = end;
        }
        return Ok(Scanned {
            hit: None,
            reached: stop.max(at),
        });
    }
    let stop = reach.from.saturating_sub(reach.budget);
    while at > stop {
        let begin = at.saturating_sub(chunk).max(stop);
        if let Some(hit) = look(begin, at)?.into_iter().last() {
            return Ok(Scanned {
                reached: hit.span.start(),
                hit: Some(hit),
            });
        }
        at = begin;
    }
    Ok(Scanned {
        hit: None,
        reached: at,
    })
}

#[cfg(test)]
mod tests {
    use super::{LONGEST, PrimerError, Primers, products, scan};
    use crate::search::{Hit, PatternError, Reach};
    use crate::{Region, Strand};

    const FORWARD: &str = "ACGTTGCAGGTC";
    const REVERSE: &str = "TTGACCGATGCA";
    /// The reverse complements of the primers.
    const FORWARD_BACK: &str = "GACCTGCAACGT";
    const REVERSE_BACK: &str = "TGCATCGGTCAA";

    fn primers() -> Primers {
        Primers::parse(FORWARD, REVERSE, 0).unwrap()
    }

    fn put(bases: &mut [u8], at: usize, text: &str) {
        bases[at..at + text.len()].copy_from_slice(text.as_bytes());
    }

    /// A product on the forward strand at 10-102 and one on the reverse strand at 120-182.
    fn sequence() -> Vec<u8> {
        let mut bases = vec![b'A'; 200];
        put(&mut bases, 10, FORWARD);
        put(&mut bases, 90, REVERSE_BACK);
        put(&mut bases, 120, REVERSE);
        put(&mut bases, 170, FORWARD_BACK);
        bases
    }

    fn product(start: u64, end: u64, strand: Strand, mismatches: &[u32]) -> Hit {
        Hit {
            span: Region::between(start, end),
            strand,
            mismatches: mismatches.to_vec(),
        }
    }

    #[test]
    fn primers_need_both_ends_and_twelve_bases_each() {
        assert_eq!(Primers::parse(" ", "", 0), Err(PrimerError::Empty));
        assert_eq!(
            Primers::parse(FORWARD, "", 0),
            Err(PrimerError::Missing(false))
        );
        assert_eq!(
            Primers::parse("", REVERSE, 0),
            Err(PrimerError::Missing(true))
        );
        assert_eq!(
            Primers::parse("ACGTACGT", REVERSE, 0),
            Err(PrimerError::Short(true))
        );
        let wrong = Primers::parse(FORWARD, "TTGACCGATGCX", 0);
        assert_eq!(
            wrong,
            Err(PrimerError::Wrong(false, PatternError::Letter('X')))
        );
        let primers = Primers::parse("5'-acgttgcaggtc-3'", REVERSE, 3).unwrap();
        assert_eq!(primers.forward.text(), FORWARD);
        let texts: Vec<String> = [
            PrimerError::Missing(false),
            PrimerError::Missing(true),
            PrimerError::Short(true),
            PrimerError::Wrong(false, PatternError::Letter('X')),
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(
            texts,
            [
                "Type the reverse primer too.",
                "Type the forward primer too.",
                "Type 12 bases or more for the forward primer.",
                "Reverse primer: X is not a base. Use A, C, G, T or IUPAC codes such as N, R and Y.",
            ]
        );
    }

    #[test]
    fn products_come_from_both_strands() {
        let found = products(&primers(), &sequence(), 1_000, 0, 100);
        assert!(found.complete);
        assert_eq!(
            found.hits,
            [
                product(1_010, 1_102, Strand::Forward, &[]),
                product(1_120, 1_182, Strand::Reverse, &[]),
            ]
        );
    }

    #[test]
    fn mismatches_must_spare_the_three_prime_ends() {
        let mut bases = sequence();
        bases[12] = b'T';
        assert_eq!(products(&primers(), &bases, 0, 0, 100).hits.len(), 1);
        let near = products(&primers(), &bases, 0, 1, 100).hits;
        assert_eq!(near[0], product(10, 102, Strand::Forward, &[2]));
        let mut bases = sequence();
        bases[20] = b'A';
        assert_eq!(products(&primers(), &bases, 0, 1, 100).hits.len(), 1);
        let mut bases = sequence();
        bases[99] = b'A';
        let near = products(&primers(), &bases, 0, 1, 100).hits;
        assert_eq!(near[0], product(10, 102, Strand::Forward, &[89]));
        let mut bases = sequence();
        bases[91] = b'A';
        assert_eq!(products(&primers(), &bases, 0, 1, 100).hits.len(), 1);
    }

    #[test]
    fn products_are_four_kilobases_at_most() {
        let mut bases = vec![b'A'; 10_000];
        put(&mut bases, 100, FORWARD);
        put(&mut bases, 100 + LONGEST as usize - 12, REVERSE_BACK);
        put(&mut bases, 5_000, FORWARD);
        put(&mut bases, 5_001 + LONGEST as usize - 12, REVERSE_BACK);
        let found = products(&primers(), &bases, 0, 0, 100);
        assert_eq!(
            found.hits,
            [product(100, 100 + LONGEST, Strand::Forward, &[])]
        );
        let limited = products(&primers(), &sequence(), 0, 0, 1);
        assert_eq!(limited.hits.len(), 1);
        assert!(!limited.complete);
    }

    #[test]
    fn scans_find_the_next_and_the_previous_product() {
        let mut bases = vec![b'A'; 10_000];
        put(&mut bases, 2_950, FORWARD);
        put(&mut bases, 3_100, REVERSE_BACK);
        put(&mut bases, 7_000, REVERSE);
        put(&mut bases, 7_200, FORWARD_BACK);
        let run = |from: u64, forward: bool| {
            let reach = Reach {
                from,
                forward,
                length: bases.len() as u64,
                budget: 10_000,
                chunk: 1_000,
            };
            let fetch = |region: Region| {
                Ok::<_, String>(bases[region.start() as usize..region.end() as usize].to_vec())
            };
            scan(&primers(), 0, reach, fetch).unwrap().hit
        };
        let first = product(2_950, 3_112, Strand::Forward, &[]);
        let second = product(7_000, 7_212, Strand::Reverse, &[]);
        assert_eq!(run(0, true), Some(first.clone()));
        assert_eq!(run(2_951, true), Some(second.clone()));
        assert_eq!(run(7_001, true), None);
        assert_eq!(run(10_000, false), Some(second));
        assert_eq!(run(7_000, false), Some(first));
        assert_eq!(run(2_950, false), None);
    }
}
