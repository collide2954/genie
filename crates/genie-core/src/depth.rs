//! The depth ratio of two read files, as tumor over normal: in each bin, the depth of each file
//! over its usual depth, and the log2 of the first over the second. Segments of bins with the
//! same ratio come from binary segmentation, as CBS finds them.

use crate::Region;

/// The fewest bins of a segment.
pub const LEAST_BINS: usize = 5;
/// The largest p of the split of a segment, after the correction for the number of places where
/// the split could be.
pub const SPLIT_P: f64 = 0.001;

/// A bin: its span, the depth of each file over its usual depth, and the log2 of their ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bin {
    pub span: Region,
    pub first: f64,
    pub second: f64,
    pub ratio: f64,
}

/// Bins next to each other with the same ratio: their span, their number and their mean ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub span: Region,
    pub bins: usize,
    pub mean: f64,
}

/// The median of the values above 0, or nothing when no value is above 0.
pub fn median(values: &[f64]) -> Option<f64> {
    let mut found: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| *value > 0.0)
        .collect();
    if found.is_empty() {
        return None;
    }
    found.sort_by(f64::total_cmp);
    let middle = found.len() / 2;
    Some(if found.len().is_multiple_of(2) {
        f64::midpoint(found[middle - 1], found[middle])
    } else {
        found[middle]
    })
}

/// The bins of two files with reads in both: each depth over the usual depth of its file, and
/// the log2 of their ratio. A bin without reads in a file has no ratio.
pub fn ratios(spans: &[Region], depths: (&[f64], &[f64]), usual: (f64, f64)) -> Vec<Bin> {
    if usual.0 <= 0.0 || usual.1 <= 0.0 {
        return Vec::new();
    }
    spans
        .iter()
        .zip(depths.0.iter().zip(depths.1))
        .filter(|(_, (first, second))| **first > 0.0 && **second > 0.0)
        .map(|(span, (first, second))| {
            let (first, second) = (first / usual.0, second / usual.1);
            Bin {
                span: *span,
                first,
                second,
                ratio: (first / second).log2(),
            }
        })
        .collect()
}

/// The segments of bins in order. genie splits a run of bins where Welch's t between its two
/// parts is largest, when each part has `LEAST_BINS` bins or more and the p of the t, times the
/// number of places of the split, is under `SPLIT_P`. Then it splits each part again.
pub fn segments(bins: &[Bin]) -> Vec<Segment> {
    let values: Vec<f64> = bins.iter().map(|bin| bin.ratio).collect();
    let mut sums = Vec::with_capacity(values.len() + 1);
    let mut squares = Vec::with_capacity(values.len() + 1);
    let (mut sum, mut square) = (0.0, 0.0);
    sums.push(0.0);
    squares.push(0.0);
    for value in &values {
        sum += value;
        square += value * value;
        sums.push(sum);
        squares.push(square);
    }
    let mut cuts = vec![0, values.len()];
    let mut runs = vec![(0, values.len())];
    while let Some((start, end)) = runs.pop() {
        if let Some(cut) = split((start, end), &sums, &squares) {
            cuts.push(cut);
            runs.push((start, cut));
            runs.push((cut, end));
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .filter(|pair| pair[1] > pair[0])
        .map(|pair| {
            let (start, end) = (pair[0], pair[1]);
            Segment {
                span: bins[start].span.hull(bins[end - 1].span),
                bins: end - start,
                mean: (sums[end] - sums[start]) / (end - start) as f64,
            }
        })
        .collect()
}

/// The place in a run of bins where it splits, if a split is significant.
fn split((start, end): (usize, usize), sums: &[f64], squares: &[f64]) -> Option<usize> {
    let length = end.checked_sub(start)?;
    if length < 2 * LEAST_BINS {
        return None;
    }
    let part = |from: usize, to: usize| {
        let count = (to - from) as f64;
        let mean = (sums[to] - sums[from]) / count;
        let spread = ((squares[to] - squares[from]) - count * mean * mean) / (count - 1.0);
        (count, mean, spread.max(0.0))
    };
    let (cut, t, freedom) = (start + LEAST_BINS..=end - LEAST_BINS)
        .map(|cut| {
            let (t, freedom) = welch(part(start, cut), part(cut, end));
            (cut, t, freedom)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let places = (end - start - 2 * LEAST_BINS + 1) as f64;
    (two_sided(t, freedom) * places < SPLIT_P).then_some(cut)
}

/// Welch's t of two parts, each as (count, mean, variance), as an absolute value, and its
/// degrees of freedom.
fn welch(a: (f64, f64, f64), b: (f64, f64, f64)) -> (f64, f64) {
    let (spread_a, spread_b) = (a.2 / a.0, b.2 / b.0);
    let error = (spread_a + spread_b).sqrt();
    if error <= 0.0 {
        let same = (a.1 - b.1).abs() < f64::EPSILON;
        return (if same { 0.0 } else { f64::INFINITY }, a.0 + b.0 - 2.0);
    }
    let t = (a.1 - b.1).abs() / error;
    let freedom = (spread_a + spread_b).powi(2)
        / (spread_a.powi(2) / (a.0 - 1.0) + spread_b.powi(2) / (b.0 - 1.0));
    (t, freedom.max(1.0))
}

/// The two-sided p of a t with some degrees of freedom: I(v / (v + t²); v / 2, 1 / 2).
fn two_sided(t: f64, freedom: f64) -> f64 {
    if !t.is_finite() {
        return 0.0;
    }
    incomplete_beta(freedom / 2.0, 0.5, freedom / (freedom + t * t))
}

/// The regularized incomplete beta function I(x; a, b), by its continued fraction.
fn incomplete_beta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front =
        (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        front * fraction(a, b, x) / a
    } else {
        1.0 - front * fraction(b, a, 1.0 - x) / b
    }
}

/// The continued fraction of the incomplete beta function I(x; a, b), by the method of Lentz.
fn fraction(a: f64, b: f64, x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let away = |value: f64| if value.abs() < TINY { TINY } else { value };
    let mut upper = 1.0;
    let mut lower = 1.0 / away(1.0 - (a + b) * x / (a + 1.0));
    let mut value = lower;
    for term in 1..300 {
        let term = f64::from(term);
        let even = term * (b - term) * x / ((a + 2.0 * term - 1.0) * (a + 2.0 * term));
        let odd = -(a + term) * (a + b + term) * x / ((a + 2.0 * term) * (a + 2.0 * term + 1.0));
        for step in [even, odd] {
            lower = 1.0 / away(1.0 + step * lower);
            upper = away(1.0 + step / upper);
            value *= lower * upper;
        }
        if (lower * upper - 1.0).abs() < 1e-12 {
            break;
        }
    }
    value
}

/// The log of the gamma function, by the approximation of Lanczos.
fn ln_gamma(x: f64) -> f64 {
    const COEFFICIENTS: [f64; 6] = [
        76.180_091_729_471_46,
        -86.505_320_329_416_77,
        24.014_098_240_830_91,
        -1.231_739_572_450_155,
        0.001_208_650_973_866_179,
        -0.000_005_395_239_384_953,
    ];
    let mut y = x;
    let tmp = x + 5.5;
    let tmp = tmp - (x + 0.5) * tmp.ln();
    let mut series = 1.000_000_000_190_015;
    for coefficient in COEFFICIENTS {
        y += 1.0;
        series += coefficient / y;
    }
    -tmp + (2.506_628_274_631_000_5 * series / x).ln()
}

#[cfg(test)]
mod tests {
    use super::{Bin, LEAST_BINS, median, ratios, segments, two_sided};
    use crate::Region;

    fn bins(values: &[f64]) -> Vec<Bin> {
        values
            .iter()
            .enumerate()
            .map(|(index, ratio)| {
                let start = index as u64 * 1_000;
                Bin {
                    span: Region::between(start, start + 1_000),
                    first: 1.0,
                    second: 1.0,
                    ratio: *ratio,
                }
            })
            .collect()
    }

    /// A small noise that repeats, so that the tests do not need random numbers.
    fn noise(index: usize) -> f64 {
        [
            0.11, -0.07, 0.03, -0.12, 0.08, -0.02, 0.05, -0.09, 0.01, 0.04,
        ][index % 10]
    }

    #[test]
    fn t_tests_give_the_p_of_student() {
        assert!((two_sided(2.0, 10.0) - 0.0734).abs() < 1e-4);
        assert!((two_sided(3.0, 20.0) - 0.007_08).abs() < 1e-4);
        assert!((two_sided(0.0, 5.0) - 1.0).abs() < 1e-9);
        assert!(two_sided(f64::INFINITY, 5.0) < 1e-12);
    }

    #[test]
    fn depths_go_over_the_median_of_their_file() {
        assert_eq!(median(&[0.0, 4.0, 1.0, 9.0]), Some(4.0));
        assert_eq!(median(&[3.0, 1.0]), Some(2.0));
        assert_eq!(median(&[0.0]), None);
        let spans = [
            Region::between(0, 10),
            Region::between(10, 20),
            Region::between(20, 30),
        ];
        let found = ratios(
            &spans,
            (&[40.0, 20.0, 0.0], &[10.0, 10.0, 5.0]),
            (20.0, 10.0),
        );
        assert_eq!(found.len(), 2);
        assert!((found[0].ratio - 1.0).abs() < 1e-12);
        assert!(found[1].ratio.abs() < 1e-12);
        assert!((found[0].first - 2.0).abs() < 1e-12);
        assert_eq!(ratios(&spans, (&[1.0; 3], &[1.0; 3]), (0.0, 1.0)), []);
    }

    /// A gain of one copy of three in the middle bins of a run: log2(3/2).
    #[test]
    fn segments_find_the_steps_of_the_ratio_and_not_the_noise() {
        let gain = (1.5_f64).log2();
        let values: Vec<f64> = (0..90)
            .map(|index| noise(index) + if (30..60).contains(&index) { gain } else { 0.0 })
            .collect();
        let found = segments(&bins(&values));
        assert_eq!(found.len(), 3, "{found:?}");
        let (first, second) = (found[1].span.start(), found[1].span.end());
        assert!(first.abs_diff(30_000) <= 1_000, "{found:?}");
        assert!(second.abs_diff(60_000) <= 1_000, "{found:?}");
        assert_eq!(found.iter().map(|segment| segment.bins).sum::<usize>(), 90);
        assert!((found[1].mean - gain).abs() < 0.03, "{}", found[1].mean);
        let flat: Vec<f64> = (0..500).map(noise).collect();
        assert_eq!(segments(&bins(&flat)).len(), 1);
        let short: Vec<f64> = (0..2 * LEAST_BINS - 1)
            .map(|index| if index < LEAST_BINS { 0.0 } else { 1.0 })
            .collect();
        assert_eq!(segments(&bins(&short)).len(), 1);
        assert_eq!(segments(&[]), []);
    }
}
