//! Motifs of DNA as weight matrices: the score of each window of a sequence, and the exact
//! p-value of a score, as FIMO finds them.

use crate::{Region, Strand};

/// The weight of a count of the background that a motif gets, as the pseudocount of FIMO.
const PSEUDO: f64 = 0.1;
/// The number of sites of a motif that does not say how many it has.
const SITES: f64 = 20.0;
/// The parts of a bit that a score counts.
const UNIT: f64 = 100.0;

/// A motif: its ID, its name, the chance of each base (A, C, G, T) at each place, and the
/// number of sites that gave the chances.
#[derive(Debug, Clone, PartialEq)]
pub struct Motif {
    pub id: String,
    pub name: String,
    pub probabilities: Vec<[f64; 4]>,
    pub sites: Option<f64>,
}

impl Motif {
    /// The base with the most chance at each place.
    pub fn consensus(&self) -> String {
        self.probabilities
            .iter()
            .map(|row| {
                let best = (0..4).fold(0, |best, at| if row[at] > row[best] { at } else { best });
                char::from(BASES[best])
            })
            .collect()
    }
}

const BASES: &[u8; 4] = b"ACGT";

/// The place of a base in A, C, G, T, or None for other letters.
fn code(base: u8) -> Option<usize> {
    match base.to_ascii_uppercase() {
        b'A' => Some(0),
        b'C' => Some(1),
        b'G' => Some(2),
        b'T' => Some(3),
        _ => None,
    }
}

/// The scores of a motif on each strand, in hundredths of a bit, and the chance of each total
/// score in a random sequence of the background.
#[derive(Debug, Clone, PartialEq)]
pub struct Scorer {
    forward: Vec<[i32; 4]>,
    reverse: Vec<[i32; 4]>,
    lowest: i32,
    /// The chance that a random window scores at least each total, from the lowest total.
    tail: Vec<f64>,
}

/// A match of a motif: its bases, its strand, its score in bits and its p-value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Site {
    pub span: Region,
    pub strand: Strand,
    pub score: f64,
    pub p: f64,
}

impl Scorer {
    /// The scorer of a motif against the chances of the bases of the background.
    pub fn new(motif: &Motif, background: [f64; 4]) -> Self {
        let sites = motif.sites.filter(|sites| *sites > 0.0).unwrap_or(SITES);
        let forward: Vec<[i32; 4]> = motif
            .probabilities
            .iter()
            .map(|row| {
                let total: f64 = row.iter().sum();
                std::array::from_fn(|at| {
                    let chance = if total > 0.0 { row[at] / total } else { 0.25 };
                    let corrected = (chance * sites + PSEUDO * background[at]) / (sites + PSEUDO);
                    ((corrected / background[at]).log2() * UNIT).round() as i32
                })
            })
            .collect();
        let reverse = forward
            .iter()
            .rev()
            .map(|row| std::array::from_fn(|at| row[3 - at]))
            .collect();
        let mut chances = vec![1.0];
        let mut lowest = 0;
        for row in &forward {
            let (low, high) = (
                *row.iter().min().unwrap_or(&0),
                *row.iter().max().unwrap_or(&0),
            );
            let mut next = vec![0.0; chances.len() + (high - low) as usize];
            for (total, chance) in chances.iter().enumerate() {
                for (at, score) in row.iter().enumerate() {
                    next[total + (score - low) as usize] += chance * background[at];
                }
            }
            chances = next;
            lowest += low;
        }
        let mut tail = chances;
        for at in (0..tail.len().saturating_sub(1)).rev() {
            tail[at] += tail[at + 1];
        }
        Self {
            forward,
            reverse,
            lowest,
            tail,
        }
    }

    /// The number of bases of the motif.
    pub fn len(&self) -> usize {
        self.forward.len()
    }

    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }

    /// The chance that a random window of the background scores at least `score`.
    pub fn p_value(&self, score: i32) -> f64 {
        let at = i64::from(score) - i64::from(self.lowest);
        match usize::try_from(at) {
            Err(_) => 1.0,
            Ok(at) => self.tail.get(at).copied().unwrap_or(0.0),
        }
    }

    /// The lowest score with a p-value under `p`, if a score has one.
    pub fn least(&self, p: f64) -> Option<i32> {
        let at = self.tail.partition_point(|chance| *chance >= p);
        if at >= self.tail.len() || self.tail[at] <= 0.0 {
            return None;
        }
        i32::try_from(at).ok().map(|at| self.lowest + at)
    }

    /// The matches of the motif on the two strands of a sequence that starts at `start`, with a
    /// p-value under `p`. A window with a base that is not A, C, G or T does not match.
    pub fn scan(&self, bases: &[u8], start: u64, p: f64) -> Vec<Site> {
        let length = self.forward.len();
        let Some(least) = self.least(p) else {
            return Vec::new();
        };
        if length == 0 || bases.len() < length {
            return Vec::new();
        }
        let codes: Vec<Option<usize>> = bases.iter().map(|base| code(*base)).collect();
        let mut found = Vec::new();
        for at in 0..=bases.len() - length {
            let window = &codes[at..at + length];
            if window.iter().any(Option::is_none) {
                continue;
            }
            let span = Region::between(start + at as u64, start + (at + length) as u64);
            for (matrix, strand) in [
                (&self.forward, Strand::Forward),
                (&self.reverse, Strand::Reverse),
            ] {
                let score: i32 = matrix
                    .iter()
                    .zip(window)
                    .map(|(row, base)| base.map_or(0, |base| row[base]))
                    .sum();
                if score >= least {
                    found.push(Site {
                        span,
                        strand,
                        score: f64::from(score) / UNIT,
                        p: self.p_value(score),
                    });
                }
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::{Motif, Scorer, Site};
    use crate::{Region, Strand};

    const EVEN: [f64; 4] = [0.25; 4];

    /// A motif of strong bases: 0.97 for its base and 0.01 for each other base.
    fn strong(bases: &str) -> Motif {
        let probabilities = bases
            .bytes()
            .map(|base| {
                let at = b"ACGT".iter().position(|known| *known == base).unwrap();
                let mut row = [0.01; 4];
                row[at] = 0.97;
                row
            })
            .collect();
        Motif {
            id: "M1".to_owned(),
            name: bases.to_owned(),
            probabilities,
            sites: Some(100.0),
        }
    }

    #[test]
    fn the_consensus_is_the_base_with_the_most_chance() {
        assert_eq!(strong("GATTACA").consensus(), "GATTACA");
    }

    #[test]
    fn p_values_are_exact_for_the_background() {
        let scorer = Scorer::new(&strong("ACG"), EVEN);
        assert_eq!(scorer.len(), 3);
        assert!(scorer.least(1.0).is_some());
        let all = scorer.p_value(i32::MIN);
        assert!((all - 1.0).abs() < 1e-9, "{all}");
        let top = scorer.least(0.02).unwrap();
        assert!((scorer.p_value(top) - 1.0 / 64.0).abs() < 1e-9);
        assert_eq!(scorer.least(0.015), None);
        let two = scorer.least(0.2).unwrap();
        assert!((scorer.p_value(two) - 10.0 / 64.0).abs() < 1e-9);
    }

    #[test]
    fn scans_find_matches_on_both_strands() {
        let scorer = Scorer::new(&strong("ACG"), EVEN);
        let found = scorer.scan(b"TTACGTTTCGTAANCG", 100, 0.02);
        let places: Vec<(Region, Strand)> =
            found.iter().map(|site| (site.span, site.strand)).collect();
        assert_eq!(
            places,
            [
                (Region::between(102, 105), Strand::Forward),
                (Region::between(103, 106), Strand::Reverse),
                (Region::between(108, 111), Strand::Reverse),
            ]
        );
        let Site { score, p, .. } = found[0];
        let exact = 3.0 * ((0.97 * 100.0 + 0.025) / 100.1 / 0.25_f64).log2();
        assert!((score - exact).abs() < 0.02, "{score} {exact}");
        assert!((p - 1.0 / 64.0).abs() < 1e-9, "{p}");
        assert_eq!(scorer.scan(b"acg", 0, 0.02).len(), 1);
        assert_eq!(scorer.scan(b"AC", 0, 0.02).len(), 0);
    }

    #[test]
    fn a_base_without_chance_still_has_a_score() {
        let motif = Motif {
            id: "Z".to_owned(),
            name: "Z".to_owned(),
            probabilities: vec![[1.0, 0.0, 0.0, 0.0]; 4],
            sites: None,
        };
        let scorer = Scorer::new(&motif, [0.3, 0.2, 0.2, 0.3]);
        let found = scorer.scan(b"AAAACAAA", 0, 1.01);
        assert_eq!(found.len(), 10);
        assert!(found.iter().all(|site| site.score.is_finite()));
    }
}
