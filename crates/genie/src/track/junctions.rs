use std::cmp::Reverse;
use std::io::{BufRead, BufReader};

use genie_core::Region;
use genie_formats::gzip;
use genie_formats::junctions::{self, Layout, Splice};
use genie_formats::table::Table;

use super::{Loaded, PLAIN_LIMIT, SNIFF_LINES, head, is_bed, is_compressed};
use crate::location::Location;

/// The most junctions that a view shows: those with the most reads.
pub const MOST: usize = 5_000;
/// The height of a junction track until it is resized.
pub const HEIGHT: f32 = 48.0;

/// The layout of a junction file: by its name, or a BED file with the track line of a junction
/// file.
pub fn layout(location: &Location, name: &str) -> Option<Layout> {
    Layout::of(name).or_else(|| {
        let sniffed = is_bed(name)
            && head(location, SNIFF_LINES)
                .iter()
                .take_while(|line| line.is_empty() || line.starts_with(['#', 't', 'b']))
                .any(|line| junctions::is_junction_track(line));
        sniffed.then_some(Layout::Bed)
    })
}

/// A track of splice junctions in memory: a `SJ.out.tab` file of STAR or a junction BED file.
#[derive(Debug)]
pub struct JunctionFile {
    table: Table<Splice>,
}

impl JunctionFile {
    /// Reads a junction file, plain or compressed, of 50 MB or less.
    pub fn open(location: &Location, name: &str, layout: Layout) -> Result<Self, String> {
        if location.size()? > PLAIN_LIMIT {
            return Err(format!(
                "The file {name} is larger than 50 MB. genie reads junction files of 50 MB or less."
            ));
        }
        let file = BufReader::new(location.open()?);
        let reader: Box<dyn BufRead> = if is_compressed(name) {
            Box::new(BufReader::new(gzip::Reader::new(file)))
        } else {
            Box::new(file)
        };
        let table = junctions::read_table(reader, layout)
            .map_err(|error| format!("The file {name} is not valid. {error}"))?;
        Ok(Self { table })
    }

    pub fn contigs(&self) -> &[String] {
        self.table.contigs()
    }

    /// The junctions of a region: all of them, or the `MOST` with the most reads.
    pub fn load(&self, contig: &str, region: Region) -> Loaded {
        let mut found = self.table.query(contig, region);
        if found.len() > MOST {
            found.sort_by_key(|splice| Reverse(splice.junction.count));
            found.truncate(MOST);
            found.sort_by_key(|splice| (splice.junction.span.start(), splice.junction.span.end()));
        }
        Loaded::junctions(region, found)
    }
}
