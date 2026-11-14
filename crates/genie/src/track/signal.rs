use std::io::{BufRead, BufReader};

use genie_core::Region;
use genie_core::methylation::Site;
use genie_core::signal::{self, Summary};
use genie_formats::bedmethyl::{self, Record};
use genie_formats::bgzf::Bgzf;
use genie_formats::table::Table;
use genie_formats::{bbi, bedgraph, tabix};

use super::{FEATURE_SPAN, Loaded, Methyl, is_compressed, plain, tabix_index};
use crate::location::Location;

const CODE_LINES: usize = 2_000;

#[derive(Debug)]
pub enum SignalFile {
    BigWig {
        index: bbi::Index,
    },
    BedGraph {
        index: tabix::Index,
    },
    BedGraphTable {
        table: Table<Summary>,
    },
    BedMethyl {
        index: tabix::Index,
        codes: Vec<String>,
    },
    BedMethylTable {
        table: Table<Record>,
        codes: Vec<String>,
    },
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

    pub fn open_methyl(
        location: &Location,
        name: &str,
        index: Option<&Location>,
    ) -> Result<Self, String> {
        if index.is_some() || is_compressed(name) {
            let index = tabix_index(location, name, index)?;
            let mut records = Vec::new();
            let reader = BufReader::new(Bgzf::new(location.open()?));
            for line in reader.lines().take(CODE_LINES).map_while(Result::ok) {
                if let Ok(Some((_, record))) = bedmethyl::parse(&line) {
                    records.push(record);
                }
            }
            let codes = bedmethyl::codes(&records);
            return Ok(Self::BedMethyl { index, codes });
        }
        let table = bedmethyl::read_table(plain(location, name)?)
            .map_err(|error| format!("The file {name} is not valid. {error}"))?;
        let records: Vec<Record> = table
            .contigs()
            .iter()
            .flat_map(|contig| table.query(contig, Region::between(0, u64::MAX)))
            .collect();
        let codes = bedmethyl::codes(&records);
        Ok(Self::BedMethylTable { table, codes })
    }

    pub fn is_bedmethyl(&self) -> bool {
        matches!(self, Self::BedMethyl { .. } | Self::BedMethylTable { .. })
    }

    pub fn codes(&self) -> &[String] {
        match self {
            Self::BedMethyl { codes, .. } | Self::BedMethylTable { codes, .. } => codes,
            _ => &[],
        }
    }

    pub fn contigs(&self) -> &[String] {
        match self {
            Self::BigWig { index } => index.names(),
            Self::BedGraph { index } | Self::BedMethyl { index, .. } => index.names(),
            Self::BedGraphTable { table } => table.contigs(),
            Self::BedMethylTable { table, .. } => table.contigs(),
        }
    }

    pub fn span(&self) -> u64 {
        match self {
            Self::BigWig { .. } | Self::BedGraphTable { .. } | Self::BedMethylTable { .. } => {
                u64::MAX
            }
            Self::BedGraph { .. } | Self::BedMethyl { .. } => FEATURE_SPAN,
        }
    }

    pub fn load(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
        step: u64,
        methyl: Methyl,
    ) -> Result<Loaded, String> {
        let summaries = match self {
            Self::BigWig { index } => {
                let summaries = index
                    .summaries(&mut BufReader::new(location.open()?), contig, region, step)
                    .map_err(|error| error.to_string())?;
                return Ok(Loaded::signal(region, summaries, step));
            }
            Self::BedGraphTable { table } => table.query(contig, region),
            Self::BedGraph { index } => {
                let mut reader = Bgzf::new(location.open()?);
                bedgraph::query(index, &mut reader, contig, region)
                    .map_err(|error| error.to_string())?
            }
            Self::BedMethylTable { table, codes } => {
                let sites = sites(&table.query(contig, region), codes, methyl);
                return Ok(Loaded::methylation(region, sites, step));
            }
            Self::BedMethyl { index, codes } => {
                let mut reader = Bgzf::new(location.open()?);
                let records = bedmethyl::query(index, &mut reader, contig, region)
                    .map_err(|error| error.to_string())?;
                let sites = sites(&records, codes, methyl);
                return Ok(Loaded::methylation(region, sites, step));
            }
        };
        Ok(Loaded::signal(
            region,
            signal::reduce(&summaries, step),
            step,
        ))
    }
}

fn sites(records: &[Record], codes: &[String], methyl: Methyl) -> Vec<Site> {
    let Some(name) = codes
        .get(usize::from(methyl.code))
        .or_else(|| codes.first())
    else {
        return Vec::new();
    };
    bedmethyl::sites(records, name, !methyl.separate)
}
