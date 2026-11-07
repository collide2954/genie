use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;

use genie_core::{Feature, Region};
use genie_formats::bgzf::Bgzf;
use genie_formats::features::{self, Entry, Kind};
use genie_formats::table::Table;
use genie_formats::{bbi, tabix};

use super::{FEATURE_SPAN, Loaded, PLAIN_LIMIT, is_compressed, plain, tabix_index};

#[derive(Debug)]
pub enum FeatureFile {
    Indexed { kind: Kind, index: tabix::Index },
    Table { kind: Kind, table: Table<Feature> },
    BigBed { index: bbi::Index },
}

impl FeatureFile {
    pub fn open(path: &Path, name: &str) -> Result<Self, String> {
        let kind = Kind::of(name).ok_or_else(|| {
            format!(
                "The file {name} is not a BED, GFF3, GTF, VCF, BAM, bigWig, bigBed or bedGraph file."
            )
        })?;
        if !is_compressed(path) {
            let table = features::read_table(kind, plain(path, name)?)
                .map_err(|error| format!("The file {name} is not valid. {error}"))?;
            return Ok(Self::Table { kind, table });
        }
        let index = tabix_index(path, name)?;
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
        path: &Path,
        contig: &str,
        region: Region,
        step: u64,
        detail: bool,
    ) -> Result<Loaded, String> {
        match self {
            Self::BigBed { index } => {
                let file = File::open(path).map_err(|error| error.to_string())?;
                let mut file = BufReader::new(file);
                if detail {
                    let features = index
                        .features(&mut file, contig, region)
                        .map_err(|error| error.to_string())?;
                    return Ok(Loaded::features(region, features));
                }
                let summaries = index
                    .summaries(&mut file, contig, region, step)
                    .map_err(|error| error.to_string())?;
                Ok(Loaded::signal(region, summaries, step))
            }
            Self::Table { table, .. } => Ok(Loaded::features(region, table.query(contig, region))),
            Self::Indexed { kind, index } => {
                let mut reader = File::open(path)
                    .map(Bgzf::new)
                    .map_err(|error| error.to_string())?;
                let (region, features) = features::query(*kind, index, &mut reader, contig, region)
                    .map_err(|error| error.to_string())?;
                Ok(Loaded::features(region, features))
            }
        }
    }

    pub fn names(&self, path: &Path) -> Result<Vec<Entry>, String> {
        let file = || File::open(path).map_err(|error| error.to_string());
        match self {
            Self::Indexed { kind, .. } => features::names(*kind, Bgzf::new(file()?)),
            Self::Table { kind, .. } => features::names(*kind, BufReader::new(file()?)),
            Self::BigBed { index } => {
                let size = fs::metadata(path).map_or(u64::MAX, |metadata| metadata.len());
                if index.has_lookup() || size > PLAIN_LIMIT {
                    return Ok(Vec::new());
                }
                bigbed_names(index, &mut BufReader::new(file()?))
            }
        }
        .map_err(|error| error.to_string())
    }

    pub fn find(&self, path: &Path, name: &str) -> Vec<(String, Feature)> {
        let Self::BigBed { index } = self else {
            return Vec::new();
        };
        let Ok(file) = File::open(path) else {
            return Vec::new();
        };
        index
            .find(&mut BufReader::new(file), name)
            .unwrap_or_default()
    }
}

fn bigbed_names(
    index: &bbi::Index,
    reader: &mut BufReader<File>,
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
