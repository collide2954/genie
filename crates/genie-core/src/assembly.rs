use std::collections::HashMap;
use std::collections::hash_map::Entry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contig {
    pub name: String,
    pub length: u64,
}

#[derive(Debug, Clone)]
pub struct Assembly {
    contigs: Vec<Contig>,
    names: HashMap<String, usize>,
    aliases: HashMap<String, Option<usize>>,
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
        }
    }

    pub fn contigs(&self) -> &[Contig] {
        &self.contigs
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.names
            .get(name)
            .copied()
            .or_else(|| self.aliases.get(&alias(name)).copied().flatten())
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
    use super::{Assembly, Contig};

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
    fn ambiguous_aliases_do_not_match() {
        let assembly = assembly(&["chr1", "1"]);
        assert_eq!(assembly.find("chr1"), Some(0));
        assert_eq!(assembly.find("1"), Some(1));
        assert_eq!(assembly.find("CHR1"), None);
    }
}
