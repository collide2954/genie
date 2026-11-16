//! UCSC chain files, as `hg19ToHg38.over.chain.gz`: blocks of bases that align between a target
//! assembly (the one of the data) and a query assembly (the one in view).

use std::collections::HashMap;
use std::io::BufRead;

use genie_core::Region;

use crate::{Error, Reason};

/// Intervals that a region of the query needs from the target join when they are this close.
const JOIN: u64 = 100_000;

/// A block of aligned bases: its start on the target, its start on the forward strand of the
/// query, its length and its chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Block {
    target: u32,
    query: u32,
    size: u32,
    chain: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Link {
    target: usize,
    query: usize,
    /// Whether the query is on the reverse strand.
    reverse: bool,
}

/// The chains of a file, indexed both ways.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chains {
    names: Vec<String>,
    links: Vec<Link>,
    /// The blocks of each target contig, in the order of their target starts.
    forward: HashMap<usize, Vec<Block>>,
    /// The blocks of each query contig, in the order of their query starts.
    back: HashMap<usize, Vec<Block>>,
}

impl Chains {
    pub fn read(reader: impl BufRead) -> Result<Self, Error> {
        let mut chains = Self::default();
        let mut ids: HashMap<String, usize> = HashMap::new();
        let mut current: Option<(u32, u64, u64, u64)> = None;
        for (number, read) in reader.lines().enumerate() {
            let read = read?;
            let text = read.trim();
            let bad = || Error::Format {
                line: number as u64 + 1,
                reason: Reason::BadLine,
            };
            if text.is_empty() || text.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = text.split_ascii_whitespace().collect();
            if fields[0] == "chain" {
                let [
                    _,
                    _,
                    target,
                    _,
                    _,
                    target_start,
                    _,
                    query,
                    query_size,
                    strand,
                    query_start,
                    ..,
                ] = fields[..]
                else {
                    return Err(bad());
                };
                let mut id = |name: &str| {
                    *ids.entry(name.to_owned()).or_insert_with(|| {
                        chains.names.push(name.to_owned());
                        chains.names.len() - 1
                    })
                };
                let link = Link {
                    target: id(target),
                    query: id(query),
                    reverse: strand == "-",
                };
                let number = |text: &str| text.parse::<u64>().map_err(|_| bad());
                let chain = u32::try_from(chains.links.len()).map_err(|_| bad())?;
                chains.links.push(link);
                current = Some((
                    chain,
                    number(target_start)?,
                    number(query_start)?,
                    number(query_size)?,
                ));
                continue;
            }
            let Some((chain, target, query, size_of_query)) = current.as_mut() else {
                return Err(bad());
            };
            let numbers: Vec<u64> = fields
                .iter()
                .map(|field| field.parse::<u64>())
                .collect::<Result<_, _>>()
                .map_err(|_| bad())?;
            let size = numbers[0];
            let link = &chains.links[*chain as usize];
            let forward_query = if link.reverse {
                size_of_query.checked_sub(*query + size).ok_or_else(bad)?
            } else {
                *query
            };
            let fit = |value: u64| u32::try_from(value).map_err(|_| bad());
            let block = Block {
                target: fit(*target)?,
                query: fit(forward_query)?,
                size: fit(size)?,
                chain: *chain,
            };
            chains.forward.entry(link.target).or_default().push(block);
            chains.back.entry(link.query).or_default().push(block);
            match numbers[..] {
                [_, gap_target, gap_query] => {
                    *target += size + gap_target;
                    *query += size + gap_query;
                }
                [_] => current = None,
                _ => return Err(bad()),
            }
        }
        for blocks in chains.forward.values_mut() {
            blocks.sort_by_key(|block| block.target);
        }
        for blocks in chains.back.values_mut() {
            blocks.sort_by_key(|block| block.query);
        }
        Ok(chains)
    }

    /// The target contigs of the chains.
    pub fn targets(&self) -> Vec<&str> {
        let mut found: Vec<usize> = self.forward.keys().copied().collect();
        found.sort_unstable();
        found
            .into_iter()
            .map(|id| self.names[id].as_str())
            .collect()
    }

    /// The query contigs of the chains.
    pub fn queries(&self) -> Vec<&str> {
        let mut found: Vec<usize> = self.back.keys().copied().collect();
        found.sort_unstable();
        found
            .into_iter()
            .map(|id| self.names[id].as_str())
            .collect()
    }

    fn id(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|known| known == name)
    }

    /// The query contig, the span on it and the strand change of a span of a target contig: the
    /// span between the first and the last aligned base of the chain that aligns the most of its
    /// bases. Nothing when that chain aligns less than half of them.
    pub fn lift(&self, contig: &str, span: Region) -> Option<(&str, Region, bool)> {
        let blocks = self.forward.get(&self.id(contig)?)?;
        let (start, end) = (span.start(), span.end());
        let first = blocks
            .partition_point(|block| u64::from(block.target) + u64::from(block.size) <= start);
        let mut shares: HashMap<u32, (u64, u64, u64)> = HashMap::new();
        for block in blocks[first..]
            .iter()
            .take_while(|block| u64::from(block.target) < end)
        {
            let from = u64::from(block.target).max(start);
            let to = (u64::from(block.target) + u64::from(block.size)).min(end);
            let mapped = |at: u64| self.map(block, at);
            let (low, high) = {
                let (a, b) = (mapped(from), mapped(to - 1));
                (a.min(b), a.max(b))
            };
            let share = shares.entry(block.chain).or_insert((0, u64::MAX, 0));
            share.0 += to - from;
            share.1 = share.1.min(low);
            share.2 = share.2.max(high);
        }
        let (chain, (aligned, low, high)) = shares
            .into_iter()
            .max_by_key(|(chain, (aligned, ..))| (*aligned, std::cmp::Reverse(*chain)))?;
        if aligned * 2 < span.len().max(1) {
            return None;
        }
        let link = &self.links[chain as usize];
        Some((
            self.names[link.query].as_str(),
            Region::between(low, high + 1),
            link.reverse,
        ))
    }

    /// The position on the forward strand of the query of a target position in a block.
    fn map(&self, block: &Block, at: u64) -> u64 {
        let offset = at - u64::from(block.target);
        if self.links[block.chain as usize].reverse {
            u64::from(block.query) + u64::from(block.size) - 1 - offset
        } else {
            u64::from(block.query) + offset
        }
    }

    /// The target contigs and regions that align with a region of a query contig, joined when
    /// they are near each other.
    pub fn back(&self, contig: &str, region: Region) -> Vec<(&str, Region)> {
        let Some(blocks) = self.id(contig).and_then(|id| self.back.get(&id)) else {
            return Vec::new();
        };
        let longest = blocks
            .iter()
            .map(|block| u64::from(block.size))
            .max()
            .unwrap_or(0);
        let first =
            blocks.partition_point(|block| u64::from(block.query) + longest <= region.start());
        let mut found: Vec<(usize, Region)> = Vec::new();
        for block in blocks[first..]
            .iter()
            .take_while(|block| u64::from(block.query) < region.end())
        {
            let (query, size) = (u64::from(block.query), u64::from(block.size));
            let from = query.max(region.start());
            let to = (query + size).min(region.end());
            if from >= to {
                continue;
            }
            let link = &self.links[block.chain as usize];
            let target = u64::from(block.target);
            let part = if link.reverse {
                Region::between(target + (query + size - to), target + (query + size - from))
            } else {
                Region::between(target + (from - query), target + (to - query))
            };
            found.push((link.target, part));
        }
        found.sort_by_key(|(contig, part)| (*contig, part.start()));
        let mut joined: Vec<(usize, Region)> = Vec::new();
        for (contig, part) in found {
            match joined.last_mut() {
                Some((last, region)) if *last == contig && part.start() <= region.end() + JOIN => {
                    *region = region.hull(part);
                }
                _ => joined.push((contig, part)),
            }
        }
        joined
            .into_iter()
            .map(|(contig, part)| (self.names[contig].as_str(), part))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::Region;

    use super::Chains;

    /// chr1 of the old assembly is chr1 of the new one with 100 more bases before position
    /// 1,000 and a gap; 500 bases of chr2 are on the reverse strand of chrB.
    const CHAINS: &str = "chain 1000 chr1 5000 + 0 3000 chr1 6000 + 0 3100 1\n\
        1000 0 100\n\
        2000\n\
        \n\
        chain 500 chr2 1000 + 100 600 chrB 2000 - 200 700 2\n\
        500\n";

    fn chains() -> Chains {
        Chains::read(Cursor::new(CHAINS)).unwrap()
    }

    #[test]
    fn spans_lift_to_the_query_through_their_blocks() {
        let chains = chains();
        assert_eq!(chains.queries(), ["chr1", "chrB"]);
        assert_eq!(chains.targets(), ["chr1", "chr2"]);
        assert_eq!(
            chains.lift("chr1", Region::between(10, 20)),
            Some(("chr1", Region::between(10, 20), false))
        );
        assert_eq!(
            chains.lift("chr1", Region::between(1_500, 1_600)),
            Some(("chr1", Region::between(1_600, 1_700), false))
        );
        assert_eq!(
            chains.lift("chr1", Region::between(900, 1_100)),
            Some(("chr1", Region::between(900, 1_200), false))
        );
        assert_eq!(
            chains.lift("chr2", Region::between(100, 110)),
            Some(("chrB", Region::between(1_790, 1_800), true))
        );
        assert_eq!(chains.lift("chr1", Region::between(3_500, 3_600)), None);
        assert_eq!(
            chains.lift("chr2", Region::between(0, 300)),
            Some(("chrB", Region::between(1_600, 1_800), true))
        );
        assert_eq!(chains.lift("chr2", Region::between(0, 150)), None);
        assert_eq!(chains.lift("chr9", Region::between(0, 10)), None);
    }

    #[test]
    fn regions_of_the_query_need_regions_of_the_target() {
        let chains = chains();
        assert_eq!(
            chains.back("chr1", Region::between(950, 1_150)),
            [("chr1", Region::between(950, 1_050))]
        );
        assert_eq!(
            chains.back("chrB", Region::between(1_700, 1_750)),
            [("chr2", Region::between(150, 200))]
        );
        assert_eq!(chains.back("chrZ", Region::between(0, 10)), []);
    }

    #[test]
    fn broken_chains_say_where() {
        let error = Chains::read(Cursor::new("chain 1 chr1\n")).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Line 1: the line is not a section or an entry."
        );
        assert!(Chains::read(Cursor::new("100 0 0\n")).is_err());
    }
}
