use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};

use genie_core::Region;
use genie_core::association::Association;
use genie_formats::bgzf::Bgzf;
use genie_formats::fields::Field;
use genie_formats::gwas::{self, Layout};
use genie_formats::{gzip, tabix};

use super::{Loaded, has_index, is_compressed, tabix_index};
use crate::location::Location;

/// The largest GWAS file without an index that genie reads whole.
const WHOLE_LIMIT: u64 = 1 << 30;
/// Rows with this -log10 p or more keep their text for the card, also in wide views.
const KEPT: f32 = 3.0;
/// Views of this width or less keep the text of all rows.
const NEAR: u64 = 3_000_000;
/// The widest view of a GWAS file with an index on the web.
const WEB_SPAN: u64 = 50_000_000;
/// The parts of a region in which the rows under `KEPT` share cells.
const CELLS: u64 = 8_192;
/// The steps of the -log10 p in one, for the cells of the rows under `KEPT`.
const LEVELS: f32 = 20.0;

/// The positions and the -log10 p of the rows of one contig, in the order of their positions.
type Rows = Vec<(u32, f32)>;

/// GWAS summary statistics: a file with a tabix index, or a whole file in memory.
#[derive(Debug)]
pub enum GwasFile {
    Indexed {
        index: tabix::Index,
        layout: Layout,
        remote: bool,
    },
    Table {
        layout: Layout,
        contigs: Vec<String>,
        rows: Vec<Rows>,
        /// The text of the rows of `KEPT` or more, by their contig and their position.
        texts: HashMap<(usize, u32), Box<str>>,
    },
}

fn missing(name: &str) -> String {
    format!(
        "The file {name} has no header line that names the chromosome, the position and the \
         p-value."
    )
}

impl GwasFile {
    pub fn open(location: &Location, name: &str, index: Option<&Location>) -> Result<Self, String> {
        let invalid =
            |error: genie_formats::Error| format!("The file {name} is not valid. {error}");
        if index.is_some() || (is_compressed(name) && has_index(location)) {
            let index = tabix_index(location, name, index)?;
            let line = gwas::header(Bgzf::new(location.open()?)).map_err(invalid)?;
            let layout = line
                .as_deref()
                .and_then(Layout::sniff)
                .ok_or_else(|| missing(name))?;
            return Ok(Self::Indexed {
                index,
                layout,
                remote: location.is_remote(),
            });
        }
        if location.size()? > WHOLE_LIMIT {
            return Err(format!(
                "The file {name} is larger than 1 GB. Compress it with bgzip and index it with \
                 tabix, with the columns of the chromosome and the position."
            ));
        }
        let file = BufReader::new(location.open()?);
        let reader: Box<dyn BufRead> = if is_compressed(name) {
            Box::new(BufReader::new(gzip::Reader::new(file)))
        } else {
            Box::new(file)
        };
        let mut contigs: Vec<String> = Vec::new();
        let mut rows: Vec<Rows> = Vec::new();
        let mut texts = HashMap::new();
        let layout = gwas::read(reader, |contig, position, value, line| {
            let Ok(position) = u32::try_from(position) else {
                return;
            };
            let at = if contigs.last().is_some_and(|last| last == contig) {
                contigs.len() - 1
            } else if let Some(at) = contigs.iter().position(|known| known == contig) {
                at
            } else {
                contigs.push(contig.to_owned());
                rows.push(Vec::new());
                contigs.len() - 1
            };
            rows[at].push((position, value));
            if value >= KEPT {
                texts.insert((at, position), line.into());
            }
        })
        .map_err(invalid)?
        .ok_or_else(|| missing(name))?;
        for list in &mut rows {
            list.sort_by_key(|(position, _)| *position);
        }
        Ok(Self::Table {
            layout,
            contigs,
            rows,
            texts,
        })
    }

    pub fn contigs(&self) -> &[String] {
        match self {
            Self::Indexed { index, .. } => index.names(),
            Self::Table { contigs, .. } => contigs,
        }
    }

    /// The widest view that shows the rows: all views, except for a file with an index on the
    /// web.
    pub fn span(&self) -> u64 {
        match self {
            Self::Indexed { remote: true, .. } => WEB_SPAN,
            _ => u64::MAX,
        }
    }

    pub fn load(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
    ) -> Result<Loaded, String> {
        let near = region.len() <= NEAR;
        let mut found = Vec::new();
        match self {
            Self::Indexed { index, layout, .. } => {
                let mut reader = Bgzf::new(location.open()?);
                gwas::query(
                    index,
                    &mut reader,
                    layout,
                    (contig, region),
                    |position, value, line| {
                        let row = (near || value >= KEPT).then(|| line.into());
                        found.push(Association {
                            position,
                            value,
                            row,
                        });
                    },
                )
                .map_err(|error| error.to_string())?;
            }
            Self::Table {
                contigs,
                rows,
                texts,
                ..
            } => {
                if let Some(at) = contigs.iter().position(|known| known == contig) {
                    let list = &rows[at];
                    let from =
                        list.partition_point(|(position, _)| u64::from(*position) < region.start());
                    for &(position, value) in list[from..]
                        .iter()
                        .take_while(|(position, _)| u64::from(*position) < region.end())
                    {
                        found.push(Association {
                            position: u64::from(position),
                            value,
                            row: texts.get(&(at, position)).cloned(),
                        });
                    }
                }
            }
        }
        Ok(Loaded::associations(region, thin(found, region)))
    }

    /// The fields of a row for its card.
    pub fn fields(&self, row: &str) -> Vec<Field> {
        match self {
            Self::Indexed { layout, .. } | Self::Table { layout, .. } => layout.fields(row),
        }
    }
}

/// The rows of `KEPT` or more, and one row of each cell of the others: a part of the region and
/// a step of the -log10 p. A region with few rows keeps them all.
fn thin(found: Vec<Association>, region: Region) -> Vec<Association> {
    if found.len() as u64 <= CELLS * 4 {
        return found;
    }
    let width = (region.len() / CELLS).max(1);
    let mut seen = HashSet::new();
    found
        .into_iter()
        .filter(|association| {
            association.value >= KEPT
                || seen.insert((
                    association.position / width,
                    (association.value * LEVELS) as u32,
                ))
        })
        .collect()
}
