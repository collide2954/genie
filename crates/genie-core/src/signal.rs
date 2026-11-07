use crate::Region;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub span: Region,
    pub count: u32,
    pub min: f32,
    pub max: f32,
    pub sum: f64,
}

impl Summary {
    pub fn single(span: Region, value: f32) -> Self {
        let count = u32::try_from(span.len()).unwrap_or(u32::MAX);
        Self {
            span,
            count,
            min: value,
            max: value,
            sum: f64::from(value) * f64::from(count),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bin {
    pub count: f64,
    pub min: f32,
    pub max: f32,
    pub sum: f64,
}

impl Bin {
    pub fn mean(self) -> f64 {
        self.sum / self.count
    }
}

pub fn slice(summaries: &[Summary], range: Region) -> Option<Bin> {
    let mut bin: Option<Bin> = None;
    for summary in summaries
        .iter()
        .take_while(|summary| summary.span.start() < range.end())
    {
        let Some(overlap) = summary.span.intersection(range) else {
            continue;
        };
        let share = overlap.len() as f64 / summary.span.len() as f64;
        let part = Bin {
            count: f64::from(summary.count) * share,
            min: summary.min,
            max: summary.max,
            sum: summary.sum * share,
        };
        bin = Some(match bin {
            None => part,
            Some(bin) => Bin {
                count: bin.count + part.count,
                min: bin.min.min(part.min),
                max: bin.max.max(part.max),
                sum: bin.sum + part.sum,
            },
        });
    }
    bin.filter(|bin| bin.count > 0.0)
}

pub fn bins(summaries: &[Summary], region: Region, count: usize) -> Vec<Option<Bin>> {
    let mut first = 0;
    let mut start = region.start();
    (1..=count as u64)
        .map(|index| {
            let end = region.start() + region.len() * index / count as u64;
            let stop = if end == start { start + 1 } else { end };
            while summaries
                .get(first)
                .is_some_and(|summary| summary.span.end() <= start)
            {
                first += 1;
            }
            let bin = slice(&summaries[first..], Region::between(start, stop));
            start = end;
            bin
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Bin, Summary, bins};
    use crate::Region;

    fn value(start: u64, end: u64, value: f32) -> Summary {
        Summary::single(Region::between(start, end), value)
    }

    #[test]
    fn bins_weight_values_by_overlap() {
        let summaries = [value(0, 10, 1.0), value(10, 20, 3.0)];
        let found = bins(&summaries, Region::between(5, 15), 1);
        let expected = Bin {
            count: 10.0,
            min: 1.0,
            max: 3.0,
            sum: 20.0,
        };
        assert_eq!(found, [Some(expected)]);
        assert!((expected.mean() - 2.0).abs() < 1e-12);
    }

    #[test]
    fn summaries_count_by_their_share() {
        let record = Summary {
            span: Region::between(0, 100),
            count: 50,
            min: 0.0,
            max: 9.0,
            sum: 100.0,
        };
        let half = Bin {
            count: 25.0,
            min: 0.0,
            max: 9.0,
            sum: 50.0,
        };
        assert_eq!(bins(&[record], Region::between(0, 100), 2), [Some(half); 2]);
    }

    #[test]
    fn bins_without_data_are_empty() {
        let found = bins(&[value(40, 50, 2.0)], Region::between(0, 100), 4);
        let filled: Vec<bool> = found.iter().map(Option::is_some).collect();
        assert_eq!(filled, [false, true, false, false]);
    }

    #[test]
    fn bins_narrower_than_a_base_take_one_base() {
        let found = bins(&[value(0, 10, 2.5)], Region::between(0, 4), 8);
        for bin in found {
            let bin = bin.unwrap();
            assert!((bin.mean() - 2.5).abs() < 1e-12);
            assert!((bin.count - 1.0).abs() < 1e-12);
        }
    }
}
