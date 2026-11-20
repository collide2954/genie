//! What the bases of a sequence are made of: the GC content of its windows, and its runs of
//! soft-masked bases.

use crate::Region;
use crate::signal::{Reducer, Summary};

/// The GC content of the windows of a sequence, in percent, merged into summaries of `step`
/// bases. The windows have `window` bases and start at multiples of `window`, so that a window
/// does not change with the part of the sequence that a view reads. A window with more unknown
/// bases (N) than known bases has no value.
pub fn gc(bases: &[u8], start: u64, window: u64, step: u64) -> Vec<Summary> {
    let window = window.max(1);
    let end = start + bases.len() as u64;
    let mut reducer = Reducer::new(step);
    let mut from = start;
    while from < end {
        let to = ((from / window + 1) * window).min(end);
        let part = &bases[(from - start) as usize..(to - start) as usize];
        let (known, strong) =
            part.iter().fold((0_u32, 0_u32), |(known, strong), base| {
                match base.to_ascii_uppercase() {
                    b'G' | b'C' => (known + 1, strong + 1),
                    b'A' | b'T' => (known + 1, strong),
                    _ => (known, strong),
                }
            });
        if known > 0 && 2 * u64::from(known) >= part.len() as u64 {
            let percent = f64::from(strong) / f64::from(known) * 100.0;
            reducer.push(Summary::single(Region::between(from, to), percent as f32));
        }
        from = to;
    }
    reducer.finish()
}

/// The runs of lower-case bases of a sequence, as the soft-masked repeats of a genome.
pub fn masked(bases: &[u8], start: u64) -> Vec<Region> {
    let mut runs = Vec::new();
    let mut open = None;
    for (offset, base) in bases.iter().enumerate() {
        let at = start + offset as u64;
        match (base.is_ascii_lowercase(), open) {
            (true, None) => open = Some(at),
            (false, Some(first)) => {
                runs.push(Region::between(first, at));
                open = None;
            }
            _ => {}
        }
    }
    if let Some(first) = open {
        runs.push(Region::between(first, start + bases.len() as u64));
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::{gc, masked};
    use crate::Region;
    use crate::signal::Summary;

    fn values(summaries: &[Summary]) -> Vec<(u64, u64, f64)> {
        summaries
            .iter()
            .map(|summary| {
                let mean = summary.sum / f64::from(summary.count);
                (summary.span.start(), summary.span.end(), mean)
            })
            .collect()
    }

    #[test]
    fn windows_count_their_g_and_c_bases() {
        let found = gc(b"GGGGCCCCAAAATTTTGCAT", 0, 4, 1);
        assert_eq!(
            values(&found),
            [
                (0, 4, 100.0),
                (4, 8, 100.0),
                (8, 12, 0.0),
                (12, 16, 0.0),
                (16, 20, 50.0)
            ]
        );
        assert_eq!(values(&gc(b"ggccatat", 0, 8, 1)), [(0, 8, 50.0)]);
    }

    #[test]
    fn windows_start_at_multiples_of_their_size() {
        let found = gc(b"GGAAAAAAGG", 6, 4, 1);
        assert_eq!(
            values(&found),
            [(6, 8, 100.0), (8, 12, 0.0), (12, 16, 50.0)]
        );
    }

    #[test]
    fn windows_with_more_unknown_bases_than_known_bases_have_no_value() {
        let found = gc(b"NNNANNACGCNN", 0, 4, 1);
        assert_eq!(values(&found), [(4, 8, 50.0), (8, 12, 100.0)]);
        assert_eq!(gc(b"NNNN", 0, 4, 1), []);
    }

    #[test]
    fn steps_keep_the_mean_and_the_highest_window() {
        let found = gc(b"GGGGAAAAGCATGCAT", 0, 4, 8);
        assert_eq!(values(&found), [(0, 8, 50.0), (8, 16, 50.0)]);
        assert_eq!((found[0].min, found[0].max), (0.0, 100.0));
        assert_eq!((found[1].min, found[1].max), (50.0, 50.0));
    }

    #[test]
    fn lower_case_runs_are_masked() {
        assert_eq!(
            masked(b"ACgtaCGnnA", 100),
            [Region::between(102, 105), Region::between(107, 109)]
        );
        assert_eq!(masked(b"acGT", 0), [Region::between(0, 2)]);
        assert_eq!(masked(b"ACgt", 0), [Region::between(2, 4)]);
        assert_eq!(masked(b"ACGT", 0), []);
    }
}
