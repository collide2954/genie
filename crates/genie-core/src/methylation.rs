use std::collections::BTreeMap;

use crate::alignment::{Alignment, REVERSE};
use crate::modification::{self, MODIFIED_FROM};

pub const PREFERRED: &str = "5mC";

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
            .max_by_key(|&(index, calls)| {
                (
                    *calls,
                    self.names[index] == PREFERRED,
                    std::cmp::Reverse(index),
                )
            })?;
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

/// The methylation of two groups at a site: the counts of each group, the difference of their
/// shares in percentage points (group A minus group B), and the p value of Fisher's exact test
/// on the counts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Difference {
    pub position: u64,
    pub a: Site,
    pub b: Site,
    pub points: f32,
    pub p: f64,
}

/// The sites that both groups have reads for, in the order of the sites of group A.
pub fn difference(a: &[Site], b: &[Site]) -> Vec<Difference> {
    let others: BTreeMap<u64, Site> = b.iter().map(|site| (site.position, *site)).collect();
    a.iter()
        .filter(|site| site.total > 0)
        .filter_map(|&site| {
            let other = *others.get(&site.position).filter(|other| other.total > 0)?;
            Some(Difference {
                position: site.position,
                a: site,
                b: other,
                points: (site.share() - other.share()) * 100.0,
                p: fisher([
                    [site.modified, site.total - site.modified],
                    [other.modified, other.total - other.modified],
                ]),
            })
        })
        .collect()
}

/// The sites of several sets added at each position: the counts of a group of files.
pub fn pooled(sets: &[Vec<Site>]) -> Vec<Site> {
    let mut found: BTreeMap<u64, (u32, u32)> = BTreeMap::new();
    for site in sets.iter().flatten() {
        let counts = found.entry(site.position).or_default();
        counts.0 += site.modified;
        counts.1 += site.total;
    }
    found
        .into_iter()
        .map(|(position, (modified, total))| Site {
            position,
            modified,
            total,
        })
        .collect()
}

/// The two-sided p value of Fisher's exact test for a table of counts: the chance of a table
/// with the same sums of rows and columns that is as likely as this one or less likely.
pub fn fisher(table: [[u32; 2]; 2]) -> f64 {
    let [[first, second], [third, fourth]] = table;
    let (row, other, column) = (first + second, third + fourth, first + third);
    let total = row + other;
    if total == 0 {
        return 1.0;
    }
    let mut logs = vec![0.0_f64; total as usize + 1];
    for value in 2..=total as usize {
        logs[value] = logs[value - 1] + (value as f64).ln();
    }
    let choose = |all: u32, some: u32| {
        logs[all as usize] - logs[some as usize] - logs[(all - some) as usize]
    };
    let chance = |count: u32| {
        (choose(row, count) + choose(other, column - count) - choose(total, column)).exp()
    };
    let observed = chance(first);
    let low = column.saturating_sub(other);
    let high = row.min(column);
    let p: f64 = (low..=high)
        .map(chance)
        .filter(|value| *value <= observed * (1.0 + 1e-7))
        .sum();
    p.min(1.0)
}

#[cfg(test)]
mod tests {
    use super::{Site, Tally, difference, fisher, pooled};
    use crate::alignment::{Alignment, Op, REVERSE, Tags};
    use crate::modification::{Call, Code, Modifications};

    #[test]
    fn fisher_tests_give_the_known_two_sided_values() {
        let near = |found: f64, wanted: f64| (found - wanted).abs() < 1e-6;
        let tea = fisher([[3, 1], [1, 3]]);
        assert!(near(tea, 0.485_714), "{tea}");
        let known = fisher([[1, 9], [11, 3]]);
        assert!(near(known, 0.002_759), "{known}");
        assert!(near(fisher([[10, 0], [0, 10]]), 2.0 / 184_756.0));
        assert!(near(fisher([[5, 5], [5, 5]]), 1.0));
        assert!(near(fisher([[0, 0], [0, 0]]), 1.0));
    }

    /// The groups differ at the sites with reads in both groups. A minus B, in percentage points.
    #[test]
    fn differences_use_the_sites_of_both_groups() {
        let a = [site(10, 9, 10), site(20, 1, 2), site(30, 0, 0)];
        let b = [site(10, 1, 10), site(25, 1, 2), site(30, 3, 4)];
        let found = difference(&a, &b);
        assert_eq!(found.len(), 1);
        let first = found[0];
        assert_eq!((first.position, first.a, first.b), (10, a[0], b[0]));
        assert!((first.points - 80.0).abs() < 1e-4);
        assert!((first.p - 202.0 / 184_756.0).abs() < 1e-9, "{}", first.p);
    }

    #[test]
    fn groups_add_their_counts() {
        let one = vec![site(10, 2, 4), site(20, 1, 1)];
        let two = vec![site(10, 3, 6)];
        assert_eq!(pooled(&[one, two]), [site(10, 5, 10), site(20, 1, 1)]);
    }

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
    fn ties_go_to_5mc() {
        let mut dorado = read(100, 0, &[(1, 0, 10), (1, 1, 200)]);
        dorado.tags.mods.codes.reverse();
        let mut tally = Tally::default();
        tally.add(&dorado, 0);
        let methylation = tally.finish(base);
        assert_eq!(methylation.names(), ["5hmC", "5mC"]);
        assert_eq!(methylation.busiest(), Some("5mC"));
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
