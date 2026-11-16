use std::io::BufReader;

use genie_core::Region;
use genie_core::signal::Tally;
use genie_core::variant::Variant;
use genie_formats::annotations::{self, Layout};
use genie_formats::bgzf::Bgzf;
use genie_formats::features::Entry;
use genie_formats::table::Table;
use genie_formats::{bcf, tabix, vcf};

use super::{Loaded, PLAIN_LIMIT, Scope, density, is_bcf, is_compressed, plain, tabix_index};
use crate::location::Location;

#[derive(Debug)]
pub enum VariantFile {
    Indexed {
        index: tabix::Index,
        samples: Vec<String>,
        infos: Vec<(String, String)>,
        /// The INFO fields that hold annotations, for example CSQ from VEP.
        layouts: Vec<Layout>,
    },
    Table {
        table: Table<Variant>,
        samples: Vec<String>,
        infos: Vec<(String, String)>,
    },
    /// A BCF file with its CSI index.
    Bcf {
        index: tabix::Index,
        header: bcf::Header,
        layouts: Vec<Layout>,
    },
}

impl VariantFile {
    pub const SPAN: u64 = 1_000_000;

    pub fn open(location: &Location, name: &str, index: Option<&Location>) -> Result<Self, String> {
        if is_bcf(name) {
            return Self::open_bcf(location, name, index);
        }
        if index.is_none() && !is_compressed(name) {
            let (header, table) = vcf::read_table(plain(location, name)?)
                .map_err(|error| format!("The file {name} is not valid. {error}"))?;
            return Ok(Self::Table {
                table,
                samples: header.samples,
                infos: header.infos,
            });
        }
        let index = tabix_index(location, name, index)?;
        let header = vcf::header(Bgzf::new(location.open()?)).map_err(|error| error.to_string())?;
        Ok(Self::Indexed {
            index,
            samples: header.samples,
            layouts: annotations::layouts(&header.infos),
            infos: header.infos,
        })
    }

    /// Opens a BCF file with its CSI index, or reads all of it when it has no index and is
    /// 50 MB or less.
    fn open_bcf(location: &Location, name: &str, index: Option<&Location>) -> Result<Self, String> {
        let invalid =
            |error: genie_formats::Error| format!("The file {name} is not valid. {error}");
        let file = if let Some(index) = index {
            Some((index.open()?, index.name()))
        } else {
            let index = location.with_suffix(".csi");
            index.open().ok().map(|file| (file, index.name()))
        };
        let Some((file, label)) = file else {
            if location.size()? > PLAIN_LIMIT {
                return Err(format!(
                    "There is no index {name}.csi. Index the file with bcftools index."
                ));
            }
            let (header, table) = bcf::read_table(Bgzf::new(location.open()?)).map_err(invalid)?;
            return Ok(Self::Table {
                table,
                samples: header.vcf.samples,
                infos: header.vcf.infos,
            });
        };
        let header = bcf::header(&mut Bgzf::new(location.open()?)).map_err(invalid)?;
        let index = tabix::Index::read(file)
            .and_then(|index| index.named(&header.contigs))
            .map_err(|error| format!("The index {label} is not valid. {error}"))?;
        Ok(Self::Bcf {
            index,
            layouts: annotations::layouts(&header.vcf.infos),
            header,
        })
    }

    pub fn contigs(&self) -> &[String] {
        match self {
            Self::Indexed { index, .. } | Self::Bcf { index, .. } => index.names(),
            Self::Table { table, .. } => table.contigs(),
        }
    }

    pub fn samples(&self) -> &[String] {
        match self {
            Self::Indexed { samples, .. } | Self::Table { samples, .. } => samples,
            Self::Bcf { header, .. } => &header.vcf.samples,
        }
    }

    pub fn infos(&self) -> &[(String, String)] {
        match self {
            Self::Indexed { infos, .. } | Self::Table { infos, .. } => infos,
            Self::Bcf { header, .. } => &header.vcf.infos,
        }
    }

    /// The widest view that shows data: all views when the file can give a density.
    pub fn span(&self) -> u64 {
        match self {
            Self::Indexed { index, .. } | Self::Bcf { index, .. } if !index.has_counts() => {
                Self::SPAN
            }
            _ => u64::MAX,
        }
    }

    /// The variants of a region, or their density when the view is wider than `SPAN`.
    pub fn load(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
        scope: Scope<'_>,
    ) -> Result<Loaded, String> {
        if !scope.detail {
            return Ok(match self {
                Self::Table { table, .. } => {
                    density(region, scope.step, table.starts(contig, region))
                }
                Self::Indexed { index, .. } | Self::Bcf { index, .. } => {
                    let tally = index
                        .density(contig, region, scope.step, scope.length)
                        .unwrap_or_else(|| Tally::new(region, scope.step));
                    Loaded::signal(region, tally.summaries(), scope.step)
                }
            });
        }
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

    pub fn variants(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
    ) -> Result<Vec<Variant>, String> {
        match self {
            Self::Table { table, .. } => Ok(table.query(contig, region)),
            Self::Indexed { index, layouts, .. } => {
                let mut reader = Bgzf::new(location.open()?);
                vcf::query(index, &mut reader, contig, region, layouts)
                    .map_err(|error| error.to_string())
            }
            Self::Bcf {
                index,
                header,
                layouts,
            } => {
                let mut reader = Bgzf::new(location.open()?);
                bcf::query(index, &mut reader, header, (contig, region), layouts)
                    .map_err(|error| error.to_string())
            }
        }
    }

    pub fn names(&self, location: &Location) -> Result<Vec<Entry>, String> {
        let binary = is_bcf(&location.name());
        match self {
            Self::Table { .. } if binary => bcf::names(Bgzf::new(location.open()?)),
            Self::Table { .. } => vcf::names(BufReader::new(location.open()?)),
            Self::Indexed { .. } | Self::Bcf { .. } => {
                let size = location.size().unwrap_or(u64::MAX);
                if size > PLAIN_LIMIT {
                    return Ok(Vec::new());
                }
                if binary {
                    bcf::names(Bgzf::new(location.open()?))
                } else {
                    vcf::names(Bgzf::new(location.open()?))
                }
            }
        }
        .map_err(|error| error.to_string())
    }
}
