use std::collections::HashMap;

use genie_core::Region;

#[derive(Debug)]
pub struct Table<T> {
    names: Vec<String>,
    shelves: Vec<Shelf<T>>,
}

#[derive(Debug)]
struct Shelf<T> {
    items: Vec<(Region, T)>,
    longest: u64,
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Self {
            names: Vec::new(),
            shelves: Vec::new(),
        }
    }
}

impl<T: Clone> Table<T> {
    pub fn new(rows: impl IntoIterator<Item = (String, Region, T)>) -> Self {
        let mut table = Self::default();
        let mut slots = HashMap::new();
        for (contig, span, item) in rows {
            let slot = *slots.entry(contig).or_insert_with_key(|contig| {
                table.names.push(contig.clone());
                table.shelves.push(Shelf {
                    items: Vec::new(),
                    longest: 0,
                });
                table.names.len() - 1
            });
            table.shelves[slot].items.push((span, item));
        }
        for shelf in &mut table.shelves {
            shelf.items.sort_by_key(|(span, _)| span.start());
            shelf.longest = shelf
                .items
                .iter()
                .map(|(span, _)| span.len())
                .max()
                .unwrap_or(0);
        }
        table
    }

    pub fn contigs(&self) -> &[String] {
        &self.names
    }

    pub fn query(&self, contig: &str, region: Region) -> Vec<T> {
        let Some(slot) = self.names.iter().position(|name| name == contig) else {
            return Vec::new();
        };
        let rows = &self.shelves[slot];
        let from = region.start().saturating_sub(rows.longest);
        let first = rows.items.partition_point(|(span, _)| span.start() < from);
        rows.items[first..]
            .iter()
            .take_while(|(span, _)| span.start() < region.end())
            .filter(|(span, _)| span.overlaps(region))
            .map(|(_, item)| item.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use genie_core::Region;

    use super::Table;

    fn row(
        contig: &str,
        start: u64,
        end: u64,
        name: &'static str,
    ) -> (String, Region, &'static str) {
        (contig.to_owned(), Region::between(start, end), name)
    }

    #[test]
    fn tables_sort_rows_by_contig_and_start() {
        let table = Table::new([
            row("chr2", 5, 9, "z"),
            row("chr1", 30, 40, "b"),
            row("chr1", 0, 1_000, "long"),
            row("chr1", 10, 20, "a"),
        ]);
        assert_eq!(table.contigs(), ["chr2", "chr1"]);
        assert_eq!(table.query("chr1", Region::between(25, 45)), ["long", "b"]);
        assert_eq!(table.query("chr1", Region::between(500, 600)), ["long"]);
        assert_eq!(table.query("chrX", Region::between(0, 10)), [] as [&str; 0]);
    }
}
