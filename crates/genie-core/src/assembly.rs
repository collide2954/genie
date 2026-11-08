use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::{Locus, LocusError, Span};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contig {
    pub name: String,
    pub length: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub contig: usize,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembly {
    contigs: Vec<Contig>,
    names: HashMap<String, usize>,
    aliases: HashMap<String, Option<usize>>,
    extra: HashMap<String, usize>,
}

impl Assembly {
    pub fn new(contigs: Vec<Contig>) -> Self {
        let mut names = HashMap::with_capacity(contigs.len());
        let mut aliases = HashMap::with_capacity(contigs.len());
        for (index, contig) in contigs.iter().enumerate() {
            names.entry(contig.name.clone()).or_insert(index);
            match aliases.entry(alias(&contig.name)) {
                Entry::Vacant(entry) => {
                    entry.insert(Some(index));
                }
                Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
        Self {
            contigs,
            names,
            aliases,
            extra: HashMap::new(),
        }
    }

    #[must_use]
    pub fn with_aliases(mut self, rows: &[Vec<String>]) -> Self {
        for row in rows {
            let Some(index) = row.iter().find_map(|name| self.names.get(name).copied()) else {
                continue;
            };
            for name in row {
                if !self.names.contains_key(name) {
                    self.extra.entry(name.clone()).or_insert(index);
                }
            }
        }
        self
    }

    pub fn contigs(&self) -> &[Contig] {
        &self.contigs
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.names
            .get(name)
            .or_else(|| self.extra.get(name))
            .copied()
            .or_else(|| self.aliases.get(&alias(name)).copied().flatten())
    }

    pub fn suggest(&self, prefix: &str, limit: usize) -> Vec<usize> {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            return Vec::new();
        }
        let lower = prefix.to_ascii_lowercase();
        let key = alias(prefix);
        self.contigs
            .iter()
            .enumerate()
            .filter(|(_, contig)| {
                contig.name.to_ascii_lowercase().starts_with(&lower)
                    || alias(&contig.name).starts_with(&key)
            })
            .map(|(index, _)| index)
            .take(limit)
            .collect()
    }

    pub fn resolve(&self, text: &str) -> Result<Target, LocusError> {
        if let Some(contig) = self.find(text.trim()) {
            return Ok(Target {
                contig,
                span: Span::Whole,
            });
        }
        let locus: Locus = text.parse()?;
        let contig = self
            .find(&locus.name)
            .ok_or(LocusError::UnknownContig(locus.name))?;
        let Contig { name, length } = &self.contigs[contig];
        let span = match locus.span {
            Span::Whole => Span::Whole,
            Span::Position(position) if position < *length => Span::Position(position),
            Span::Region(region) if region.start() < *length => {
                Span::Region(region.clamp_to(*length))
            }
            _ => {
                return Err(LocusError::PastEnd {
                    name: name.clone(),
                    length: *length,
                });
            }
        };
        Ok(Target { contig, span })
    }
}

fn alias(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let bare = lower
        .strip_prefix("chr")
        .filter(|rest| !rest.is_empty())
        .unwrap_or(&lower);
    if bare == "m" {
        "mt".to_owned()
    } else {
        bare.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{Assembly, Contig, Target};
    use crate::{LocusError, Region, Span};

    fn assembly(names: &[&str]) -> Assembly {
        Assembly::new(
            names
                .iter()
                .map(|name| Contig {
                    name: (*name).to_owned(),
                    length: 1_000,
                })
                .collect(),
        )
    }

    #[test]
    fn finds_exact_names() {
        let assembly = assembly(&["chr1", "chr2"]);
        assert_eq!(assembly.find("chr2"), Some(1));
        assert_eq!(assembly.find("chr3"), None);
        assert_eq!(assembly.contigs()[1].name, "chr2");
    }

    #[test]
    fn chr_prefix_is_optional() {
        assert_eq!(assembly(&["chr1"]).find("1"), Some(0));
        assert_eq!(assembly(&["1"]).find("chr1"), Some(0));
    }

    #[test]
    fn mitochondrion_names_match() {
        for (contig, query) in [("chrM", "MT"), ("MT", "chrM"), ("MT", "M")] {
            assert_eq!(assembly(&[contig]).find(query), Some(0), "{query}");
        }
    }

    #[test]
    fn alias_match_ignores_case() {
        assert_eq!(assembly(&["chrX"]).find("CHRX"), Some(0));
    }

    #[test]
    fn resolves_names_and_spans() {
        let assembly = assembly(&["chr1", "chr2"]);
        assert_eq!(
            assembly.resolve("chr2"),
            Ok(Target {
                contig: 1,
                span: Span::Whole
            })
        );
        assert_eq!(
            assembly.resolve("1:100"),
            Ok(Target {
                contig: 0,
                span: Span::Position(99)
            })
        );
    }

    #[test]
    fn resolve_clamps_range_end() {
        let span = Span::Region(Region::new(899, 1_000).unwrap());
        assert_eq!(
            assembly(&["chr1"]).resolve("chr1:900-2,000"),
            Ok(Target { contig: 0, span })
        );
    }

    #[test]
    fn whole_name_with_colon_wins() {
        assert_eq!(
            assembly(&["HLA-A*01:01"]).resolve("HLA-A*01:01"),
            Ok(Target {
                contig: 0,
                span: Span::Whole
            })
        );
    }

    #[test]
    fn resolve_rejects_unknown_contigs_and_positions_past_end() {
        let assembly = assembly(&["chr1"]);
        assert_eq!(
            assembly.resolve("chr9:1-10"),
            Err(LocusError::UnknownContig("chr9".into()))
        );
        let past_end = LocusError::PastEnd {
            name: "chr1".into(),
            length: 1_000,
        };
        assert_eq!(assembly.resolve("chr1:1,001"), Err(past_end.clone()));
        assert_eq!(assembly.resolve("chr1:1,001-2,000"), Err(past_end.clone()));
        assert_eq!(past_end.to_string(), "chr1 ends at 1,000.");
    }

    #[test]
    fn suggests_contigs_by_name_prefix_in_assembly_order() {
        let assembly = assembly(&["chr1", "chr2", "chr10", "chrX", "chrM"]);
        assert_eq!(assembly.suggest("chr1", 8), [0, 2]);
        assert_eq!(assembly.suggest("CHRX", 8), [3]);
        assert_eq!(assembly.suggest("c", 8), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn suggests_contigs_by_alias() {
        let assembly = assembly(&["chr1", "chr2", "chr10", "chrX", "chrM"]);
        assert_eq!(assembly.suggest("1", 8), [0, 2]);
        assert_eq!(assembly.suggest("x", 8), [3]);
        assert_eq!(assembly.suggest("MT", 8), [4]);
    }

    #[test]
    fn suggestions_respect_the_limit_and_need_text() {
        let assembly = assembly(&["chr1", "chr2", "chr10"]);
        assert_eq!(assembly.suggest("chr", 2), [0, 1]);
        assert_eq!(assembly.suggest("  ", 8), []);
    }

    #[test]
    fn assemblies_with_the_same_contigs_are_equal() {
        assert_eq!(assembly(&["chr1", "chr2"]), assembly(&["chr1", "chr2"]));
        assert_ne!(assembly(&["chr1"]), assembly(&["chr2"]));
    }

    #[test]
    fn alias_tables_find_contigs_by_other_names() {
        let row = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect();
        let rows: Vec<Vec<String>> = vec![
            row(&["chr1", "1", "CM000663.2", "NC_000001.11"]),
            row(&["chrUn_x", "GL000001.1"]),
            row(&["NC_000002.12", "chr2", "CM000664.2"]),
        ];
        let assembly = assembly(&["chr1", "chr2", "CM000664.2"]).with_aliases(&rows);
        assert_eq!(assembly.find("NC_000001.11"), Some(0));
        assert_eq!(assembly.find("CM000663.2"), Some(0));
        assert_eq!(assembly.find("NC_000002.12"), Some(1));
        assert_eq!(assembly.find("CM000664.2"), Some(2));
        assert_eq!(assembly.find("GL000001.1"), None);
        assert_eq!(assembly.find("1"), Some(0));
    }

    #[test]
    fn ambiguous_aliases_do_not_match() {
        let assembly = assembly(&["chr1", "1"]);
        assert_eq!(assembly.find("chr1"), Some(0));
        assert_eq!(assembly.find("1"), Some(1));
        assert_eq!(assembly.find("CHR1"), None);
    }
}
