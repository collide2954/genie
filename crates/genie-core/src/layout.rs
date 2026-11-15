//! The widths of contigs side by side, as in a whole-genome view.

/// The widths of contigs side by side in `width` pixels, with `gap` pixels between them: each
/// contig gets its share of the bases, and at least `least` pixels.
pub fn widths(lengths: &[u64], width: f32, gap: f32, least: f32) -> Vec<f32> {
    let count = lengths.len();
    if count == 0 {
        return Vec::new();
    }
    let room = (width - gap * (count - 1) as f32).max(least * count as f32);
    let mut narrow = vec![false; count];
    loop {
        let wide: f64 = lengths
            .iter()
            .zip(&narrow)
            .filter(|(_, narrow)| !**narrow)
            .map(|(length, _)| *length as f64)
            .sum();
        let narrows = narrow.iter().filter(|narrow| **narrow).count();
        let left = f64::from(room - least * narrows as f32);
        let share = |length: u64| {
            if wide > 0.0 {
                length as f64 / wide * left
            } else {
                0.0
            }
        };
        let mut changed = false;
        for (index, length) in lengths.iter().enumerate() {
            if !narrow[index] && share(*length) < f64::from(least) {
                narrow[index] = true;
                changed = true;
            }
        }
        if !changed {
            return lengths
                .iter()
                .zip(&narrow)
                .map(|(length, narrow)| {
                    if *narrow {
                        least
                    } else {
                        share(*length) as f32
                    }
                })
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::widths;

    fn near(found: &[f32], wanted: &[f32]) -> bool {
        found.len() == wanted.len()
            && found
                .iter()
                .zip(wanted)
                .all(|(found, wanted)| (found - wanted).abs() < 1e-3)
    }

    #[test]
    fn contigs_get_their_share_of_the_width_and_a_least_width() {
        let found = widths(&[100, 300, 1], 404.0, 1.0, 4.0);
        assert!(near(&found, &[99.5, 298.5, 4.0]), "{found:?}");
        let found = widths(&[100, 100], 21.0, 1.0, 4.0);
        assert!(near(&found, &[10.0, 10.0]), "{found:?}");
        let found = widths(&[1, 1, 1], 5.0, 1.0, 4.0);
        assert!(near(&found, &[4.0, 4.0, 4.0]), "{found:?}");
        assert_eq!(widths(&[], 100.0, 1.0, 4.0), [] as [f32; 0]);
    }
}
