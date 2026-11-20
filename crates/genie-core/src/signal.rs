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

/// Counts of records in the parts of a region, `step` bases each, as densities per Mb.
#[derive(Debug, Clone, PartialEq)]
pub struct Tally {
    region: Region,
    step: u64,
    counts: Vec<f64>,
}

impl Tally {
    pub fn new(region: Region, step: u64) -> Self {
        let step = step.max(1);
        let parts = usize::try_from(region.len().div_ceil(step)).unwrap_or(0);
        Self {
            region,
            step,
            counts: vec![0.0; parts],
        }
    }

    fn part(&self, index: usize) -> Region {
        let start = self.region.start() + index as u64 * self.step;
        Region::between(start, (start + self.step).min(self.region.end()))
    }

    /// Adds a record that starts at a position.
    pub fn add(&mut self, position: u64) {
        if self.region.contains(position) {
            let index = (position - self.region.start()) / self.step;
            if let Some(count) = usize::try_from(index)
                .ok()
                .and_then(|index| self.counts.get_mut(index))
            {
                *count += 1.0;
            }
        }
    }

    /// Adds an amount of records spread evenly over a span.
    pub fn spread(&mut self, span: Region, amount: f64) {
        let Some(inside) = span
            .intersection(self.region)
            .filter(|inside| !inside.is_empty())
        else {
            return;
        };
        let per_base = amount / span.len() as f64;
        let first = (inside.start() - self.region.start()) / self.step;
        let last = (inside.end() - 1 - self.region.start()) / self.step;
        for index in first..=last {
            let Ok(index) = usize::try_from(index) else {
                break;
            };
            let Some(overlap) = self.part(index).intersection(inside) else {
                continue;
            };
            if let Some(count) = self.counts.get_mut(index) {
                *count += per_base * overlap.len() as f64;
            }
        }
    }

    pub fn total(&self) -> f64 {
        self.counts.iter().sum()
    }

    /// One summary for each part, with its records per Mb.
    pub fn summaries(&self) -> Vec<Summary> {
        self.counts
            .iter()
            .enumerate()
            .map(|(index, count)| {
                let part = self.part(index);
                Summary::single(part, (count / part.len() as f64 * 1e6) as f32)
            })
            .collect()
    }
}

pub fn reduce(summaries: &[Summary], step: u64) -> Vec<Summary> {
    if step <= 1 {
        return summaries.to_vec();
    }
    let mut reducer = Reducer::new(step);
    for summary in summaries {
        reducer.push(*summary);
    }
    reducer.finish()
}

/// Merges summaries in order into steps of `step` bases, one summary at a time, so that a long
/// list of small summaries does not need to be in memory at one time.
#[derive(Debug, Clone, PartialEq)]
pub struct Reducer {
    step: u64,
    found: Vec<Summary>,
}

impl Reducer {
    pub fn new(step: u64) -> Self {
        Self {
            step: step.max(1),
            found: Vec::new(),
        }
    }

    pub fn push(&mut self, summary: Summary) {
        let step = self.step;
        if step == 1 {
            self.found.push(summary);
            return;
        }
        let mut start = summary.span.start();
        while start < summary.span.end() {
            let slot = start / step;
            let part = Region::between(start, ((slot + 1) * step).min(summary.span.end()));
            let share = part.len() as f64 / summary.span.len() as f64;
            let count = (f64::from(summary.count) * share).round() as u32;
            let sum = summary.sum * share;
            match self.found.last_mut() {
                Some(last) if last.span.start() / step == slot => {
                    last.span = last.span.hull(part);
                    last.count += count;
                    last.min = last.min.min(summary.min);
                    last.max = last.max.max(summary.max);
                    last.sum += sum;
                }
                _ => self.found.push(Summary {
                    span: part,
                    count,
                    min: summary.min,
                    max: summary.max,
                    sum,
                }),
            }
            start = part.end();
        }
    }

    pub fn finish(self) -> Vec<Summary> {
        self.found
    }
}

#[cfg(test)]
mod tests {
    use super::{Bin, Summary, Tally, bins, reduce};
    use crate::Region;

    #[test]
    fn reduce_merges_records_into_steps() {
        let records = [value(0, 10, 1.0), value(10, 30, 3.0), value(45, 50, 2.0)];
        let summary = |start, end, count, min, max, sum| Summary {
            span: Region::between(start, end),
            count,
            min,
            max,
            sum,
        };
        assert_eq!(
            reduce(&records, 20),
            [
                summary(0, 20, 20, 1.0, 3.0, 40.0),
                summary(20, 30, 10, 3.0, 3.0, 30.0),
                summary(45, 50, 5, 2.0, 2.0, 10.0),
            ]
        );
        assert_eq!(reduce(&records, 1), records);
    }

    #[test]
    fn reduced_records_give_the_same_bins() {
        let records: Vec<Summary> = (0..40)
            .map(|index| value(index * 7, index * 7 + 5, index as f32 * 0.5 - 3.0))
            .collect();
        let region = Region::between(0, 280);
        let raw = bins(&records, region, 14);
        let reduced = bins(&reduce(&records, 10), region, 14);
        for (raw, reduced) in raw.iter().zip(&reduced) {
            let (raw, reduced) = (raw.unwrap(), reduced.unwrap());
            assert!((raw.mean() - reduced.mean()).abs() < 1e-9);
            assert!((raw.count - reduced.count).abs() < 1e-9);
            assert_eq!((raw.min, raw.max), (reduced.min, reduced.max));
        }
    }

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
    fn tallies_give_records_per_megabase() {
        let region = Region::between(1_000_000, 3_500_000);
        let mut tally = Tally::new(region, 1_000_000);
        for position in [
            1_000_000, 1_500_000, 1_999_999, 2_000_000, 999_999, 3_500_000,
        ] {
            tally.add(position);
        }
        tally.spread(Region::between(2_500_000, 3_500_000), 2.0);
        let summaries = tally.summaries();
        let densities: Vec<(u64, u64, f32)> = summaries
            .iter()
            .map(|summary| (summary.span.start(), summary.span.end(), summary.max))
            .collect();
        assert_eq!(
            densities,
            [
                (1_000_000, 2_000_000, 3.0),
                (2_000_000, 3_000_000, 2.0),
                (3_000_000, 3_500_000, 2.0)
            ]
        );
        let all = bins(&summaries, region, 1)[0].unwrap();
        assert!((all.mean() - 2.4).abs() < 1e-9, "{}", all.mean());
        assert!((tally.total() - 6.0).abs() < 1e-9);
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
