use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;

use genie_core::Region;
use genie_core::variant::Variant;
use genie_formats::bgzf::Bgzf;
use genie_formats::features::Entry;
use genie_formats::table::Table;
use genie_formats::{tabix, vcf};

use super::{Loaded, PLAIN_LIMIT, is_compressed, plain, tabix_index};

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

    pub fn open(path: &Path, name: &str) -> Result<Self, String> {
        if !is_compressed(path) {
            let (header, table) = vcf::read_table(plain(path, name)?)
                .map_err(|error| format!("The file {name} is not valid. {error}"))?;
            return Ok(Self::Table {
                table,
                samples: header.samples,
            });
        }
        let index = tabix_index(path, name)?;
        let file = File::open(path).map_err(|error| error.to_string())?;
        let header = vcf::header(Bgzf::new(file)).map_err(|error| error.to_string())?;
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

    pub fn load(&self, path: &Path, contig: &str, region: Region) -> Result<Loaded, String> {
        match self {
            Self::Table { table, samples } => Ok(Loaded::variants(
                region,
                table.query(contig, region),
                samples.len(),
            )),
            Self::Indexed { index, samples } => {
                let mut reader = File::open(path)
                    .map(Bgzf::new)
                    .map_err(|error| error.to_string())?;
                let variants = vcf::query(index, &mut reader, contig, region)
                    .map_err(|error| error.to_string())?;
                Ok(Loaded::variants(region, variants, samples.len()))
            }
        }
    }

    pub fn names(&self, path: &Path) -> Result<Vec<Entry>, String> {
        let file = || File::open(path).map_err(|error| error.to_string());
        match self {
            Self::Table { .. } => vcf::names(BufReader::new(file()?)),
            Self::Indexed { .. } => {
                let size = fs::metadata(path).map_or(u64::MAX, |metadata| metadata.len());
                if size > PLAIN_LIMIT {
                    return Ok(Vec::new());
                }
                vcf::names(Bgzf::new(file()?))
            }
        }
        .map_err(|error| error.to_string())
    }
}
