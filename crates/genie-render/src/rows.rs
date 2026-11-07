use std::{cmp::Reverse, collections::BinaryHeap};

use genie_core::Region;

pub fn pack<T>(items: &[T], span: impl Fn(&T) -> Region, gap: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|index| span(&items[*index]).start());
    let mut rows = vec![0; items.len()];
    let mut busy = BinaryHeap::new();
    let mut free = BinaryHeap::new();
    let mut count = 0;
    for index in order {
        let span = span(&items[index]);
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
    use genie_core::Region;

    use super::pack;

    fn rows(spans: &[(u64, u64)], gap: u64) -> Vec<usize> {
        let spans: Vec<Region> = spans
            .iter()
            .map(|&(start, end)| Region::new(start, end).unwrap())
            .collect();
        pack(&spans, |span| *span, gap)
    }

    #[test]
    fn spans_apart_share_a_row() {
        assert_eq!(rows(&[(0, 10), (20, 30)], 0), [0, 0]);
    }

    #[test]
    fn overlapping_spans_stack() {
        assert_eq!(rows(&[(0, 10), (5, 15), (12, 20)], 0), [0, 1, 0]);
    }

    #[test]
    fn the_gap_keeps_close_spans_apart() {
        assert_eq!(rows(&[(0, 10), (12, 20)], 5), [0, 1]);
    }

    #[test]
    fn takes_the_lowest_free_row() {
        assert_eq!(
            rows(&[(0, 100), (0, 10), (0, 10), (20, 30)], 0),
            [0, 1, 2, 1]
        );
    }

    #[test]
    fn sorts_spans_by_start() {
        assert_eq!(rows(&[(20, 30), (0, 25)], 0), [1, 0]);
    }
}
