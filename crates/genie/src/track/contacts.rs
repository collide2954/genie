use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use genie_core::Region;
use genie_core::contact::ContactMap;
use genie_formats::cool::{self, Cooler};
use genie_formats::hic::{self, Contact, Hic, Matrix};

use super::{Contacts, Loaded};
use crate::location::{Location, Reader};

/// The bins of a map are at least this many steps wide, so that their bricks are 2 to 8 pixels
/// wide.
const STEPS: u64 = 16;
/// The normalizations that a map shows when nobody chose one, the best first: those of Juicer,
/// then the balancing weights of cooler.
const PREFERRED: [&str; 5] = ["SCALE", "KR", "VC_SQRT", "VC", "weight"];

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

/// The file of a Hi-C map.
#[derive(Debug)]
enum Backend {
    /// A Hi-C file of Juicer, with the matrices and the vectors that genie read.
    Hic {
        hic: Hic,
        matrices: Mutex<Matrices>,
        vectors: Mutex<Vectors>,
    },
    /// A cooler file, `.cool` or `.mcool`.
    Cool(Cooler),
}

/// A Hi-C file of Juicer or a cooler file. genie reads only the parts of the matrix that the
/// view needs.
#[derive(Debug)]
pub struct ContactFile {
    backend: Backend,
    names: Vec<String>,
    normalizations: Vec<String>,
}

/// The contacts of a view and their values: the counts or the normalized counts, divided by
/// the expected value of their distance for a ratio.
type Values = Vec<(u32, u32, f32)>;

impl ContactFile {
    pub fn open(location: &Location, name: &str) -> Result<Self, String> {
        let invalid =
            |error: genie_formats::Error| format!("The file {name} is not valid. {error}");
        let mut reader = location.open()?;
        if cool::is_cooler(name) {
            let cooler = Cooler::read(&mut reader).map_err(invalid)?;
            let names = cooler
                .chromosomes
                .iter()
                .map(|(name, _)| name.clone())
                .collect();
            let normalizations = cooler.normalizations.clone();
            return Ok(Self {
                backend: Backend::Cool(cooler),
                names,
                normalizations,
            });
        }
        let hic = Hic::read(&mut reader).map_err(invalid)?;
        let names = hic
            .chromosomes
            .iter()
            .map(|(name, _)| name.clone())
            .filter(|name| !name.eq_ignore_ascii_case("all"))
            .collect();
        let normalizations = hic.normalizations();
        Ok(Self {
            backend: Backend::Hic {
                hic,
                matrices: Mutex::default(),
                vectors: Mutex::default(),
            },
            names,
            normalizations,
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

    /// Whether the file has a normalization for a chromosome at a bin size.
    fn usable(&self, kind: &str, chromosome: usize, bin: u32) -> bool {
        kind == hic::NONE
            || match &self.backend {
                Backend::Hic { hic, .. } => hic.has_norm(kind, chromosome, bin),
                Backend::Cool(_) => self.normalizations.iter().any(|known| known == kind),
            }
    }

    /// The normalization of a map of a chromosome at a bin size: the chosen one, or else the
    /// best one of the file, if the file has its vector there. Else the map shows the counts.
    fn kind(&self, choice: Option<u8>, chromosome: usize, bin: u32) -> &str {
        match choice.and_then(|place| self.normalizations.get(usize::from(place))) {
            Some(kind) if self.usable(kind, chromosome, bin) => kind,
            Some(_) => hic::NONE,
            None => PREFERRED
                .iter()
                .copied()
                .find(|kind| self.usable(kind, chromosome, bin))
                .unwrap_or(hic::NONE),
        }
    }

    /// The bin sizes of the file.
    fn resolutions(&self) -> Vec<u32> {
        match &self.backend {
            Backend::Hic { hic, .. } => hic.resolutions.clone(),
            Backend::Cool(cooler) => cooler.resolutions(),
        }
    }

    /// The bin size for a step: the smallest of the file that is at least `STEPS` steps, else
    /// the largest.
    pub fn bin(&self, step: u64) -> Option<u32> {
        let resolutions = self.resolutions();
        resolutions
            .iter()
            .copied()
            .filter(|bin| u64::from(*bin) >= step.saturating_mul(STEPS))
            .min()
            .or_else(|| resolutions.iter().copied().max())
    }

    /// The place of a contig in the chromosomes of the file.
    fn chromosome(&self, contig: &str) -> Option<usize> {
        match &self.backend {
            Backend::Hic { hic, .. } => hic.chromosomes.iter().position(|(name, _)| name == contig),
            Backend::Cool(cooler) => cooler
                .chromosomes
                .iter()
                .position(|(name, _)| name == contig),
        }
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
        let Some(chromosome) = self.chromosome(contig) else {
            return Ok((empty, none()));
        };
        let mut reader = location.open()?;
        let bins = |position: u64| u32::try_from(position / size).unwrap_or(u32::MAX);
        let (first, last) = (bins(region.start()), bins(region.end().saturating_sub(1)));
        let reach = bins(region.len() / 3).saturating_add(1);
        let kind = self.kind(showing.normalization, chromosome, bin).to_owned();
        let window = (chromosome, bin, (first, last), reach);
        let values = match &self.backend {
            Backend::Hic {
                hic,
                matrices,
                vectors,
            } => hic_values(
                hic,
                (matrices, vectors),
                &mut reader,
                window,
                (&kind, ratio),
            )?,
            Backend::Cool(cooler) => cool_values(cooler, &mut reader, window, (&kind, ratio))?,
        };
        let Some(mut contacts) = values else {
            return Ok((empty, kind));
        };
        contacts.sort_by_key(|(x, y, _)| (*x, *y));
        let map = ContactMap {
            bin: size,
            contacts,
            ratio,
        };
        Ok((map, kind))
    }
}

/// The finite values above 0 of contacts, with a value for each.
fn kept(contacts: Vec<Contact>, value: impl Fn(&Contact) -> f64) -> Values {
    contacts
        .into_iter()
        .filter_map(|contact| {
            let value = value(&contact);
            (value.is_finite() && value > 0.0).then_some((contact.x, contact.y, value as f32))
        })
        .collect()
}

/// The contacts of a view in a Hi-C file of Juicer: divided by the normalization vector at their
/// two bins, and by the expected value of their distance for a ratio. None when the file has no
/// matrix or no vector there.
fn hic_values(
    hic: &Hic,
    (matrices, vectors): (&Mutex<Matrices>, &Mutex<Vectors>),
    reader: &mut Reader,
    (chromosome, bin, (first, last), reach): (usize, u32, (u32, u32), u32),
    (kind, ratio): (&str, bool),
) -> Result<Option<Values>, String> {
    let Some(matrix) = matrix(hic, matrices, reader, chromosome, bin)? else {
        return Ok(None);
    };
    let found = hic
        .contacts(reader, &matrix, (first, last), reach)
        .map_err(|error| error.to_string())?;
    let norm = if kind == hic::NONE {
        None
    } else {
        let key = (kind.to_owned(), chromosome, bin, false);
        let Some(norm) = vector(hic, vectors, reader, key)? else {
            return Ok(None);
        };
        Some(norm)
    };
    let expected = if ratio {
        let key = (kind.to_owned(), chromosome, bin, true);
        let Some(expected) = vector(hic, vectors, reader, key)? else {
            return Ok(None);
        };
        Some(expected)
    } else {
        None
    };
    Ok(Some(kept(found, |contact| {
        let mut value = f64::from(contact.count);
        if let Some(norm) = &norm {
            let at = |bin: u32| norm.get(bin as usize).copied().unwrap_or(f64::NAN);
            value /= at(contact.x) * at(contact.y);
        }
        if let Some(expected) = &expected {
            let distance = (contact.y - contact.x) as usize;
            value /= expected[distance.min(expected.len().saturating_sub(1))];
        }
        value
    })))
}

/// The contacts of a view in a cooler file: multiplied by the balancing weights of their two
/// bins. Cooler files have no expected values, so a ratio divides each value by the mean value
/// of the contacts in view at its distance.
fn cool_values(
    cooler: &Cooler,
    reader: &mut Reader,
    (chromosome, bin, (first, last), reach): (usize, u32, (u32, u32), u32),
    (kind, ratio): (&str, bool),
) -> Result<Option<Values>, String> {
    let found = cooler
        .contacts(reader, bin, chromosome, (first, last), reach)
        .map_err(|error| error.to_string())?;
    let weights = cooler
        .weights(reader, bin, chromosome, kind)
        .map_err(|error| error.to_string())?;
    let mut values = kept(found, |contact| {
        let mut value = f64::from(contact.count);
        if let Some(weights) = &weights {
            let at = |bin: u32| weights.get(bin as usize).copied().unwrap_or(f64::NAN);
            value *= at(contact.x) * at(contact.y);
        }
        value
    });
    if ratio {
        let mut sums: HashMap<u32, (f64, u32)> = HashMap::new();
        for (x, y, value) in &values {
            let sum = sums.entry(y - x).or_default();
            sum.0 += f64::from(*value);
            sum.1 += 1;
        }
        for (x, y, value) in &mut values {
            let (sum, count) = sums[&(*y - *x)];
            *value = (f64::from(*value) * f64::from(count) / sum) as f32;
        }
    }
    Ok(Some(values))
}

fn matrix(
    hic: &Hic,
    matrices: &Mutex<Matrices>,
    reader: &mut Reader,
    chromosome: usize,
    bin: u32,
) -> Result<Option<Arc<Matrix>>, String> {
    let key = (chromosome, bin);
    let known = matrices
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .cloned();
    if let Some(known) = known {
        return Ok(known);
    }
    let found = hic
        .matrix(reader, chromosome, bin)
        .map_err(|error| error.to_string())?
        .map(Arc::new);
    matrices
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, found.clone());
    Ok(found)
}

/// A normalization vector, or the expected values of each distance when `key.3` is true.
fn vector(
    hic: &Hic,
    vectors: &Mutex<Vectors>,
    reader: &mut Reader,
    key: Key,
) -> Result<Option<Arc<Vec<f64>>>, String> {
    let known = vectors
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .cloned();
    if let Some(known) = known {
        return Ok(known);
    }
    let (kind, chromosome, bin, expected) = (&key.0, key.1, key.2, key.3);
    let found = if expected {
        hic.expected(reader, kind, chromosome, bin)
    } else {
        hic.norm(reader, kind, chromosome, bin)
    }
    .map_err(|error| error.to_string())?
    .filter(|values| !values.is_empty())
    .map(Arc::new);
    vectors
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, found.clone());
    Ok(found)
}
