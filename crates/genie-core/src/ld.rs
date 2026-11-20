//! Linkage disequilibrium of the variants of a region: the r² of the allele counts of each pair
//! of variants over a set of samples, as PLINK computes it from genotypes without phase.

use crate::variant::Call;

/// The byte of a pair without r².
const NONE: u8 = 255;
/// The byte of an r² of 1.
const SCALE: f32 = 250.0;

/// The count of the alternate alleles of a call: 0, 1 or 2, or nothing for a missing call.
pub fn dosage(call: Call) -> Option<u8> {
    match call {
        Call::Reference => Some(0),
        Call::Het => Some(1),
        Call::Alternate => Some(2),
        Call::Missing => None,
    }
}

/// The r² of each pair of a run of variants, as a triangle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matrix {
    count: usize,
    /// The r² of each pair (i, j) with i < j, in the order of i then j, as bytes of
    /// `SCALE`, with `NONE` for a pair without r².
    values: Vec<u8>,
}

/// The sums of the allele counts of the samples of one variant, of a second variant, and of
/// their products, over `n` samples: n, the sum and the sum of squares of each, and the sum of
/// the products.
fn squared(n: f64, (sum_a, squares_a): (f64, f64), (sum_b, squares_b): (f64, f64), dot: f64) -> u8 {
    let spread_a = n * squares_a - sum_a * sum_a;
    let spread_b = n * squares_b - sum_b * sum_b;
    if n < 2.0 || spread_a <= 0.0 || spread_b <= 0.0 {
        return NONE;
    }
    let covariance = n * dot - sum_a * sum_b;
    let r2 = (covariance * covariance / (spread_a * spread_b)).clamp(0.0, 1.0);
    (r2 as f32 * SCALE).round() as u8
}

impl Matrix {
    /// The r² of each pair of variants, from the calls of each variant. With `samples`, only
    /// those samples count, by their place in the calls.
    pub fn new(calls: &[&[Call]], samples: Option<&[usize]>) -> Self {
        let rows: Vec<Vec<Option<u8>>> = calls
            .iter()
            .map(|row| match samples {
                Some(samples) => samples
                    .iter()
                    .map(|&at| row.get(at).copied().and_then(dosage))
                    .collect(),
                None => row.iter().map(|call| dosage(*call)).collect(),
            })
            .collect();
        let count = rows.len();
        let mut values = Vec::with_capacity(count * count.saturating_sub(1) / 2);
        let complete = rows.iter().all(|row| row.iter().all(Option::is_some));
        if complete {
            let plain: Vec<Vec<u8>> = rows
                .iter()
                .map(|row| row.iter().map(|value| value.unwrap_or(0)).collect())
                .collect();
            let n = plain.first().map_or(0, Vec::len) as f64;
            let sums: Vec<(f64, f64)> = plain
                .iter()
                .map(|row| {
                    let sum: u32 = row.iter().map(|&value| u32::from(value)).sum();
                    let squares: u32 = row.iter().map(|&value| u32::from(value * value)).sum();
                    (f64::from(sum), f64::from(squares))
                })
                .collect();
            for first in 0..count {
                for second in first + 1..count {
                    let dot: u32 = plain[first]
                        .iter()
                        .zip(&plain[second])
                        .map(|(&a, &b)| u32::from(a * b))
                        .sum();
                    values.push(squared(n, sums[first], sums[second], f64::from(dot)));
                }
            }
        } else {
            for first in 0..count {
                for second in first + 1..count {
                    let mut samples = 0.0;
                    let (mut one, mut other, mut dot) = ((0.0, 0.0), (0.0, 0.0), 0.0);
                    for pair in rows[first].iter().zip(&rows[second]) {
                        if let (Some(left), Some(right)) = pair {
                            let (left, right) = (f64::from(*left), f64::from(*right));
                            samples += 1.0;
                            one = (one.0 + left, one.1 + left * left);
                            other = (other.0 + right, other.1 + right * right);
                            dot += left * right;
                        }
                    }
                    values.push(squared(samples, one, other, dot));
                }
            }
        }
        Self { count, values }
    }

    /// The number of the variants.
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The r² of two variants: 1 for a variant with itself, and nothing when one of the two
    /// has the same count in all the samples with calls at both.
    pub fn get(&self, a: usize, b: usize) -> Option<f32> {
        if a >= self.count || b >= self.count {
            return None;
        }
        if a == b {
            return Some(1.0);
        }
        let (low, high) = (a.min(b), a.max(b));
        let at = low * (2 * self.count - low - 1) / 2 + (high - low - 1);
        let value = *self.values.get(at)?;
        (value != NONE).then(|| f32::from(value) / SCALE)
    }
}

#[cfg(test)]
mod tests {
    use super::Matrix;
    use crate::variant::Call::{Alternate, Het, Missing, Reference};

    #[test]
    fn pairs_of_variants_have_the_r2_of_their_allele_counts() {
        let a = [Reference, Het, Alternate, Het];
        let same = [Reference, Het, Alternate, Het];
        let flipped = [Alternate, Het, Reference, Het];
        let flat = [Het, Het, Het, Het];
        let half = [Reference, Reference, Alternate, Alternate];
        let matrix = Matrix::new(&[&a, &same, &flipped, &flat, &half], None);
        assert_eq!(matrix.len(), 5);
        assert_eq!(matrix.get(0, 1), Some(1.0));
        assert_eq!(matrix.get(1, 0), Some(1.0));
        assert_eq!(matrix.get(0, 2), Some(1.0));
        assert_eq!(matrix.get(0, 3), None);
        assert_eq!(matrix.get(0, 4), Some(0.5));
        assert_eq!(matrix.get(2, 2), Some(1.0));
        assert_eq!(matrix.get(4, 5), None);
        assert!(Matrix::new(&[], None).is_empty());
    }

    #[test]
    fn missing_calls_leave_out_the_sample_and_groups_pick_samples() {
        let a = [Reference, Het, Alternate, Missing, Het];
        let b = [Reference, Het, Alternate, Alternate, Reference];
        let matrix = Matrix::new(&[&a, &b], None);
        let r2 = matrix.get(0, 1).unwrap();
        assert!((r2 - 64.0 / 88.0).abs() < 0.005, "{r2}");
        let group = Matrix::new(&[&a, &b], Some(&[0, 1, 2]));
        assert_eq!(group.get(0, 1), Some(1.0));
    }
}
