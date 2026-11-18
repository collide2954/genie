//! Cooler files of Hi-C contacts (`.cool`), and their form with several bin sizes (`.mcool`)
//! as the 4D Nucleome project shares them. They are HDF5 files: the chromosomes, the bins with
//! their balancing weights, the pixels of the upper triangle of the matrix, and indexes.

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::sync::{Arc, Mutex, PoisonError};

use crate::hdf5::{Dataset, Hdf5};
use crate::hic::{Contact, NONE};
use crate::{Error, Reason};

/// The columns of the bins that are not balancing weights.
const PLACES: [&str; 3] = ["chrom", "start", "end"];

/// True for the name of a cooler file.
pub fn is_cooler(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("mcool") || extension.eq_ignore_ascii_case("cool")
        })
}

/// An open cooler file.
#[derive(Debug)]
pub struct Cooler {
    file: Hdf5,
    /// The bin sizes and the paths of their groups, the smallest first: "" for a `.cool` file,
    /// "resolutions/1000" in an `.mcool` file.
    groups: Vec<(u32, String)>,
    /// The chromosomes and their lengths.
    pub chromosomes: Vec<(String, u64)>,
    /// "NONE", then the columns of balancing weights of the bins, such as "weight".
    pub normalizations: Vec<String>,
    datasets: Mutex<HashMap<String, Arc<Dataset>>>,
}

/// The path of a dataset in the group of a cooler.
fn join(group: &str, name: &str) -> String {
    if group.is_empty() {
        name.to_owned()
    } else {
        format!("{group}/{name}")
    }
}

fn not_cooler() -> Error {
    Error::Invalid(Reason::NotCooler)
}

/// A whole number of a dataset as a place.
fn place(value: i64) -> Result<u64, Error> {
    u64::try_from(value).map_err(|_| not_cooler())
}

impl Cooler {
    pub fn read(reader: &mut (impl Read + Seek)) -> Result<Self, Error> {
        let file = Hdf5::open(reader)?;
        let top = file.members(reader, "")?;
        let mut groups = Vec::new();
        if top.iter().any(|name| name == "resolutions") {
            for name in file.members(reader, "resolutions")? {
                if let Ok(bin) = name.parse::<u32>() {
                    groups.push((bin, format!("resolutions/{name}")));
                }
            }
        } else if top.iter().any(|name| name == "pixels") {
            let value = |path: &str, reader: &mut _| -> Result<i64, Error> {
                let found = file.dataset(reader, path)?.integers(&file, reader, 0..1)?;
                found.first().copied().ok_or_else(not_cooler)
            };
            let bin = value("bins/end", reader)? - value("bins/start", reader)?;
            groups.push((u32::try_from(bin).map_err(|_| not_cooler())?, String::new()));
        }
        groups.sort();
        let Some((_, first)) = groups.first().cloned() else {
            return Err(not_cooler());
        };
        let names = file.dataset(reader, &join(&first, "chroms/name"))?;
        let names = names.texts(&file, reader, 0..names.len)?;
        let lengths = file.dataset(reader, &join(&first, "chroms/length"))?;
        let lengths = lengths.integers(&file, reader, 0..lengths.len)?;
        let chromosomes = names
            .into_iter()
            .zip(lengths)
            .map(|(name, length)| Ok((name, place(length)?)))
            .collect::<Result<Vec<_>, Error>>()?;
        let normalizations = std::iter::once(NONE.to_owned())
            .chain(
                file.members(reader, &join(&first, "bins"))?
                    .into_iter()
                    .filter(|name| !PLACES.contains(&name.as_str())),
            )
            .collect();
        Ok(Self {
            file,
            groups,
            chromosomes,
            normalizations,
            datasets: Mutex::default(),
        })
    }

    /// The bin sizes of the file, the smallest first.
    pub fn resolutions(&self) -> Vec<u32> {
        self.groups.iter().map(|(bin, _)| *bin).collect()
    }

    /// A dataset of the cooler of a bin size, read once.
    fn dataset(
        &self,
        reader: &mut (impl Read + Seek),
        bin: u32,
        name: &str,
    ) -> Result<Arc<Dataset>, Error> {
        let group = self
            .groups
            .iter()
            .find(|(size, _)| *size == bin)
            .map(|(_, group)| group)
            .ok_or_else(not_cooler)?;
        let path = join(group, name);
        let known = self
            .datasets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&path)
            .cloned();
        if let Some(known) = known {
            return Ok(known);
        }
        let found = Arc::new(self.file.dataset(reader, &path)?);
        self.datasets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path, Arc::clone(&found));
        Ok(found)
    }

    /// The bins of a chromosome at a bin size, in the bins of the whole genome.
    fn bins(
        &self,
        reader: &mut (impl Read + Seek),
        bin: u32,
        chromosome: usize,
    ) -> Result<(u64, u64), Error> {
        let offsets = self.dataset(reader, bin, "indexes/chrom_offset")?;
        let at = chromosome as u64;
        let bounds = offsets.integers(&self.file, reader, at..at + 2)?;
        Ok((place(bounds[0])?, place(bounds[1])?))
    }

    /// The contacts between the bins from `first` to `last` of a chromosome at a bin size, of
    /// bins at most `reach` bins apart, with `x <= y` in the bins of the chromosome.
    pub fn contacts(
        &self,
        reader: &mut (impl Read + Seek),
        bin: u32,
        chromosome: usize,
        (first, last): (u32, u32),
        reach: u32,
    ) -> Result<Vec<Contact>, Error> {
        let (low, high) = self.bins(reader, bin, chromosome)?;
        let start = low + u64::from(first);
        let end = (low + u64::from(last)).min(high.saturating_sub(1));
        if start > end {
            return Ok(Vec::new());
        }
        let index = self.dataset(reader, bin, "indexes/bin1_offset")?;
        let rows = index.integers(&self.file, reader, start..end + 2)?;
        let (from, to) = (place(rows[0])?, place(rows[rows.len() - 1])?);
        if from >= to {
            return Ok(Vec::new());
        }
        let column = |reader: &mut _, name: &str| -> Result<Vec<i64>, Error> {
            self.dataset(reader, bin, name)?
                .integers(&self.file, reader, from..to)
        };
        let firsts = column(reader, "pixels/bin1_id")?;
        let seconds = column(reader, "pixels/bin2_id")?;
        let counts =
            self.dataset(reader, bin, "pixels/count")?
                .floats(&self.file, reader, from..to)?;
        let mut found = Vec::new();
        for ((x, y), count) in firsts.into_iter().zip(seconds).zip(counts) {
            let (x, y) = (place(x)?, place(y)?);
            if y < x || y > end || x < start || y - x > u64::from(reach) {
                continue;
            }
            let local = |value: u64| u32::try_from(value - low).map_err(|_| not_cooler());
            found.push(Contact {
                x: local(x)?,
                y: local(y)?,
                count: count as f32,
            });
        }
        Ok(found)
    }

    /// The balancing weights of the bins of a chromosome at a bin size, one for each bin. None
    /// for "NONE", and for a column that the file does not have.
    pub fn weights(
        &self,
        reader: &mut (impl Read + Seek),
        bin: u32,
        chromosome: usize,
        name: &str,
    ) -> Result<Option<Vec<f64>>, Error> {
        if name == NONE || !self.normalizations.iter().any(|known| known == name) {
            return Ok(None);
        }
        let (low, high) = self.bins(reader, bin, chromosome)?;
        let column = self.dataset(reader, bin, &format!("bins/{name}"))?;
        column.floats(&self.file, reader, low..high).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{Cooler, is_cooler};
    use crate::hdf5::tests::{Builder, Values};
    use crate::hic::Contact;

    /// A cooler of two chromosomes at a bin size: chr1 of 4,000 and chr2 of 2,000 bases. The
    /// pixels are (bin1, bin2, count) in the bins of the whole genome.
    fn cooler(file: &mut Builder, bin: i64, pixels: &[(i64, i64, i32)]) -> u64 {
        let names = file.contiguous(Values::Text(&["chr1", "chr2"], 4));
        let lengths = file.contiguous(Values::I64(&[4_000, 2_000]));
        let chroms = file.group(&[("name", names), ("length", lengths)]);
        let counts = [4_000 / bin, 2_000 / bin];
        let total = counts[0] + counts[1];
        let starts: Vec<i64> = (0..counts[0])
            .chain(0..counts[1])
            .map(|index| index * bin)
            .collect();
        let ends: Vec<i64> = starts.iter().map(|start| start + bin).collect();
        let weights: Vec<f64> = (0..total).map(|index| 1.0 + index as f64).collect();
        let start = file.contiguous(Values::I64(&starts));
        let end = file.contiguous(Values::I64(&ends));
        let weight = file.contiguous(Values::F64(&weights));
        let bins = file.group(&[("start", start), ("end", end), ("weight", weight)]);
        let first: Vec<i64> = pixels.iter().map(|pixel| pixel.0).collect();
        let second: Vec<i64> = pixels.iter().map(|pixel| pixel.1).collect();
        let count: Vec<i32> = pixels.iter().map(|pixel| pixel.2).collect();
        let rows = file.chunked(Values::I64(&first), 3, true);
        let columns = file.chunked(Values::I64(&second), 3, true);
        let count = file.chunked(Values::I32(&count), 3, true);
        let pixels_group = file.group(&[("bin1_id", rows), ("bin2_id", columns), ("count", count)]);
        let chrom_offset = file.contiguous(Values::I64(&[0, counts[0], total]));
        let mut offsets = vec![0_i64; usize::try_from(total).unwrap() + 1];
        for (row, offset) in offsets.iter_mut().enumerate() {
            let row = i64::try_from(row).unwrap();
            *offset = i64::try_from(first.iter().filter(|bin| **bin < row).count()).unwrap();
        }
        let bin1_offset = file.contiguous(Values::I64(&offsets));
        let indexes = file.group(&[("chrom_offset", chrom_offset), ("bin1_offset", bin1_offset)]);
        file.group(&[
            ("chroms", chroms),
            ("bins", bins),
            ("pixels", pixels_group),
            ("indexes", indexes),
        ])
    }

    /// An `.mcool` file with bins of 1,000 and 2,000 bases.
    pub fn mcool() -> Vec<u8> {
        let mut file = Builder::new();
        let small = cooler(
            &mut file,
            1_000,
            &[
                (0, 0, 10),
                (0, 1, 5),
                (0, 3, 1),
                (1, 1, 8),
                (2, 3, 4),
                (3, 4, 7),
                (4, 5, 9),
            ],
        );
        let large = cooler(
            &mut file,
            2_000,
            &[(0, 0, 23), (0, 1, 5), (1, 1, 11), (2, 2, 9)],
        );
        let resolutions = file.group(&[("1000", small), ("2000", large)]);
        let root = file.group(&[("resolutions", resolutions)]);
        file.finish(root)
    }

    /// The test file of the other crates is the file of `mcool`. To write it again, run this test
    /// with `GENIE_WRITE_FIXTURES=1`.
    #[test]
    fn the_test_file_is_up_to_date() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/contacts.mcool");
        if std::env::var_os("GENIE_WRITE_FIXTURES").is_some() {
            std::fs::write(&path, mcool()).unwrap();
        }
        assert_eq!(std::fs::read(&path).unwrap(), mcool());
    }

    #[test]
    fn names_end_with_cool_or_mcool() {
        assert!(is_cooler("4DN.mcool") && is_cooler("sample.COOL"));
        assert!(!is_cooler("sample.hic"));
    }

    #[test]
    fn coolers_list_their_bin_sizes_chromosomes_and_weights() {
        let bytes = mcool();
        let mut reader = Cursor::new(&bytes);
        let cooler = Cooler::read(&mut reader).unwrap();
        assert_eq!(cooler.resolutions(), [1_000, 2_000]);
        assert_eq!(
            cooler.chromosomes,
            [("chr1".to_owned(), 4_000), ("chr2".to_owned(), 2_000)]
        );
        assert_eq!(cooler.normalizations, ["NONE", "weight"]);
    }

    fn found(contacts: &[Contact]) -> Vec<(u32, u32, f32)> {
        contacts
            .iter()
            .map(|contact| (contact.x, contact.y, contact.count))
            .collect()
    }

    #[test]
    fn contacts_stay_in_their_chromosome_window_and_reach() {
        let bytes = mcool();
        let mut reader = Cursor::new(&bytes);
        let cooler = Cooler::read(&mut reader).unwrap();
        let all = cooler.contacts(&mut reader, 1_000, 0, (0, 3), 10).unwrap();
        assert_eq!(
            found(&all),
            [
                (0, 0, 10.0),
                (0, 1, 5.0),
                (0, 3, 1.0),
                (1, 1, 8.0),
                (2, 3, 4.0)
            ]
        );
        let near = cooler.contacts(&mut reader, 1_000, 0, (1, 3), 1).unwrap();
        assert_eq!(found(&near), [(1, 1, 8.0), (2, 3, 4.0)]);
        let second = cooler.contacts(&mut reader, 1_000, 1, (0, 1), 5).unwrap();
        assert_eq!(found(&second), [(0, 1, 9.0)]);
        let coarse = cooler.contacts(&mut reader, 2_000, 1, (0, 0), 5).unwrap();
        assert_eq!(found(&coarse), [(0, 0, 9.0)]);
        assert!(cooler.contacts(&mut reader, 5_000, 0, (0, 1), 5).is_err());
    }

    #[test]
    fn weights_follow_the_bins_of_a_chromosome() {
        let bytes = mcool();
        let mut reader = Cursor::new(&bytes);
        let cooler = Cooler::read(&mut reader).unwrap();
        assert_eq!(
            cooler.weights(&mut reader, 1_000, 1, "weight").unwrap(),
            Some(vec![5.0, 6.0])
        );
        assert_eq!(cooler.weights(&mut reader, 1_000, 1, "NONE").unwrap(), None);
        assert_eq!(cooler.weights(&mut reader, 1_000, 1, "KR").unwrap(), None);
    }

    /// A real cooler file, as written by the cooler library: genie reads its chromosomes, all
    /// its pixels and its weights, on this computer and through a server. The balanced rows of
    /// the matrix have almost the same sum. Set `GENIE_REAL_COOL` to the file.
    #[test]
    #[ignore = "reads the real cooler file of GENIE_REAL_COOL"]
    fn real_cooler_file() {
        let path = std::env::var_os("GENIE_REAL_COOL").expect("GENIE_REAL_COOL names a file");
        let bytes = std::fs::read(path).unwrap();
        let mut reader = Cursor::new(&bytes);
        let cooler = Cooler::read(&mut reader).unwrap();
        println!("bin sizes {:?}", cooler.resolutions());
        println!("chromosomes {:?}", cooler.chromosomes);
        println!("normalizations {:?}", cooler.normalizations);
        let bin = cooler.resolutions()[0];
        let file = &cooler.file;
        let column = |reader: &mut Cursor<&Vec<u8>>, name: &str| {
            let dataset = file.dataset(reader, name).unwrap();
            dataset.floats(file, reader, 0..dataset.len).unwrap()
        };
        let places = |reader: &mut Cursor<&Vec<u8>>, name: &str| -> Vec<usize> {
            let dataset = file.dataset(reader, name).unwrap();
            let values = dataset.integers(file, reader, 0..dataset.len).unwrap();
            values
                .into_iter()
                .map(|value| usize::try_from(value).unwrap())
                .collect()
        };
        let firsts = places(&mut reader, "pixels/bin1_id");
        let seconds = places(&mut reader, "pixels/bin2_id");
        let counts = column(&mut reader, "pixels/count");
        let weights = column(&mut reader, "bins/weight");
        let offsets = places(&mut reader, "indexes/chrom_offset");
        println!("{} pixels, {} bins", firsts.len(), weights.len());
        let mut sums = vec![0.0; weights.len()];
        let mut cis = vec![(0_usize, 0.0); cooler.chromosomes.len()];
        let owner = |bin: usize| offsets.iter().rposition(|start| *start <= bin).unwrap();
        for ((&i, &j), count) in firsts.iter().zip(&seconds).zip(&counts) {
            if owner(i) == owner(j) {
                let found = &mut cis[owner(i)];
                *found = (found.0 + 1, found.1 + count);
            }
            let value = count * weights[i] * weights[j];
            if j >= i + 2 && value.is_finite() {
                sums[i] += value;
                sums[j] += value;
            }
        }
        let kept: Vec<f64> = sums
            .iter()
            .zip(&weights)
            .filter(|(_, weight)| weight.is_finite())
            .map(|(sum, _)| *sum)
            .collect();
        let (low, high) = kept.iter().fold((f64::MAX, f64::MIN), |(low, high), sum| {
            (low.min(*sum), high.max(*sum))
        });
        println!("{} balanced rows, sums from {low} to {high}", kept.len());
        for (index, (name, length)) in cooler.chromosomes.iter().enumerate() {
            let last = u32::try_from(length.div_ceil(u64::from(bin)) - 1).unwrap();
            let contacts = cooler
                .contacts(&mut reader, bin, index, (0, last), u32::MAX)
                .unwrap();
            let sum: f64 = contacts
                .iter()
                .map(|contact| f64::from(contact.count))
                .sum();
            assert_eq!((contacts.len(), sum), cis[index], "{name}");
        }
        assert!(high - low < 1e-3 * high, "{low} {high}");
        let server = crate::server::serve(vec![("/real.cool".to_owned(), bytes.clone())]);
        let url = crate::http::Url::parse(&server.url("/real.cool")).unwrap();
        let mut remote = crate::remote::Remote::open(&url).unwrap();
        let far = Cooler::read(&mut remote).unwrap();
        assert_eq!(far.chromosomes, cooler.chromosomes);
        let near = cooler.contacts(&mut reader, bin, 3, (0, 40), 20).unwrap();
        let through = far.contacts(&mut remote, bin, 3, (0, 40), 20).unwrap();
        assert_eq!(found(&through), found(&near));
        println!("{} requests to the server", server.requests());
    }

    #[test]
    fn single_coolers_take_their_bin_size_from_the_bins() {
        let mut file = Builder::new();
        let root = cooler(&mut file, 500, &[(0, 0, 3)]);
        let bytes = file.finish(root);
        let mut reader = Cursor::new(&bytes);
        let cooler = Cooler::read(&mut reader).unwrap();
        assert_eq!(cooler.resolutions(), [500]);
        let contacts = cooler.contacts(&mut reader, 500, 0, (0, 2), 2).unwrap();
        assert_eq!(found(&contacts), [(0, 0, 3.0)]);
    }
}
