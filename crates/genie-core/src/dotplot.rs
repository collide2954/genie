//! Dot plots of pairwise alignments: the target sequences across, the query sequences down, and
//! each alignment as a line from its first base on the target to its last.

use std::collections::HashMap;

use crate::Region;

/// A sequence on an axis of a dot plot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    pub name: String,
    pub length: u64,
    /// The place of the first base of the sequence on its axis: the sum of the lengths of the
    /// sequences before it.
    pub offset: u64,
}

/// An alignment of a part of a query sequence with a part of a target sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    /// The query sequence, as an index of the queries of the plot.
    pub query: usize,
    /// The target sequence, as an index of the targets of the plot.
    pub target: usize,
    pub on_query: Region,
    pub on_target: Region,
    /// True when the query aligns with the reverse strand of the target.
    pub reverse: bool,
    /// The matching bases of the alignment.
    pub matches: u64,
    /// The length of the alignment block, with its gaps.
    pub block: u64,
    /// The mapping quality, if the aligner gives one.
    pub mapq: Option<u8>,
    /// The type that minimap2 gives: `P` for primary, `S` for secondary, `I` for an inversion.
    pub kind: Option<char>,
}

impl Hit {
    /// The share of matching bases in the block, from 0 to 1.
    pub fn identity(&self) -> f64 {
        if self.block == 0 {
            return 0.0;
        }
        self.matches as f64 / self.block as f64
    }
}

/// A part of a plot, in bases: across from the first base of the first target, and down from
/// the first base of the first query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Window {
    pub across: Region,
    pub down: Region,
}

/// A dot plot as the history keeps it: the track of the plot and the part of the plot in view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shown {
    pub track: u64,
    /// The part of the plot in view, or None for the whole plot.
    pub window: Option<Window>,
}

/// The sequences of the two axes of a dot plot, in their order, and the alignments between them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plot {
    pub targets: Vec<Sequence>,
    pub queries: Vec<Sequence>,
    pub hits: Vec<Hit>,
}

impl Plot {
    /// The window that shows all sequences.
    pub fn whole(&self) -> Window {
        let end =
            |sequences: &[Sequence]| sequences.last().map_or(0, |last| last.offset + last.length);
        Window {
            across: Region::between(0, end(&self.targets)),
            down: Region::between(0, end(&self.queries)),
        }
    }

    /// The range of the identities of the alignments, for their colors: from the identity that
    /// one alignment in 20 is under, to the highest identity. None for a plot without alignments.
    pub fn identities(&self) -> Option<(f64, f64)> {
        let mut found: Vec<f64> = self
            .hits
            .iter()
            .filter(|hit| hit.block > 0)
            .map(Hit::identity)
            .collect();
        found.sort_by(f64::total_cmp);
        Some((*found.get(found.len() / 20)?, *found.last()?))
    }

    /// The line of an alignment in the plot, from its first base on the target to its last.
    /// The line goes down for the forward strand and up for the reverse strand.
    pub fn line(&self, hit: &Hit) -> ((u64, u64), (u64, u64)) {
        let across = self.targets[hit.target].offset;
        let down = self.queries[hit.query].offset;
        let (first, last) = if hit.reverse {
            (hit.on_query.end(), hit.on_query.start())
        } else {
            (hit.on_query.start(), hit.on_query.end())
        };
        (
            (across + hit.on_target.start(), down + first),
            (across + hit.on_target.end(), down + last),
        )
    }
}

/// Collects the sequences and the alignments of a plot, then puts the axes in order.
#[derive(Debug, Default)]
pub struct Builder {
    targets: Names,
    queries: Names,
    hits: Vec<Hit>,
}

#[derive(Debug, Default)]
struct Names {
    sequences: Vec<(String, u64)>,
    indices: HashMap<String, usize>,
}

impl Names {
    fn index(&mut self, name: &str, length: u64) -> usize {
        if let Some(&index) = self.indices.get(name) {
            return index;
        }
        self.sequences.push((name.to_owned(), length));
        self.indices
            .insert(name.to_owned(), self.sequences.len() - 1);
        self.sequences.len() - 1
    }
}

/// The sequences in a new order, with their offsets, and the new index of each old index.
fn placed(sequences: Vec<(String, u64)>, order: &[usize]) -> (Vec<Sequence>, Vec<usize>) {
    let mut moved = vec![0; sequences.len()];
    let mut slots: Vec<Option<(String, u64)>> = sequences.into_iter().map(Some).collect();
    let mut offset = 0;
    let placed = order
        .iter()
        .enumerate()
        .filter_map(|(at, &old)| {
            moved[old] = at;
            let (name, length) = slots[old].take()?;
            let sequence = Sequence {
                name,
                length,
                offset,
            };
            offset += length;
            Some(sequence)
        })
        .collect();
    (placed, moved)
}

impl Builder {
    /// The index of a target sequence. The builder adds the sequence the first time.
    pub fn target(&mut self, name: &str, length: u64) -> usize {
        self.targets.index(name, length)
    }

    /// The index of a query sequence. The builder adds the sequence the first time.
    pub fn query(&mut self, name: &str, length: u64) -> usize {
        self.queries.index(name, length)
    }

    pub fn push(&mut self, hit: Hit) {
        self.hits.push(hit);
    }

    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    /// The plot. The targets follow their `rank`, for example the order of the reference, and
    /// the targets without a rank come after them, the longest first. Each query goes next to
    /// the target place where most of its bases align: on the target with the most aligned
    /// bases, at the mean middle of its alignments there.
    pub fn finish(self, rank: impl Fn(&str) -> Option<usize>) -> Plot {
        let Self {
            targets,
            queries,
            mut hits,
        } = self;
        let mut across: Vec<usize> = (0..targets.sequences.len()).collect();
        across.sort_by_key(|&index| {
            let (name, length) = &targets.sequences[index];
            let rank = rank(name);
            (
                rank.is_none(),
                rank,
                std::cmp::Reverse(*length),
                name.clone(),
            )
        });
        let (targets, moved) = placed(targets.sequences, &across);
        for hit in &mut hits {
            hit.target = moved[hit.target];
        }
        let anchors = anchors(&hits, queries.sequences.len());
        let mut down: Vec<usize> = (0..queries.sequences.len()).collect();
        down.sort_by(|a, b| {
            anchors[*a]
                .cmp(&anchors[*b])
                .then_with(|| queries.sequences[*a].0.cmp(&queries.sequences[*b].0))
        });
        let (queries, moved) = placed(queries.sequences, &down);
        for hit in &mut hits {
            hit.query = moved[hit.query];
        }
        Plot {
            targets,
            queries,
            hits,
        }
    }
}

/// The place of each query along the targets: its target with the most aligned bases and the
/// mean middle of its alignments on that target, weighted by their lengths.
fn anchors(hits: &[Hit], count: usize) -> Vec<(usize, u64)> {
    let mut sums: Vec<HashMap<usize, (u128, u128)>> = vec![HashMap::new(); count];
    for hit in hits {
        let length = u128::from(hit.on_target.len().max(1));
        let middle = u128::from(hit.on_target.start() + hit.on_target.len() / 2);
        let (bases, weighted) = sums[hit.query].entry(hit.target).or_default();
        *bases += length;
        *weighted += middle * length;
    }
    sums.into_iter()
        .map(|targets| {
            let best =
                targets
                    .into_iter()
                    .max_by(|(left, (left_bases, _)), (right, (right_bases, _))| {
                        left_bases.cmp(right_bases).then_with(|| right.cmp(left))
                    });
            best.map_or((usize::MAX, 0), |(target, (bases, weighted))| {
                let middle = u64::try_from(weighted / bases).unwrap_or(u64::MAX);
                (target, middle)
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Builder, Hit, Plot, Window};
    use crate::Region;

    fn hit(query: usize, target: usize, on_target: (u64, u64), reverse: bool) -> Hit {
        Hit {
            query,
            target,
            on_query: Region::between(0, on_target.1 - on_target.0),
            on_target: Region::between(on_target.0, on_target.1),
            reverse,
            matches: 90,
            block: 100,
            mapq: Some(60),
            kind: Some('P'),
        }
    }

    /// The names of the sequences of an axis with their offsets.
    type Axis<'a> = Vec<(&'a str, u64)>;

    fn names(plot: &Plot) -> (Axis<'_>, Axis<'_>) {
        fn pairs(sequences: &[super::Sequence]) -> Axis<'_> {
            sequences
                .iter()
                .map(|sequence| (sequence.name.as_str(), sequence.offset))
                .collect()
        }
        (pairs(&plot.targets), pairs(&plot.queries))
    }

    #[test]
    fn targets_follow_their_rank_and_queries_follow_the_targets() {
        let mut builder = Builder::default();
        let un = builder.target("chrUn_1", 50);
        let two = builder.target("chr2", 500);
        let one = builder.target("chr1", 1_000);
        let late = builder.query("late", 300);
        let early = builder.query("early", 200);
        let second = builder.query("second", 100);
        let lost = builder.query("lost", 40);
        builder.push(hit(late, one, (700, 900), false));
        builder.push(hit(late, two, (0, 50), false));
        builder.push(hit(early, one, (100, 200), true));
        builder.push(hit(second, two, (10, 400), false));
        builder.push(hit(second, one, (0, 100), false));
        builder.push(hit(lost, un, (0, 40), false));
        let rank = |name: &str| ["chr1", "chr2"].iter().position(|known| *known == name);
        let plot = builder.finish(rank);
        let (targets, queries) = names(&plot);
        assert_eq!(targets, [("chr1", 0), ("chr2", 1_000), ("chrUn_1", 1_500)]);
        assert_eq!(
            queries,
            [("early", 0), ("late", 200), ("second", 500), ("lost", 600)]
        );
        let first = plot.hits[0];
        assert_eq!(
            (plot.queries[first.query].name.as_str(), first.target),
            ("late", 0)
        );
        assert_eq!(plot.targets[plot.hits[3].target].name, "chr2");
        assert_eq!(
            plot.whole(),
            Window {
                across: Region::between(0, 1_550),
                down: Region::between(0, 640),
            }
        );
    }

    #[test]
    fn lines_go_down_on_the_forward_strand_and_up_on_the_reverse_strand() {
        let mut builder = Builder::default();
        let target = builder.target("chr1", 1_000);
        let first = builder.query("a", 500);
        let second = builder.query("b", 500);
        builder.push(hit(first, target, (100, 300), false));
        let mut back = hit(second, target, (600, 800), true);
        back.on_query = Region::between(50, 250);
        builder.push(back);
        let plot = builder.finish(|_| None);
        assert_eq!(plot.line(&plot.hits[0]), ((100, 0), (300, 200)));
        assert_eq!(plot.line(&plot.hits[1]), ((600, 750), (800, 550)));
        assert!((plot.hits[0].identity() - 0.9).abs() < 1e-9);
        let empty = Hit {
            block: 0,
            ..plot.hits[0]
        };
        assert!(empty.identity().abs() < 1e-9);
    }

    #[test]
    fn identities_range_from_the_lowest_twentieth_to_the_highest() {
        let mut builder = Builder::default();
        let target = builder.target("chr1", 10_000);
        let query = builder.query("q", 10_000);
        for step in 0..40 {
            let mut alignment = hit(query, target, (step * 100, step * 100 + 100), false);
            alignment.matches = 60 + step;
            builder.push(alignment);
        }
        let plot = builder.finish(|_| None);
        let (low, high) = plot.identities().unwrap();
        assert!((low - 0.62).abs() < 1e-9 && (high - 0.99).abs() < 1e-9);
        assert_eq!(Plot::default().identities(), None);
    }

    #[test]
    fn a_sequence_comes_once_with_its_first_length() {
        let mut builder = Builder::default();
        assert!(builder.is_empty());
        assert_eq!(builder.target("chr1", 100), builder.target("chr1", 999));
        assert_eq!(builder.query("q", 10), 0);
        assert_eq!(builder.query("r", 10), 1);
        builder.push(hit(1, 0, (0, 10), false));
        assert!(!builder.is_empty());
        let plot = builder.finish(|_| None);
        assert_eq!(plot.targets[0].length, 100);
        assert_eq!(plot.queries.len(), 2);
    }
}
