use crate::Region;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub name: String,
    pub contig: usize,
    pub span: Region,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Names {
    entries: Vec<(String, Named)>,
}

impl Names {
    pub fn new(entries: impl IntoIterator<Item = Named>) -> Self {
        let mut entries: Vec<(String, Named)> = entries
            .into_iter()
            .map(|named| (named.name.to_lowercase(), named))
            .collect();
        entries.sort_by(|(a, x), (b, y)| {
            a.cmp(b)
                .then(x.contig.cmp(&y.contig))
                .then(x.span.start().cmp(&y.span.start()))
        });
        entries.dedup_by(|(key, next), (kept_key, kept)| {
            let same = key == kept_key
                && next.contig == kept.contig
                && next.span.start() <= kept.span.end();
            if same {
                kept.span = kept.span.hull(next.span);
            }
            same
        });
        Self { entries }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn search(&self, prefix: &str, limit: usize) -> impl Iterator<Item = &Named> {
        let key = prefix.trim().to_lowercase();
        let start = if key.is_empty() {
            self.entries.len()
        } else {
            self.first(&key)
        };
        self.entries[start..]
            .iter()
            .take_while(move |(name, _)| name.starts_with(key.as_str()))
            .take(limit)
            .map(|(_, named)| named)
    }

    pub fn find(&self, name: &str) -> Option<&Named> {
        let key = name.trim().to_lowercase();
        self.entries
            .get(self.first(&key))
            .filter(|(entry, _)| *entry == key)
            .map(|(_, named)| named)
    }

    fn first(&self, key: &str) -> usize {
        self.entries
            .partition_point(|(name, _)| name.as_str() < key)
    }
}

#[cfg(test)]
mod tests {
    use super::{Named, Names};
    use crate::Region;

    fn named(name: &str, contig: usize, start: u64, end: u64) -> Named {
        Named {
            name: name.to_owned(),
            contig,
            span: Region::new(start, end).unwrap(),
        }
    }

    fn found<'a>(hits: impl Iterator<Item = &'a Named>) -> Vec<&'a str> {
        hits.map(|named| named.name.as_str()).collect()
    }

    fn names() -> Names {
        Names::new([
            named("TP53", 0, 5, 10),
            named("BRCA2", 1, 50, 60),
            named("BRCA1", 0, 100, 200),
        ])
    }

    #[test]
    fn searches_by_prefix_without_case() {
        let names = names();
        assert_eq!(found(names.search("brc", 8)), ["BRCA1", "BRCA2"]);
        assert_eq!(found(names.search(" Tp", 8)), ["TP53"]);
        assert_eq!(found(names.search("", 8)), [] as [&str; 0]);
        assert_eq!(found(names.search("brc", 1)), ["BRCA1"]);
    }

    #[test]
    fn finds_exact_names() {
        let names = names();
        assert_eq!(names.find("tp53"), Some(&named("TP53", 0, 5, 10)));
        assert_eq!(names.find("TP5"), None);
    }

    #[test]
    fn merges_overlapping_copies_of_a_name() {
        let names = Names::new([
            named("BRCA1", 0, 100, 200),
            named("BRCA1", 0, 1_000, 1_100),
            named("BRCA1", 0, 150, 300),
        ]);
        let spans: Vec<Region> = names.search("brca1", 8).map(|named| named.span).collect();
        assert_eq!(
            spans,
            [
                Region::new(100, 300).unwrap(),
                Region::new(1_000, 1_100).unwrap()
            ]
        );
        assert_eq!(names.len(), 2);
    }
}
