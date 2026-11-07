use std::iter;

use genie_core::grouped;

pub fn major_step(scale: f64, min_spacing: f64) -> u64 {
    steps()
        .find(|&step| step as f64 / scale >= min_spacing)
        .unwrap_or(u64::MAX)
}

pub fn minor_step(major: u64, scale: f64, min_spacing: f64) -> Option<u64> {
    steps()
        .skip_while(|&step| step.saturating_mul(10) < major)
        .take_while(|&step| step < major)
        .filter(|&step| major.is_multiple_of(step))
        .find(|&step| step as f64 / scale >= min_spacing)
}

pub fn label(position: u64, step: u64) -> String {
    let (unit, suffix) = [(1_000_000_000, " Gb"), (1_000_000, " Mb"), (1_000, " kb")]
        .into_iter()
        .find(|&(unit, _)| step >= unit)
        .unwrap_or((1, ""));
    format!("{}{suffix}", grouped(position / unit))
}

fn steps() -> impl Iterator<Item = u64> {
    iter::successors(Some(1_u64), |power| power.checked_mul(10)).flat_map(|power| {
        [1, 2, 5]
            .into_iter()
            .filter_map(move |factor| power.checked_mul(factor))
    })
}

#[cfg(test)]
mod tests {
    use super::{label, major_step, minor_step};

    #[test]
    fn major_step_is_smallest_spaced_1_2_5_value() {
        assert_eq!(major_step(1.0, 100.0), 100);
        assert_eq!(major_step(10.0, 100.0), 1_000);
        assert_eq!(major_step(3.0, 100.0), 500);
        assert_eq!(major_step(1.0 / 32.0, 100.0), 5);
        assert_eq!(major_step(1e7, 100.0), 1_000_000_000);
    }

    #[test]
    fn minor_step_divides_major_step() {
        assert_eq!(minor_step(1_000, 1.0, 10.0), Some(100));
        assert_eq!(minor_step(2_000, 1.0, 10.0), Some(200));
        assert_eq!(minor_step(5_000, 1.0, 600.0), Some(1_000));
        assert_eq!(minor_step(5_000, 1.0, 2_000.0), None);
        assert_eq!(minor_step(1, 1.0, 1.0), None);
    }

    #[test]
    fn labels_use_unit_of_step() {
        assert_eq!(label(12_500_000, 500_000), "12,500 kb");
        assert_eq!(label(14_000_000, 2_000_000), "14 Mb");
        assert_eq!(label(3_000_000_000, 1_000_000_000), "3 Gb");
        assert_eq!(label(38_270_125, 5), "38,270,125");
    }
}
