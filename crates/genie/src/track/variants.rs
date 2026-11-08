use std::io::BufReader;

use genie_core::Region;
use genie_core::variant::Variant;
use genie_formats::bgzf::Bgzf;
use genie_formats::features::Entry;
use genie_formats::table::Table;
use genie_formats::{tabix, vcf};

use super::{Loaded, PLAIN_LIMIT, is_compressed, plain, tabix_index};
use crate::location::Location;

#[derive(Debug)]
pub enum VariantFile {
    Indexed {
        index: tabix::Index,
        samples: Vec<String>,
    },
    Table {
        table: Table<Variant>,
        samples: Vec<String>,
    },
}

impl VariantFile {
    pub const SPAN: u64 = 1_000_000;

    pub fn open(location: &Location, name: &str) -> Result<Self, String> {
        if !is_compressed(name) {
            let (header, table) = vcf::read_table(plain(location, name)?)
                .map_err(|error| format!("The file {name} is not valid. {error}"))?;
            return Ok(Self::Table {
                table,
                samples: header.samples,
            });
        }
        let index = tabix_index(location, name)?;
        let header = vcf::header(Bgzf::new(location.open()?)).map_err(|error| error.to_string())?;
        Ok(Self::Indexed {
            index,
            samples: header.samples,
        })
    }

    pub fn contigs(&self) -> &[String] {
        match self {
            Self::Indexed { index, .. } => index.names(),
            Self::Table { table, .. } => table.contigs(),
        }
    }

    pub fn samples(&self) -> &[String] {
        match self {
            Self::Indexed { samples, .. } | Self::Table { samples, .. } => samples,
        }
    }

    pub fn load(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
    ) -> Result<Loaded, String> {
        let variants = self.variants(location, contig, region)?;
        Ok(Loaded::variants(region, variants, self.samples().len()))
    }

    pub fn spans(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
    ) -> Result<Vec<Region>, String> {
        let variants = self.variants(location, contig, region)?;
        Ok(variants.iter().map(|variant| variant.span).collect())
    }

    fn variants(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
    ) -> Result<Vec<Variant>, String> {
        match self {
            Self::Table { table, .. } => Ok(table.query(contig, region)),
            Self::Indexed { index, .. } => {
                let mut reader = Bgzf::new(location.open()?);
                vcf::query(index, &mut reader, contig, region).map_err(|error| error.to_string())
            }
        }
    }

    pub fn names(&self, location: &Location) -> Result<Vec<Entry>, String> {
        match self {
            Self::Table { .. } => vcf::names(BufReader::new(location.open()?)),
            Self::Indexed { .. } => {
                let size = location.size().unwrap_or(u64::MAX);
                if size > PLAIN_LIMIT {
                    return Ok(Vec::new());
                }
                vcf::names(Bgzf::new(location.open()?))
            }
        }
        .map_err(|error| error.to_string())
    }
}
