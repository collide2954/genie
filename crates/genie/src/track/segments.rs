use std::io::{BufRead, BufReader};

use genie_core::Region;
use genie_formats::gzip;
use genie_formats::seg::{self, Segments};

use super::{Loaded, PLAIN_LIMIT, is_compressed};
use crate::location::Location;

/// A copy number segment file in memory: SEG, `CNVkit` or GATK.
#[derive(Debug)]
pub struct SegmentFile {
    pub segments: Segments,
}

impl SegmentFile {
    /// Reads a segment file, plain or compressed, of 50 MB or less.
    pub fn open(location: &Location, name: &str) -> Result<Self, String> {
        if location.size()? > PLAIN_LIMIT {
            return Err(format!(
                "The file {name} is larger than 50 MB. genie reads segment files of 50 MB or less."
            ));
        }
        let file = BufReader::new(location.open()?);
        let reader: Box<dyn BufRead> = if is_compressed(name) {
            Box::new(BufReader::new(gzip::Reader::new(file)))
        } else {
            Box::new(file)
        };
        let segments = seg::read(reader, name)
            .map_err(|error| format!("The file {name} is not valid. {error}"))?;
        Ok(Self { segments })
    }

    pub fn contigs(&self) -> &[String] {
        self.segments.table.contigs()
    }

    pub fn samples(&self) -> &[String] {
        &self.segments.samples
    }

    pub fn load(&self, contig: &str, region: Region) -> Loaded {
        let found = self.segments.table.query(contig, region);
        Loaded::segments(region, found, self.segments.samples.len())
    }
}
