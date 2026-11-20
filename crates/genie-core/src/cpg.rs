//! CpG islands, found as the CpG Islands track of UCSC finds them: each CG dinucleotide scores
//! +17 and each other dinucleotide -1, the maximal scoring segments are the candidates, and an
//! island is a candidate of more than 200 bases with GC of 50 % or more and a ratio of observed
//! to expected CpG of more than 0.6.

use crate::Region;

const CPG_SCORE: i64 = 17;
const OTHER_SCORE: i64 = -1;
const SHORTEST: u64 = 201;
const LEAST_GC: f64 = 0.5;
const LEAST_RATIO: f64 = 0.6;

/// A CpG island: its bases, its CG dinucleotides, and its C and G bases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Island {
    pub span: Region,
    pub cpgs: u32,
    pub c: u32,
    pub g: u32,
}

impl Island {
    /// The share of C and G bases, in percent.
    pub fn gc_percent(&self) -> f64 {
        f64::from(self.c + self.g) / self.span.len() as f64 * 100.0
    }

    /// The share of the bases in CG dinucleotides, in percent.
    pub fn cpg_percent(&self) -> f64 {
        f64::from(2 * self.cpgs) / self.span.len() as f64 * 100.0
    }

    /// The observed CpG over the CpG that the C and G bases give, as Gardiner-Garden and
    /// Frommer (1987): CpG × length / (C × G).
    pub fn ratio(&self) -> f64 {
        let expected = f64::from(self.c) * f64::from(self.g);
        if expected == 0.0 {
            return 0.0;
        }
        f64::from(self.cpgs) * self.span.len() as f64 / expected
    }
}

/// The CpG islands of a sequence that starts at `start`. Lower-case bases count as the other
/// bases, as in the unmasked CpG islands of UCSC.
pub fn islands(bases: &[u8], start: u64) -> Vec<Island> {
    let upper: Vec<u8> = bases.iter().map(u8::to_ascii_uppercase).collect();
    let is_cpg = |at: usize| upper[at] == b'C' && upper[at + 1] == b'G';
    let pairs = upper.len().saturating_sub(1);
    let scores = (0..pairs).map(|at| if is_cpg(at) { CPG_SCORE } else { OTHER_SCORE });
    maximal(scores)
        .into_iter()
        .filter_map(|(first, last)| {
            let (c, g) = upper[first..last + 2]
                .iter()
                .fold((0, 0), |(c, g), base| match base {
                    b'C' => (c + 1, g),
                    b'G' => (c, g + 1),
                    _ => (c, g),
                });
            let island = Island {
                span: Region::between(start + first as u64, start + last as u64 + 2),
                cpgs: (first..=last).filter(|&at| is_cpg(at)).count() as u32,
                c,
                g,
            };
            let kept = island.span.len() >= SHORTEST
                && island.gc_percent() >= LEAST_GC * 100.0
                && island.ratio() > LEAST_RATIO;
            kept.then_some(island)
        })
        .collect()
}

/// A segment of scores while the search runs: its first and last places, the sum of the
/// scores before it and up to its end, and the place of the segment before it with a lower
/// sum before it.
struct Segment {
    first: usize,
    last: usize,
    before: i64,
    through: i64,
    lower: Option<usize>,
}

/// The maximal scoring segments of a list of scores, as first and last places, by the linear
/// method of Ruzzo and Tompa (1999).
fn maximal(scores: impl IntoIterator<Item = i64>) -> Vec<(usize, usize)> {
    let mut segments: Vec<Segment> = Vec::new();
    let mut total = 0;
    for (at, score) in scores.into_iter().enumerate() {
        let before = total;
        total += score;
        if score <= 0 {
            continue;
        }
        let mut segment = Segment {
            first: at,
            last: at,
            before,
            through: total,
            lower: None,
        };
        loop {
            let mut found = segments.len().checked_sub(1);
            while let Some(index) = found {
                if segments[index].before < segment.before {
                    break;
                }
                found = segments[index].lower;
            }
            match found {
                Some(index) if segments[index].through < segment.through => {
                    segment.first = segments[index].first;
                    segment.before = segments[index].before;
                    segments.truncate(index);
                }
                _ => {
                    segment.lower = found;
                    segments.push(segment);
                    break;
                }
            }
        }
    }
    segments
        .iter()
        .map(|segment| (segment.first, segment.last))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Island, islands, maximal};
    use crate::Region;

    fn text(parts: &[(&str, usize)]) -> Vec<u8> {
        parts
            .iter()
            .flat_map(|(part, count)| part.repeat(*count).into_bytes())
            .collect()
    }

    #[test]
    fn maximal_segments_keep_apart_unless_joining_scores_more() {
        assert_eq!(maximal([5, -4, 3]), [(0, 0), (2, 2)]);
        assert_eq!(maximal([5, -3, 5]), [(0, 2)]);
        assert_eq!(maximal([-1, 2, -5, 1, 1]), [(1, 1), (3, 4)]);
        assert_eq!(maximal([-1, -2]), []);
        assert_eq!(maximal([3, 0, 2, -9, 4]), [(0, 2), (4, 4)]);
    }

    #[test]
    fn a_stretch_rich_in_cpg_is_one_island() {
        let bases = text(&[("A", 1_000), ("CGCA", 75), ("A", 1_000)]);
        let found = islands(&bases, 0);
        let island = Island {
            span: Region::between(1_000, 1_298),
            cpgs: 75,
            c: 149,
            g: 75,
        };
        assert_eq!(found, [island]);
        assert!((island.gc_percent() - 224.0 / 2.98).abs() < 1e-9);
        assert!((island.cpg_percent() - 150.0 / 2.98).abs() < 1e-9);
        assert!((island.ratio() - 2.0).abs() < 1e-9);
        let lower = text(&[("a", 1_000), ("cgca", 75), ("a", 1_000)]);
        assert_eq!(
            islands(&lower, 5_000)[0].span,
            Region::between(6_000, 6_298)
        );
    }

    #[test]
    fn islands_are_long_rich_in_gc_and_rich_in_cpg() {
        let short = text(&[("A", 100), ("CGCA", 40), ("A", 100)]);
        assert_eq!(islands(&short, 0), []);
        let weak = text(&[("A", 100), ("CGATAT", 50), ("A", 100)]);
        assert_eq!(islands(&weak, 0), []);
        let few = text(&[("A", 100), ("GGGGCCCC", 40), ("A", 100)]);
        assert_eq!(islands(&few, 0), []);
    }

    #[test]
    fn near_stretches_join_and_far_stretches_stay_apart() {
        let near = text(&[
            ("A", 100),
            ("CGCA", 60),
            ("A", 20),
            ("CGCA", 60),
            ("A", 100),
        ]);
        let spans: Vec<Region> = islands(&near, 0).iter().map(|island| island.span).collect();
        assert_eq!(spans, [Region::between(100, 598)]);
        let far = text(&[
            ("A", 100),
            ("CGCA", 60),
            ("A", 2_000),
            ("CGCA", 60),
            ("A", 100),
        ]);
        let spans: Vec<Region> = islands(&far, 0).iter().map(|island| island.span).collect();
        assert_eq!(
            spans,
            [Region::between(100, 338), Region::between(2_340, 2_578)]
        );
    }
}
