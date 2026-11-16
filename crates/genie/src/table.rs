//! The variant table: a row for each variant, and a column for each of its fields.

use std::cmp::Ordering;
use std::collections::HashMap;

use genie_core::Region;
use genie_core::variant::{Call, Variant};
use genie_formats::fields::Field;

/// The columns of each variant, before the counts of the calls and the fields.
const FIXED: [&str; 5] = ["Chrom", "Pos", "ID", "Ref", "Alt"];
/// The columns that count the calls of the samples.
const CALLS: [&str; 2] = ["Het", "Hom alt"];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub span: Region,
    pub cells: Vec<String>,
}

/// The value of a cell for a sort: numbers in order, then text without letter case, then the
/// empty cells.
#[derive(Debug, PartialEq)]
enum Key {
    Number(f64),
    Text(String),
    Empty,
}

impl Key {
    fn of(cell: &str) -> Self {
        let cell = cell.trim();
        if matches!(cell, "" | ".") {
            return Self::Empty;
        }
        match cell.parse::<f64>() {
            Ok(number) if number.is_finite() => Self::Number(number),
            _ => Self::Text(cell.to_lowercase()),
        }
    }

    fn rank(&self) -> u8 {
        match self {
            Self::Number(_) => 0,
            Self::Text(_) => 1,
            Self::Empty => 2,
        }
    }

    /// The order of two keys. A descending sort turns numbers and text around, and keeps the
    /// empty cells last.
    fn compare(&self, other: &Self, descending: bool) -> Ordering {
        let order = match (self, other) {
            (Self::Number(a), Self::Number(b)) => a.total_cmp(b),
            (Self::Text(a), Self::Text(b)) => a.cmp(b),
            _ => return self.rank().cmp(&other.rank()),
        };
        if descending { order.reverse() } else { order }
    }
}

impl Table {
    /// The table of the variants of a contig, with the fields that `fields` gives each variant.
    /// With samples, two columns count the heterozygous and the homozygous alternate calls.
    pub fn new(
        contig: &str,
        variants: &[Variant],
        samples: bool,
        fields: impl Fn(&Variant) -> Vec<Field>,
    ) -> Self {
        let mut columns: Vec<String> = FIXED.iter().map(|name| (*name).to_owned()).collect();
        if samples {
            columns.extend(CALLS.iter().map(|name| (*name).to_owned()));
        }
        let mut places: HashMap<String, usize> = HashMap::new();
        let mut rows = Vec::with_capacity(variants.len());
        for variant in variants {
            let id = if variant.id == "." { "" } else { &variant.id };
            let mut cells = vec![
                contig.to_owned(),
                (variant.span.start() + 1).to_string(),
                id.to_owned(),
                variant.reference.clone(),
                variant.alternates.join(","),
            ];
            if samples {
                let count = |call: Call| {
                    let found = variant.calls.iter().filter(|known| **known == call).count();
                    found.to_string()
                };
                cells.push(count(Call::Het));
                cells.push(count(Call::Alternate));
            }
            for field in fields(variant) {
                let at = *places.entry(field.label.clone()).or_insert_with(|| {
                    columns.push(field.label);
                    columns.len() - 1
                });
                if cells.len() <= at {
                    cells.resize(at + 1, String::new());
                }
                cells[at] = field.value;
            }
            rows.push(Row {
                span: variant.span,
                cells,
            });
        }
        for row in &mut rows {
            row.cells.resize(columns.len(), String::new());
        }
        Self { columns, rows }
    }

    #[cfg(test)]
    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|known| known == name)
    }

    /// The rows in a region that have a cell with the search text, ignoring letter case, in the
    /// order of a column, or in the order of the file without a sort.
    pub fn order(&self, within: Region, sort: Option<(usize, bool)>, search: &str) -> Vec<usize> {
        let search = search.trim().to_lowercase();
        let found = |row: &Row| {
            search.is_empty()
                || row
                    .cells
                    .iter()
                    .any(|cell| cell.to_lowercase().contains(&search))
        };
        let mut order: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                let span =
                    Region::between(row.span.start(), row.span.end().max(row.span.start() + 1));
                span.overlaps(within) && found(row)
            })
            .map(|(at, _)| at)
            .collect();
        if let Some((column, descending)) = sort {
            let keys: Vec<Key> = self
                .rows
                .iter()
                .map(|row| {
                    row.cells
                        .get(column)
                        .map_or(Key::Empty, |cell| Key::of(cell))
                })
                .collect();
            order.sort_by(|a, b| keys[*a].compare(&keys[*b], descending));
        }
        order
    }

    /// The rows in an order as text with a tab between the cells and a header line.
    pub fn tsv(&self, order: &[usize]) -> String {
        let clean = |cell: &str| cell.replace(['\t', '\n', '\r'], " ");
        let line = |cells: &[String]| {
            let cells: Vec<String> = cells.iter().map(|cell| clean(cell)).collect();
            cells.join("\t") + "\n"
        };
        let mut text = line(&self.columns);
        for &at in order {
            text.push_str(&line(&self.rows[at].cells));
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use genie_core::Region;
    use genie_core::variant::{Annotations, Call, Variant};
    use genie_formats::fields::Field;

    use super::Table;

    const ALL: Region = Region::between(0, u64::MAX);

    fn site(start: u64, id: &str, info: &str, calls: Vec<Call>) -> Variant {
        Variant {
            id: id.to_owned(),
            span: Region::between(start, start + 1),
            reference: "A".to_owned(),
            alternates: vec!["G".to_owned(), "T".to_owned()],
            quality: None,
            filter: "PASS".to_owned(),
            info: info.to_owned(),
            annotations: Annotations::default(),
            structure: None,
            calls,
        }
    }

    fn fields(variant: &Variant) -> Vec<Field> {
        variant
            .info
            .split(';')
            .filter_map(|entry| {
                let (label, value) = entry.split_once('=')?;
                Some(Field {
                    label: label.to_owned(),
                    value: value.to_owned(),
                })
            })
            .collect()
    }

    #[test]
    fn variants_become_rows_with_a_column_for_each_field() {
        let calls = vec![Call::Het, Call::Alternate, Call::Reference];
        let variants = [
            site(99, "rs1", "AF=0.5;DP=30", calls),
            site(
                9,
                ".",
                "DP=4;GENE=BRCA1",
                vec![Call::Missing, Call::Het, Call::Het],
            ),
        ];
        let table = Table::new("chr1", &variants, true, fields);
        let columns = [
            "Chrom", "Pos", "ID", "Ref", "Alt", "Het", "Hom alt", "AF", "DP", "GENE",
        ];
        assert_eq!(table.columns, columns);
        let first = ["chr1", "100", "rs1", "A", "G,T", "1", "1", "0.5", "30", ""];
        assert_eq!(table.rows[0].cells, first);
        let second = ["chr1", "10", "", "A", "G,T", "2", "0", "", "4", "BRCA1"];
        assert_eq!(table.rows[1].cells, second);
        assert_eq!(table.rows[1].span, Region::between(9, 10));
        assert_eq!(table.column("GENE"), Some(9));
        let plain = Table::new("chr1", &variants, false, fields);
        assert_eq!(plain.column("Het"), None);
        assert_eq!(plain.column("AF"), Some(5));
    }

    #[test]
    fn rows_sort_by_a_column_and_match_a_search() {
        let variants = [
            site(10, "rs3", "DP=100;NOTE=b", Vec::new()),
            site(20, "rs1", "DP=9;NOTE=A", Vec::new()),
            site(30, "rs2", "NOTE=c", Vec::new()),
            site(40, "rs4", "DP=25.5", Vec::new()),
        ];
        let table = Table::new("chr1", &variants, false, fields);
        let (dp, note) = (table.column("DP").unwrap(), table.column("NOTE").unwrap());
        assert_eq!(table.order(ALL, None, ""), [0, 1, 2, 3]);
        assert_eq!(table.order(ALL, Some((dp, false)), ""), [1, 3, 0, 2]);
        assert_eq!(table.order(ALL, Some((dp, true)), ""), [0, 3, 1, 2]);
        assert_eq!(table.order(ALL, Some((note, false)), ""), [1, 0, 2, 3]);
        assert_eq!(table.order(ALL, Some((note, true)), ""), [2, 0, 1, 3]);
        assert_eq!(table.order(ALL, None, " RS2 "), [2]);
        assert_eq!(table.order(Region::between(15, 31), None, ""), [1, 2]);
        assert_eq!(
            table.order(Region::between(15, 31), Some((dp, true)), "rs"),
            [1, 2]
        );
    }

    #[test]
    fn tables_write_tab_separated_text() {
        let variants = [site(0, "rs1", "NOTE=a\tb", Vec::new())];
        let table = Table::new("chr1", &variants, false, fields);
        assert_eq!(
            table.tsv(&[0]),
            "Chrom\tPos\tID\tRef\tAlt\tNOTE\nchr1\t1\trs1\tA\tG,T\ta b\n"
        );
    }
}
