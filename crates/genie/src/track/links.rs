use std::io::{BufRead, BufReader};

use genie_core::Region;
use genie_core::interaction::Interaction;
use genie_formats::bedpe::{self, Bedpe};
use genie_formats::fields::{self, Field};
use genie_formats::{bbi, gzip};

use super::{Loaded, PLAIN_LIMIT, is_compressed};
use crate::location::Location;

/// The widest view that loads the links of a bigInteract file.
const BIG_SPAN: u64 = 50_000_000;
/// The first column of a BEDPE file after the ten standard columns, counted from 1.
const EXTRA_COLUMN: usize = 11;

/// A track of links between two regions: a bigInteract file, or a BEDPE file in memory.
#[derive(Debug)]
pub enum LinkFile {
    Big { index: bbi::Index },
    Table { bedpe: Bedpe },
}

impl LinkFile {
    /// Reads a BEDPE file, plain or compressed, of 50 MB or less.
    pub fn open(location: &Location, name: &str) -> Result<Self, String> {
        if location.size()? > PLAIN_LIMIT {
            return Err(format!(
                "The file {name} is larger than 50 MB. genie reads BEDPE files of 50 MB or less."
            ));
        }
        let file = BufReader::new(location.open()?);
        let reader: Box<dyn BufRead> = if is_compressed(name) {
            Box::new(BufReader::new(gzip::Reader::new(file)))
        } else {
            Box::new(file)
        };
        let bedpe = bedpe::read_table(reader)
            .map_err(|error| format!("The file {name} is not valid. {error}"))?;
        Ok(Self::Table { bedpe })
    }

    pub fn contigs(&self) -> &[String] {
        match self {
            Self::Big { index } => index.names(),
            Self::Table { bedpe } => bedpe.table.contigs(),
        }
    }

    pub fn span(&self) -> u64 {
        match self {
            Self::Big { .. } => BIG_SPAN,
            Self::Table { .. } => u64::MAX,
        }
    }

    pub fn load(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
    ) -> Result<Loaded, String> {
        let links = match self {
            Self::Big { index } => index
                .interactions(&mut location.open()?, contig, region)
                .map_err(|error| error.to_string())?,
            Self::Table { bedpe } => bedpe.table.query(contig, region),
        };
        Ok(Loaded::links(region, links))
    }

    /// The other fields of a link for its card.
    pub fn fields(&self, link: &Interaction) -> Vec<Field> {
        match self {
            Self::Big { index } => index.link_fields(&link.extra),
            Self::Table { bedpe } => {
                let columns = &bedpe.columns;
                link.extra
                    .split('\t')
                    .enumerate()
                    .filter(|(place, value)| {
                        fields::shown(value) && !(*place == 0 && bedpe::rgb(value).is_some())
                    })
                    .map(|(place, value)| Field {
                        label: columns
                            .get(place)
                            .filter(|name| !name.is_empty())
                            .cloned()
                            .unwrap_or_else(|| format!("Column {}", place + EXTRA_COLUMN)),
                        value: value.to_owned(),
                    })
                    .collect()
            }
        }
    }
}
