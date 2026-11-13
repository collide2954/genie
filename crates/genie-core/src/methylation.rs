use std::collections::BTreeMap;

use crate::alignment::{Alignment, REVERSE};
use crate::modification::{self, MODIFIED_FROM};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    pub position: u64,
    pub modified: u32,
    pub total: u32,
}

impl Site {
    pub fn share(self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.modified as f32 / self.total as f32
        }
    }
}

type Counts = (u32, u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tally<K> {
    names: Vec<String>,
    calls: Vec<u64>,
    counts: BTreeMap<(usize, K, u64, bool), Counts>,
}

impl<K> Default for Tally<K> {
    fn default() -> Self {
        Self {
            names: Vec::new(),
            calls: Vec::new(),
            counts: BTreeMap::new(),
        }
    }
}

impl<K: Ord + Copy> Tally<K> {
    pub fn add(&mut self, read: &Alignment, group: K) {
        let reverse = read.has(REVERSE);
        for placed in read.placed_calls() {
            let Some(code) = read.tags.mods.codes.get(usize::from(placed.code)) else {
                continue;
            };
            let name = self.name_index(modification::name(&code.code));
            let flips = reverse && code.base == b'C';
            let entry = self
                .counts
                .entry((name, group, placed.position, flips))
                .or_default();
            entry.1 += 1;
            if placed.chance >= MODIFIED_FROM {
                entry.0 += 1;
            }
            self.calls[name] += 1;
        }
    }

    pub fn finish(self, base: impl Fn(u64) -> Option<u8>) -> Methylation<K> {
        let upper = |position: u64| base(position).map(|letter| letter.to_ascii_uppercase());
        let mut sites: BTreeMap<(usize, K, u64), Counts> = BTreeMap::new();
        for ((name, group, position, flips), (modified, total)) in self.counts {
            let cpg =
                position > 0 && upper(position) == Some(b'G') && upper(position - 1) == Some(b'C');
            let at = if flips && cpg { position - 1 } else { position };
            let entry = sites.entry((name, group, at)).or_default();
            entry.0 += modified;
            entry.1 += total;
        }
        Methylation {
            names: self.names,
            calls: self.calls,
            sites,
        }
    }

    fn name_index(&mut self, name: String) -> usize {
        if let Some(index) = self.names.iter().position(|known| *known == name) {
            return index;
        }
        self.names.push(name);
        self.calls.push(0);
        self.names.len() - 1
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Methylation<K> {
    names: Vec<String>,
    calls: Vec<u64>,
    sites: BTreeMap<(usize, K, u64), Counts>,
}

impl<K> Default for Methylation<K> {
    fn default() -> Self {
        Self {
            names: Vec::new(),
            calls: Vec::new(),
            sites: BTreeMap::new(),
        }
    }
}

impl<K: Ord + Copy> Methylation<K> {
    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn busiest(&self) -> Option<&str> {
        let (index, _) = self
            .calls
            .iter()
            .enumerate()
            .max_by_key(|&(index, calls)| (*calls, std::cmp::Reverse(index)))?;
        Some(self.names[index].as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.sites.is_empty()
    }

    pub fn groups(&self) -> Vec<K> {
        let mut groups: Vec<K> = self.sites.keys().map(|&(_, group, _)| group).collect();
        groups.sort_unstable();
        groups.dedup();
        groups
    }

    pub fn sites(&self, name: &str, group: K) -> Vec<Site> {
        let Some(index) = self.names.iter().position(|known| known == name) else {
            return Vec::new();
        };
        self.sites
            .iter()
            .filter(|((known, owner, _), _)| *known == index && *owner == group)
            .map(|(&(_, _, position), &(modified, total))| Site {
                position,
                modified,
                total,
            })
            .collect()
    }

    pub fn at(&self, group: K, position: u64) -> Vec<(String, u32, u32)> {
        self.names
            .iter()
            .enumerate()
            .filter_map(|(index, name)| {
                let &(modified, total) = self.sites.get(&(index, group, position))?;
                Some((name.clone(), modified, total))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Site, Tally};
    use crate::alignment::{Alignment, Op, REVERSE, Tags};
    use crate::modification::{Call, Code, Modifications};

    fn read(start: u64, flags: u16, calls: &[(u32, u8, u8)]) -> Alignment {
        Alignment {
            name: String::new(),
            start,
            flags,
            mapq: 60,
            cigar: vec![(Op::Match, 6)],
            bases: b"ACGTCG".to_vec(),
            qualities: vec![30; 6],
            mate: None,
            tags: Tags {
                mods: Modifications {
                    codes: vec![
                        Code {
                            base: b'C',
                            code: "m".to_owned(),
                        },
                        Code {
                            base: b'C',
                            code: "h".to_owned(),
                        },
                    ],
                    calls: calls
                        .iter()
                        .map(|&(at, code, chance)| Call { at, code, chance })
                        .collect(),
                },
                ..Tags::default()
            },
        }
    }

    fn site(position: u64, modified: u32, total: u32) -> Site {
        Site {
            position,
            modified,
            total,
        }
    }

    const REFERENCE: &[u8] = b"ACGTCGAGG";

    fn base(position: u64) -> Option<u8> {
        REFERENCE
            .get(usize::try_from(position.checked_sub(100)?).ok()?)
            .copied()
    }

    #[test]
    fn sites_count_modified_and_called_reads() {
        let mut tally = Tally::default();
        tally.add(&read(100, 0, &[(1, 0, 200), (1, 1, 10)]), 0);
        tally.add(&read(100, 0, &[(1, 0, 40), (4, 0, 128)]), 0);
        let methylation = tally.finish(base);
        assert_eq!(methylation.names(), ["5mC", "5hmC"]);
        assert_eq!(methylation.busiest(), Some("5mC"));
        assert_eq!(
            methylation.sites("5mC", 0),
            [site(101, 1, 2), site(104, 1, 1)]
        );
        assert_eq!(methylation.sites("5hmC", 0), [site(101, 0, 1)]);
        assert_eq!(
            methylation.at(0, 101),
            [("5mC".to_owned(), 1, 2), ("5hmC".to_owned(), 0, 1)]
        );
    }

    #[test]
    fn calls_on_the_g_of_a_cpg_join_its_c() {
        let mut tally = Tally::default();
        tally.add(&read(100, 0, &[(1, 0, 200)]), 0);
        tally.add(
            &read(100, REVERSE, &[(2, 0, 220), (5, 0, 10), (8, 0, 255)]),
            0,
        );
        let methylation = tally.finish(base);
        assert_eq!(
            methylation.sites("5mC", 0),
            [site(101, 2, 2), site(104, 0, 1)]
        );
        let mut forward = Tally::default();
        forward.add(&read(100, 0, &[(2, 0, 200)]), 0);
        assert_eq!(forward.finish(base).sites("5mC", 0), [site(102, 1, 1)]);
    }

    #[test]
    fn groups_keep_their_own_sites() {
        let mut tally = Tally::default();
        tally.add(&read(100, 0, &[(1, 0, 200)]), (0, 1));
        tally.add(&read(100, 0, &[(1, 0, 20)]), (0, 2));
        let methylation = tally.finish(base);
        assert_eq!(methylation.groups(), [(0, 1), (0, 2)]);
        assert_eq!(methylation.sites("5mC", (0, 1)), [site(101, 1, 1)]);
        assert_eq!(methylation.sites("5mC", (0, 2)), [site(101, 0, 1)]);
        assert!(!methylation.is_empty());
        assert!(Tally::<u8>::default().finish(base).is_empty());
    }
}
