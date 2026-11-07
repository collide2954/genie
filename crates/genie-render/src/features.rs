use std::{cmp::Reverse, collections::BinaryHeap};

use genie_core::Feature;

pub fn pack(features: &[Feature], gap: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..features.len()).collect();
    order.sort_by_key(|index| features[*index].span().start());
    let mut rows = vec![0; features.len()];
    let mut busy = BinaryHeap::new();
    let mut free = BinaryHeap::new();
    let mut count = 0;
    for index in order {
        let span = features[index].span();
        while let Some(&Reverse((end, row))) = busy.peek()
            && end <= span.start()
        {
            busy.pop();
            free.push(Reverse(row));
        }
        let row = free.pop().map_or_else(
            || {
                count += 1;
                count - 1
            },
            |Reverse(row)| row,
        );
        rows[index] = row;
        busy.push(Reverse((span.end().saturating_add(gap), row)));
    }
    rows
}

#[cfg(test)]
mod tests {
    use genie_core::{Feature, Region, Strand};

    use super::pack;

    fn features(spans: &[(u64, u64)]) -> Vec<Feature> {
        spans
            .iter()
            .map(|&(start, end)| {
                Feature::new("", Region::new(start, end).unwrap(), Strand::Forward)
            })
            .collect()
    }

    #[test]
    fn features_apart_share_a_row() {
        assert_eq!(pack(&features(&[(0, 10), (20, 30)]), 0), [0, 0]);
    }

    #[test]
    fn overlapping_features_stack() {
        assert_eq!(pack(&features(&[(0, 10), (5, 15), (12, 20)]), 0), [0, 1, 0]);
    }

    #[test]
    fn the_gap_keeps_close_features_apart() {
        assert_eq!(pack(&features(&[(0, 10), (12, 20)]), 5), [0, 1]);
    }

    #[test]
    fn takes_the_lowest_free_row() {
        let features = features(&[(0, 100), (0, 10), (0, 10), (20, 30)]);
        assert_eq!(pack(&features, 0), [0, 1, 2, 1]);
    }

    #[test]
    fn sorts_features_by_start() {
        assert_eq!(pack(&features(&[(20, 30), (0, 25)]), 0), [1, 0]);
    }
}
