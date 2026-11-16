use genie_core::signal::{Summary, reduce};
use genie_core::{Feature, Region, Strand};
use genie_formats::bgzf::Bgzf;
use genie_formats::columns::{self, Columns};
use genie_formats::fields::{self, Field};
use genie_formats::tabix;

use super::{Loaded, tabix_index};
use crate::location::Location;

/// What a table track shows: a column of numbers as a signal, or the rows as features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pick {
    /// The last column of numbers, or the rows when no column holds numbers.
    #[default]
    Auto,
    /// A column of numbers, counted from 0.
    Column(u16),
    Rows,
}

/// A table of tab-separated columns with a tabix index, as the CADD scores.
#[derive(Debug)]
pub struct ColumnFile {
    index: tabix::Index,
    pub columns: Columns,
}

impl ColumnFile {
    pub fn open(location: &Location, name: &str, index: Option<&Location>) -> Result<Self, String> {
        let index = tabix_index(location, name, index)?;
        let columns = Columns::read(Bgzf::new(location.open()?), &index)
            .map_err(|error| format!("The file {name} is not valid. {error}"))?;
        Ok(Self { index, columns })
    }

    pub fn contigs(&self) -> &[String] {
        self.index.names()
    }

    /// The column of numbers that a pick shows, or nothing for the rows.
    pub fn shown(&self, pick: Pick) -> Option<usize> {
        let numbers = &self.columns.numbers;
        match pick {
            Pick::Column(column) => Some(usize::from(column))
                .filter(|column| numbers.contains(column))
                .or_else(|| numbers.last().copied()),
            Pick::Rows => None,
            Pick::Auto => numbers.last().copied(),
        }
    }

    pub fn load(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
        (step, pick): (u64, Pick),
    ) -> Result<Loaded, String> {
        let mut reader = Bgzf::new(location.open()?);
        let error = |error: genie_formats::Error| error.to_string();
        if let Some(column) = self.shown(pick) {
            let mut summaries = Vec::new();
            columns::query(&self.index, &mut reader, contig, region, |span, fields| {
                let value = fields
                    .get(column)
                    .and_then(|text| text.trim().parse::<f32>().ok())
                    .filter(|value| value.is_finite());
                if let Some(value) = value {
                    summaries.push(Summary::single(span, value));
                }
            })
            .map_err(error)?;
            return Ok(Loaded::signal(region, reduce(&summaries, step), step));
        }
        let label = self.columns.label;
        let mut features = Vec::new();
        columns::query(&self.index, &mut reader, contig, region, |span, fields| {
            let name = label
                .and_then(|column| fields.get(column))
                .copied()
                .unwrap_or_default();
            let feature = Feature::new(name, span, Strand::Unknown).with_extra(fields.join("\t"));
            features.push(feature);
        })
        .map_err(error)?;
        Ok(Loaded::features(region, features))
    }

    /// The fields of a row for its card: each column with its name, except the columns of the
    /// place of the row.
    pub fn fields(&self, row: &str) -> Vec<Field> {
        row.split('\t')
            .enumerate()
            .filter(|(column, value)| !self.columns.place.contains(column) && fields::shown(value))
            .map(|(column, value)| Field {
                label: self
                    .columns
                    .names
                    .get(column)
                    .cloned()
                    .unwrap_or_else(|| format!("Column {}", column + 1)),
                value: value.to_owned(),
            })
            .collect()
    }
}
