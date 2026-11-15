use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use genie_core::Region;
use genie_core::contact::ContactMap;
use genie_formats::hic::{self, Hic, Matrix};

use super::{Contacts, Loaded};
use crate::location::{Location, Reader};

/// The bins of a map are at least this many steps wide, so that their bricks are 2 to 8 pixels
/// wide.
const STEPS: u64 = 16;
/// The normalizations that a map shows when nobody chose one, the best first.
const PREFERRED: [&str; 4] = ["SCALE", "KR", "VC_SQRT", "VC"];

/// What a Hi-C map shows: a normalization of the file (its place in the list of the file, or
/// else the best one) and counts, or ratios of observed to expected contacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Showing {
    pub normalization: Option<u8>,
    pub ratio: bool,
}

/// A normalization, a chromosome, a bin size, and expected values instead of a normalization
/// vector.
type Key = (String, usize, u32, bool);
/// The matrices of the chromosomes at the bin sizes that genie read.
type Matrices = HashMap<(usize, u32), Option<Arc<Matrix>>>;
type Vectors = HashMap<Key, Option<Arc<Vec<f64>>>>;

/// A Hi-C file of Juicer. genie reads only the blocks of the matrix that the view needs.
#[derive(Debug)]
pub struct ContactFile {
    hic: Hic,
    names: Vec<String>,
    normalizations: Vec<String>,
    matrices: Mutex<Matrices>,
    vectors: Mutex<Vectors>,
}

impl ContactFile {
    pub fn open(location: &Location, name: &str) -> Result<Self, String> {
        let hic = Hic::read(&mut location.open()?)
            .map_err(|error| format!("The file {name} is not valid. {error}"))?;
        let names = hic
            .chromosomes
            .iter()
            .map(|(name, _)| name.clone())
            .filter(|name| !name.eq_ignore_ascii_case("all"))
            .collect();
        let normalizations = hic.normalizations();
        Ok(Self {
            hic,
            names,
            normalizations,
            matrices: Mutex::default(),
            vectors: Mutex::default(),
        })
    }

    pub fn contigs(&self) -> &[String] {
        &self.names
    }

    pub fn normalizations(&self) -> &[String] {
        &self.normalizations
    }

    /// The place of the normalization that a choice shows: the chosen one, or else the best one
    /// of the file.
    pub fn normalization(&self, choice: Option<u8>) -> usize {
        choice
            .map(usize::from)
            .filter(|place| *place < self.normalizations.len())
            .or_else(|| {
                PREFERRED
                    .iter()
                    .find_map(|kind| self.normalizations.iter().position(|known| known == kind))
            })
            .unwrap_or(0)
    }

    /// The normalization of a map of a chromosome at a bin size: the chosen one, or else the
    /// best one of the file, if the file has its vector there. Else the map shows the counts.
    fn kind(&self, choice: Option<u8>, chromosome: usize, bin: u32) -> &str {
        let usable = |kind: &str| kind == hic::NONE || self.hic.has_norm(kind, chromosome, bin);
        match choice.and_then(|place| self.normalizations.get(usize::from(place))) {
            Some(kind) if usable(kind) => kind,
            Some(_) => hic::NONE,
            None => PREFERRED
                .iter()
                .copied()
                .find(|kind| usable(kind))
                .unwrap_or(hic::NONE),
        }
    }

    /// The bin size for a step: the smallest of the file that is at least `STEPS` steps, else
    /// the largest.
    pub fn bin(&self, step: u64) -> Option<u32> {
        let resolutions = &self.hic.resolutions;
        resolutions
            .iter()
            .copied()
            .filter(|bin| u64::from(*bin) >= step.saturating_mul(STEPS))
            .min()
            .or_else(|| resolutions.iter().copied().max())
    }

    pub fn load(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
        (step, showing): (u64, Showing),
    ) -> Result<Loaded, String> {
        let (map, normalization) = self.map(location, contig, region, (step, showing))?;
        Ok(Loaded::contacts(
            region,
            Contacts {
                map,
                step,
                normalization,
            },
        ))
    }

    /// The map of a region, and the normalization of its values.
    fn map(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
        (step, showing): (u64, Showing),
    ) -> Result<(ContactMap, String), String> {
        let ratio = showing.ratio;
        let none = || hic::NONE.to_owned();
        let Some(bin) = self.bin(step) else {
            let empty = ContactMap {
                ratio,
                ..ContactMap::default()
            };
            return Ok((empty, none()));
        };
        let size = u64::from(bin);
        let empty = ContactMap {
            bin: size,
            contacts: Vec::new(),
            ratio,
        };
        let Some(chromosome) = self
            .hic
            .chromosomes
            .iter()
            .position(|(name, _)| name == contig)
        else {
            return Ok((empty, none()));
        };
        let mut reader = location.open()?;
        let Some(matrix) = self.matrix(&mut reader, chromosome, bin)? else {
            return Ok((empty, none()));
        };
        let bins = |position: u64| u32::try_from(position / size).unwrap_or(u32::MAX);
        let (first, last) = (bins(region.start()), bins(region.end().saturating_sub(1)));
        let reach = bins(region.len() / 3).saturating_add(1);
        let found = self
            .hic
            .contacts(&mut reader, &matrix, (first, last), reach)
            .map_err(|error| error.to_string())?;
        let kind = self.kind(showing.normalization, chromosome, bin).to_owned();
        let norm = if kind == hic::NONE {
            None
        } else {
            let Some(norm) = self.vector(&mut reader, (kind.clone(), chromosome, bin, false))?
            else {
                return Ok((empty, kind));
            };
            Some(norm)
        };
        let expected = if ratio {
            let key = (kind.clone(), chromosome, bin, true);
            let Some(expected) = self.vector(&mut reader, key)? else {
                return Ok((empty, kind));
            };
            Some(expected)
        } else {
            None
        };
        let mut contacts = Vec::with_capacity(found.len());
        for contact in found {
            let mut value = f64::from(contact.count);
            if let Some(norm) = &norm {
                let at = |bin: u32| norm.get(bin as usize).copied().unwrap_or(f64::NAN);
                value /= at(contact.x) * at(contact.y);
            }
            if let Some(expected) = &expected {
                let distance = (contact.y - contact.x) as usize;
                value /= expected[distance.min(expected.len().saturating_sub(1))];
            }
            if value.is_finite() && value > 0.0 {
                contacts.push((contact.x, contact.y, value as f32));
            }
        }
        contacts.sort_by_key(|(x, y, _)| (*x, *y));
        let map = ContactMap {
            bin: size,
            contacts,
            ratio,
        };
        Ok((map, kind))
    }

    fn matrix(
        &self,
        reader: &mut Reader,
        chromosome: usize,
        bin: u32,
    ) -> Result<Option<Arc<Matrix>>, String> {
        let key = (chromosome, bin);
        let known = self
            .matrices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
            .cloned();
        if let Some(known) = known {
            return Ok(known);
        }
        let found = self
            .hic
            .matrix(reader, chromosome, bin)
            .map_err(|error| error.to_string())?
            .map(Arc::new);
        self.matrices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, found.clone());
        Ok(found)
    }

    /// A normalization vector, or the expected values of each distance when `key.3` is true.
    fn vector(&self, reader: &mut Reader, key: Key) -> Result<Option<Arc<Vec<f64>>>, String> {
        let known = self
            .vectors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
            .cloned();
        if let Some(known) = known {
            return Ok(known);
        }
        let (kind, chromosome, bin, expected) = (&key.0, key.1, key.2, key.3);
        let found = if expected {
            self.hic.expected(reader, kind, chromosome, bin)
        } else {
            self.hic.norm(reader, kind, chromosome, bin)
        }
        .map_err(|error| error.to_string())?
        .filter(|values| !values.is_empty())
        .map(Arc::new);
        self.vectors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, found.clone());
        Ok(found)
    }
}
