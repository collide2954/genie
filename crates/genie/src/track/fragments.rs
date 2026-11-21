//! Fragment files of single-cell ATAC-seq as tracks: the coverage of the fragments of each cell
//! type in lanes, from the cell table of the track, or of all fragments in one lane.

use std::io::{BufRead, BufReader};
use std::sync::Arc;

use genie_core::Region;
use genie_core::cells::{CellMap, Depths};
use genie_formats::bgzf::Bgzf;
use genie_formats::table::Table;
use genie_formats::{fragments, gzip, tabix};

use super::{CellLanes, Loaded, has_index, is_compressed, tabix_index};
use crate::location::Location;

/// The largest fragment file without an index that genie reads whole.
const WHOLE_LIMIT: u64 = 256 << 20;
/// The widest view that shows the fragments of a file on this computer, and of a file on the
/// web.
const SPAN: u64 = 2_000_000;
const WEB_SPAN: u64 = 500_000;
/// The name of the lane of all fragments, without a cell table.
pub const ALL: &str = "Fragments";

/// A fragment file: with a tabix index, or small and in memory.
#[derive(Debug)]
pub enum FragmentFile {
    Indexed { index: tabix::Index, remote: bool },
    Table(Table<(Region, String)>),
}

impl FragmentFile {
    pub fn open(location: &Location, name: &str, index: Option<&Location>) -> Result<Self, String> {
        if index.is_some() || (is_compressed(name) && has_index(location)) {
            return Ok(Self::Indexed {
                index: tabix_index(location, name, index)?,
                remote: location.is_remote(),
            });
        }
        if location.size()? > WHOLE_LIMIT {
            return Err(format!(
                "The file {name} is larger than 256 MB. Compress it with bgzip and index it with \
                 tabix -p bed."
            ));
        }
        let file = BufReader::new(location.open()?);
        let reader: Box<dyn BufRead> = if is_compressed(name) {
            Box::new(BufReader::new(gzip::Reader::new(file)))
        } else {
            Box::new(file)
        };
        fragments::read_table(reader)
            .map(Self::Table)
            .map_err(|error| format!("The file {name} is not valid. {error}"))
    }

    pub fn contigs(&self) -> &[String] {
        match self {
            Self::Indexed { index, .. } => index.names(),
            Self::Table(table) => table.contigs(),
        }
    }

    /// The widest view that shows the fragments.
    pub fn span(&self) -> u64 {
        match self {
            Self::Indexed { remote: true, .. } => WEB_SPAN,
            _ => SPAN,
        }
    }

    /// The coverage of the fragments of a region: a lane for each of the largest cell types of
    /// a cell table, or one lane of all fragments without one.
    pub fn load(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
        cells: Option<&Arc<CellMap>>,
    ) -> Result<Loaded, String> {
        let mut lanes = match cells {
            Some(map) => CellLanes::new(map, region),
            None => CellLanes {
                lanes: vec![(ALL.to_owned(), 0)],
                depths: Depths::new(region, 1),
            },
        };
        let mut add = |span: Region, barcode: &str| match cells {
            Some(map) => {
                if let Some(kind) = map.type_of(barcode) {
                    lanes.depths.add_span(map.lane(kind), span);
                }
            }
            None => lanes.depths.add_span(0, span),
        };
        match self {
            Self::Indexed { index, .. } => {
                let mut reader = Bgzf::new(location.open()?);
                fragments::query(index, &mut reader, (contig, region), &mut add)
                    .map_err(|error| error.to_string())?;
            }
            Self::Table(table) => {
                for (span, barcode) in table.query(contig, region) {
                    add(span, &barcode);
                }
            }
        }
        Ok(Loaded::cells(region, lanes))
    }
}
