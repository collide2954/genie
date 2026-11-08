use std::io::BufReader;

use genie_core::{Feature, Region};
use genie_formats::bgzf::Bgzf;
use genie_formats::features::{self, Entry, Kind};
use genie_formats::table::Table;
use genie_formats::{bbi, tabix};

use super::{FEATURE_SPAN, Loaded, PLAIN_LIMIT, is_compressed, plain, tabix_index};
use crate::location::{Location, Reader};

#[derive(Debug)]
pub enum FeatureFile {
    Indexed { kind: Kind, index: tabix::Index },
    Table { kind: Kind, table: Table<Feature> },
    BigBed { index: bbi::Index },
}

impl FeatureFile {
    pub fn open(location: &Location, name: &str) -> Result<Self, String> {
        let kind = Kind::of(name).ok_or_else(|| {
            format!(
                "The file {name} is not a BED, GFF3, GTF, VCF, BAM, bigWig, bigBed or bedGraph file."
            )
        })?;
        if !is_compressed(name) {
            let table = features::read_table(kind, plain(location, name)?)
                .map_err(|error| format!("The file {name} is not valid. {error}"))?;
            return Ok(Self::Table { kind, table });
        }
        let index = tabix_index(location, name)?;
        Ok(Self::Indexed { kind, index })
    }

    pub fn contigs(&self) -> &[String] {
        match self {
            Self::Indexed { index, .. } => index.names(),
            Self::Table { table, .. } => table.contigs(),
            Self::BigBed { index } => index.names(),
        }
    }

    pub fn span(&self) -> u64 {
        match self {
            Self::Indexed { .. } | Self::Table { .. } => FEATURE_SPAN,
            Self::BigBed { .. } => u64::MAX,
        }
    }

    pub fn has_colors(&self) -> bool {
        matches!(
            self,
            Self::Indexed {
                kind: Kind::Bed,
                ..
            } | Self::Table {
                kind: Kind::Bed,
                ..
            } | Self::BigBed { .. }
        )
    }

    pub fn load(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
        step: u64,
        detail: bool,
    ) -> Result<Loaded, String> {
        if let Self::BigBed { index } = self
            && !detail
        {
            let summaries = index
                .summaries(&mut BufReader::new(location.open()?), contig, region, step)
                .map_err(|error| error.to_string())?;
            return Ok(Loaded::signal(region, summaries, step));
        }
        let (region, features) = self.features(location, contig, region)?;
        Ok(Loaded::features(region, features))
    }

    pub fn spans(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
    ) -> Result<Vec<Region>, String> {
        let (_, features) = self.features(location, contig, region)?;
        Ok(features.iter().map(Feature::span).collect())
    }

    fn features(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
    ) -> Result<(Region, Vec<Feature>), String> {
        match self {
            Self::Table { table, .. } => Ok((region, table.query(contig, region))),
            Self::Indexed { kind, index } => {
                let mut reader = Bgzf::new(location.open()?);
                features::query(*kind, index, &mut reader, contig, region)
                    .map_err(|error| error.to_string())
            }
            Self::BigBed { index } => {
                let features = index
                    .features(&mut BufReader::new(location.open()?), contig, region)
                    .map_err(|error| error.to_string())?;
                Ok((region, features))
            }
        }
    }

    pub fn names(&self, location: &Location) -> Result<Vec<Entry>, String> {
        match self {
            Self::Indexed { kind, .. } => features::names(*kind, Bgzf::new(location.open()?)),
            Self::Table { kind, .. } => features::names(*kind, BufReader::new(location.open()?)),
            Self::BigBed { index } => {
                let size = location.size().unwrap_or(u64::MAX);
                if index.has_lookup() || size > PLAIN_LIMIT {
                    return Ok(Vec::new());
                }
                bigbed_names(index, &mut BufReader::new(location.open()?))
            }
        }
        .map_err(|error| error.to_string())
    }

    pub fn find(&self, location: &Location, name: &str) -> Vec<(String, Feature)> {
        let Self::BigBed { index } = self else {
            return Vec::new();
        };
        let Ok(reader) = location.open() else {
            return Vec::new();
        };
        index
            .find(&mut BufReader::new(reader), name)
            .unwrap_or_default()
    }
}

fn bigbed_names(
    index: &bbi::Index,
    reader: &mut BufReader<Reader>,
) -> Result<Vec<Entry>, genie_formats::Error> {
    let whole = Region::between(0, u64::from(u32::MAX));
    let mut entries = Vec::new();
    for contig in index.names() {
        for feature in index.features(reader, contig, whole)? {
            if !feature.name().is_empty() {
                entries.push(Entry {
                    name: feature.name().to_owned(),
                    contig: contig.clone(),
                    span: feature.span(),
                });
            }
        }
    }
    Ok(entries)
}
