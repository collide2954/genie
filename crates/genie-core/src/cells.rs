//! Cell types of single-cell reads: the type of each cell barcode, from a table of the cells
//! such as the meta.data of Seurat or the obs of Scanpy.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

/// The most cell types with a lane of their own. The cells of the other types share the lane
/// "Other".
pub const LANES: usize = 12;
/// The fewest letters of the core of a barcode.
const CORE: usize = 8;

/// The cell type of each barcode of a table, with the number of cells of each type.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellMap {
    /// The names of the types, the most cells first.
    pub types: Vec<String>,
    /// The number of cells of each type.
    pub cells: Vec<usize>,
    exact: HashMap<String, usize>,
    /// The type of each core of a barcode, or nothing when cells with that core have other
    /// types.
    cores: HashMap<String, Option<usize>>,
}

impl CellMap {
    /// The map of the barcodes of rows with a barcode and a cell type. A row with an empty type
    /// has no type, and only the first row of a barcode counts.
    pub fn new<'a>(rows: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut found: Vec<(&str, &str)> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for (barcode, kind) in rows {
            let kind = kind.trim();
            if kind.is_empty() || !seen.insert(barcode) {
                continue;
            }
            found.push((barcode, kind));
        }
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for &(_, kind) in &found {
            *counts.entry(kind).or_default() += 1;
        }
        let mut order: Vec<(&str, usize)> = counts.into_iter().collect();
        order.sort_by(|(a, first), (b, second)| second.cmp(first).then(a.cmp(b)));
        let place: HashMap<&str, usize> = order
            .iter()
            .enumerate()
            .map(|(at, (kind, _))| (*kind, at))
            .collect();
        let mut exact = HashMap::new();
        let mut cores: HashMap<String, Option<usize>> = HashMap::new();
        for (barcode, kind) in found {
            let kind = place[kind];
            exact.insert(barcode.to_owned(), kind);
            if let Some(core) = core(barcode) {
                match cores.entry(core.to_owned()) {
                    Entry::Vacant(slot) => {
                        slot.insert(Some(kind));
                    }
                    Entry::Occupied(mut slot) => {
                        if *slot.get() != Some(kind) {
                            slot.insert(None);
                        }
                    }
                }
            }
        }
        Self {
            types: order.iter().map(|(kind, _)| (*kind).to_owned()).collect(),
            cells: order.iter().map(|(_, cells)| *cells).collect(),
            exact,
            cores,
        }
    }

    /// The type of a barcode: the type of the same barcode in the table, else the type of the
    /// cells with the same core when they all have one type.
    pub fn type_of(&self, barcode: &str) -> Option<usize> {
        if let Some(&kind) = self.exact.get(barcode) {
            return Some(kind);
        }
        self.cores.get(core(barcode)?).copied().flatten()
    }

    /// The lane of a type: the types with the most cells have a lane each, and the other types
    /// share the last lane.
    pub fn lane(&self, kind: usize) -> usize {
        kind.min(LANES)
    }

    /// The name and the number of cells of each lane.
    pub fn lanes(&self) -> Vec<(String, usize)> {
        let mut lanes: Vec<(String, usize)> = self
            .types
            .iter()
            .zip(&self.cells)
            .take(LANES)
            .map(|(kind, cells)| (kind.clone(), *cells))
            .collect();
        if self.types.len() > LANES {
            lanes.push(("Other".to_owned(), self.cells[LANES..].iter().sum()));
        }
        lanes
    }
}

/// The core of a barcode: its longest run of A, C, G and T, when the run has 8 letters or more.
/// Barcodes with a sample prefix or another suffix, as Seurat and Scanpy write them, have the
/// same core as the barcode of the reads.
pub fn core(barcode: &str) -> Option<&str> {
    let mut best = (0, 0);
    let mut from = None;
    for (at, byte) in barcode.bytes().enumerate() {
        if matches!(byte, b'A' | b'C' | b'G' | b'T') {
            let start = *from.get_or_insert(at);
            if at + 1 - start > best.1 - best.0 {
                best = (start, at + 1);
            }
        } else {
            from = None;
        }
    }
    (best.1 - best.0 >= CORE).then(|| &barcode[best.0..best.1])
}

#[cfg(test)]
mod tests {
    use super::{CellMap, LANES, core};

    #[test]
    fn barcodes_find_their_cell_type_also_by_their_core() {
        let map = CellMap::new([
            ("pbmc1_AAACCCAAGAAACACT-1", "B cell"),
            ("pbmc1_AAACCCAAGAAACCAT-1", "T cell"),
            ("pbmc1_AAACCCAAGAAACCCA-1", "T cell"),
            ("pbmc2_AAACCCAAGAAACACT-1", "NK cell"),
            ("TTTGTTGGTCTCAACA-1", ""),
            ("GGGTCACAGTGGTTCT-1", "T cell"),
        ]);
        assert_eq!(map.types, ["T cell", "B cell", "NK cell"]);
        assert_eq!(map.cells, [3, 1, 1]);
        assert_eq!(map.type_of("pbmc1_AAACCCAAGAAACCAT-1"), Some(0));
        assert_eq!(
            map.type_of("AAACCCAAGAAACCAT-1"),
            Some(0),
            "the core without the sample prefix"
        );
        assert_eq!(
            map.type_of("GGGTCACAGTGGTTCT"),
            Some(0),
            "the core without the suffix"
        );
        assert_eq!(
            map.type_of("AAACCCAAGAAACACT-1"),
            None,
            "two cells with this core have other types"
        );
        assert_eq!(map.type_of("pbmc2_AAACCCAAGAAACACT-1"), Some(2));
        assert_eq!(map.type_of("TTTGTTGGTCTCAACA-1"), None, "no value");
        assert_eq!(map.type_of("ACGTACGT"), None, "not in the table");
        assert_eq!(core("pbmc1_AAACCCAAGAAACACT-1-0"), Some("AAACCCAAGAAACACT"));
        assert_eq!(core("AC-1"), None);
    }

    #[test]
    fn the_types_with_the_most_cells_have_lanes_and_the_others_share_one() {
        let rows: Vec<(String, String)> = (0..14)
            .flat_map(|kind| {
                (0..=kind).map(move |cell| (format!("C{kind}_{cell}"), format!("type {kind}")))
            })
            .collect();
        let map = CellMap::new(
            rows.iter()
                .map(|(cell, kind)| (cell.as_str(), kind.as_str())),
        );
        let lanes = map.lanes();
        assert_eq!(lanes.len(), LANES + 1);
        assert_eq!(lanes[0], ("type 13".to_owned(), 14));
        assert_eq!(lanes[LANES], ("Other".to_owned(), 3), "types 1 and 0");
        let lane = |cell: &str| map.type_of(cell).map(|kind| map.lane(kind));
        assert_eq!(lane("C13_0"), Some(0));
        assert_eq!(lane("C2_0"), Some(LANES - 1));
        assert_eq!(lane("C0_0"), Some(LANES));
        let few = CellMap::new([("C1", "a"), ("C2", "b")]);
        assert_eq!(
            few.lanes(),
            [("a".to_owned(), 1), ("b".to_owned(), 1)],
            "no Other lane"
        );
    }
}
