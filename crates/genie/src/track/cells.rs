//! The cell table of a single-cell track: the cell type of each barcode of its reads or
//! fragments, from a column of a table of the cells.

use std::sync::Arc;

use genie_core::alignment::Alignment;
use genie_core::cells::{CellMap, Depths};
use genie_core::samples::Table;

use crate::location::Location;

/// The most values of a column that gives cell types.
const MOST_TYPES: usize = 100;
/// Words in the names of the columns that give cell types, the best first, compared without
/// case.
const TYPE_WORDS: [&str; 11] = [
    "celltype",
    "cell_type",
    "cell.type",
    "cell type",
    "annotation",
    "predicted",
    "label",
    "cluster",
    "leiden",
    "louvain",
    "ident",
];

/// The cell table of a track: its file, its table, the column of the cell types, the columns
/// that can give cell types, and the cell type of each barcode.
#[derive(Debug, Clone)]
pub struct Cells {
    pub location: Location,
    pub table: Arc<Table>,
    pub column: usize,
    /// The columns with 2 to 100 values.
    pub columns: Vec<usize>,
    pub map: Arc<CellMap>,
}

impl Cells {
    /// The cell types of a column of a table, or of the column whose name says cell type best
    /// when `column` is `None`. Nothing when no column has 2 to 100 values.
    pub fn new(location: Location, table: Arc<Table>, column: Option<usize>) -> Option<Self> {
        let columns: Vec<usize> = (0..table.columns.len())
            .filter(|&column| (2..=MOST_TYPES).contains(&table.distinct(column, MOST_TYPES + 1)))
            .collect();
        let column = column
            .filter(|column| columns.contains(column))
            .or_else(|| best_column(&table, &columns))?;
        let map = Arc::new(CellMap::new(table.entries(column)));
        Some(Self {
            location,
            table,
            column,
            columns,
            map,
        })
    }

    /// The name of the column of the cell types.
    pub fn column_name(&self) -> &str {
        &self.table.columns[self.column]
    }

    /// The same table with the cell types of another column.
    #[must_use]
    pub fn with_column(&self, column: usize) -> Self {
        Self::new(self.location.clone(), Arc::clone(&self.table), Some(column))
            .unwrap_or_else(|| self.clone())
    }
}

/// The coverage of the lanes of cell types: the name and the number of cells of each lane, and
/// the depths of its reads or fragments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellLanes {
    pub lanes: Vec<(String, usize)>,
    pub depths: Depths,
}

impl CellLanes {
    /// Empty lanes for the cell types of a map in a region.
    pub fn new(map: &CellMap, region: genie_core::Region) -> Self {
        let lanes = map.lanes();
        Self {
            depths: Depths::new(region, lanes.len()),
            lanes,
        }
    }
}

/// The column whose name says cell type best, else the first column.
fn best_column(table: &Table, columns: &[usize]) -> Option<usize> {
    TYPE_WORDS
        .iter()
        .find_map(|word| {
            columns
                .iter()
                .copied()
                .find(|&column| table.columns[column].to_lowercase().contains(word))
        })
        .or_else(|| columns.first().copied())
}

/// The barcode of the cell of a read: its CB tag, else its XC tag, as Drop-seq writes it.
pub fn barcode(alignment: &Alignment) -> Option<&str> {
    alignment
        .tags
        .get(*b"CB")
        .or_else(|| alignment.tags.get(*b"XC"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::samples::Table;

    use super::Cells;

    fn cell_table(columns: &[&str], rows: &[(&str, &[&str])]) -> Arc<Table> {
        Arc::new(Table::new(
            columns.iter().map(|name| (*name).to_owned()).collect(),
            rows.iter()
                .map(|(name, values)| {
                    let values = values.iter().map(|value| (*value).to_owned()).collect();
                    ((*name).to_owned(), values)
                })
                .collect(),
        ))
    }

    #[test]
    fn cell_tables_pick_the_column_that_names_cell_types() {
        let table = cell_table(
            &["orig.ident", "nCount_RNA", "seurat_clusters", "cell_type"],
            &[
                ("AAACCCAAGAAACACT-1", &["pbmc", "4520", "0", "CD4 T"]),
                ("AAACCCAAGAAACCAT-1", &["pbmc", "3011", "2", "B"]),
                ("AAACCCAAGAAACCCA-1", &["pbmc", "2800", "0", "CD4 T"]),
            ],
        );
        let location = PathBuf::from("/data/meta.csv").into();
        let cells = Cells::new(location, Arc::clone(&table), None).unwrap();
        assert_eq!(cells.columns, [1, 2, 3], "orig.ident has one value");
        assert_eq!(cells.column_name(), "cell_type");
        assert_eq!(cells.map.types, ["CD4 T", "B"]);
        let clusters = cells.with_column(2);
        assert_eq!(clusters.column_name(), "seurat_clusters");
        assert_eq!(clusters.map.types, ["0", "2"]);
        let location = PathBuf::from("/data/one.csv").into();
        let one = cell_table(&["sample"], &[("AAACCCAAGAAACACT-1", &["pbmc"])]);
        assert!(
            Cells::new(location, one, None).is_none(),
            "no column with 2 values or more"
        );
    }
}
