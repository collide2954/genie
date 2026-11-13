use std::io::BufReader;

use genie_core::Region;
use genie_core::signal::{self, Summary};
use genie_formats::bgzf::Bgzf;
use genie_formats::table::Table;
use genie_formats::{bbi, bedgraph, tabix};

use super::{FEATURE_SPAN, Loaded, is_compressed, plain, tabix_index};
use crate::location::Location;

#[derive(Debug)]
pub enum SignalFile {
    BigWig { index: bbi::Index },
    BedGraph { index: tabix::Index },
    BedGraphTable { table: Table<Summary> },
}

impl SignalFile {
    pub fn open(location: &Location, name: &str, index: Option<&Location>) -> Result<Self, String> {
        if index.is_some() || is_compressed(name) {
            return Ok(Self::BedGraph {
                index: tabix_index(location, name, index)?,
            });
        }
        let table = bedgraph::read_table(plain(location, name)?)
            .map_err(|error| format!("The file {name} is not valid. {error}"))?;
        Ok(Self::BedGraphTable { table })
    }

    pub fn contigs(&self) -> &[String] {
        match self {
            Self::BigWig { index } => index.names(),
            Self::BedGraph { index } => index.names(),
            Self::BedGraphTable { table } => table.contigs(),
        }
    }

    pub fn span(&self) -> u64 {
        match self {
            Self::BigWig { .. } | Self::BedGraphTable { .. } => u64::MAX,
            Self::BedGraph { .. } => FEATURE_SPAN,
        }
    }

    pub fn load(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
        step: u64,
    ) -> Result<Loaded, String> {
        match self {
            Self::BigWig { index } => {
                let summaries = index
                    .summaries(&mut BufReader::new(location.open()?), contig, region, step)
                    .map_err(|error| error.to_string())?;
                Ok(Loaded::signal(region, summaries, step))
            }
            Self::BedGraphTable { table } => {
                let records = table.query(contig, region);
                Ok(Loaded::signal(region, signal::reduce(&records, step), step))
            }
            Self::BedGraph { index } => {
                let mut reader = Bgzf::new(location.open()?);
                let records = bedgraph::query(index, &mut reader, contig, region)
                    .map_err(|error| error.to_string())?;
                Ok(Loaded::signal(region, signal::reduce(&records, step), step))
            }
        }
    }
}
