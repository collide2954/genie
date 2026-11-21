//! CRAI indexes: gzip text with one line per slice and reference.

use std::io::Read;

use genie_core::Region;
use genie_core::signal::Tally;

use crate::{Error, Reason, gzip};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub reference: i32,
    pub start: i64,
    pub span: i64,
    pub container: u64,
    pub slice: u64,
    pub size: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    pub entries: Vec<Entry>,
}

impl Index {
    pub fn read(mut reader: impl Read) -> Result<Self, Error> {
        let mut packed = Vec::new();
        reader.read_to_end(&mut packed)?;
        let text = gzip::inflate(&packed)?;
        let mut entries = Vec::new();
        for line in String::from_utf8_lossy(&text).lines() {
            if line.trim().is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            let [reference, start, span, container, slice, size] = fields[..] else {
                return Err(Error::Invalid(Reason::BadIndex));
            };
            let number = |text: &str| {
                text.trim()
                    .parse::<i64>()
                    .map_err(|_| Error::Invalid(Reason::BadIndex))
            };
            let offset = |text: &str| {
                u64::try_from(number(text)?).map_err(|_| Error::Invalid(Reason::BadIndex))
            };
            entries.push(Entry {
                reference: i32::try_from(number(reference)?)
                    .map_err(|_| Error::Invalid(Reason::BadIndex))?,
                start: number(start)?,
                span: number(span)?,
                container: offset(container)?,
                slice: offset(slice)?,
                size: offset(size)?,
            });
        }
        Ok(Self { entries })
    }

    /// The (container, slice) offsets of the slices with reads of a reference in a region.
    pub fn slices(&self, reference: i32, region: Region) -> Vec<(u64, u64)> {
        let mut found: Vec<(u64, u64)> = self
            .entries
            .iter()
            .filter(|entry| {
                let start = entry.start - 1;
                let end = start + entry.span.max(1);
                entry.reference == reference
                    && start < region.end().cast_signed()
                    && region.start().cast_signed() < end
            })
            .map(|entry| (entry.container, entry.slice))
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }

    /// An estimate of the depth in each part of a region of a reference, from the index alone,
    /// as goleft indexcov finds it: the bytes of each slice spread over the bases that it covers.
    /// Nothing for a reference without slices.
    pub fn density(&self, reference: i32, region: Region, step: u64) -> Option<Tally> {
        let mut tally = Tally::new(region, step);
        let mut found = false;
        for entry in self
            .entries
            .iter()
            .filter(|entry| entry.reference == reference)
        {
            found = true;
            let (Ok(start), Ok(span)) = (u64::try_from(entry.start - 1), u64::try_from(entry.span))
            else {
                continue;
            };
            if span > 0 {
                tally.spread(Region::between(start, start + span), entry.size as f64);
            }
        }
        found.then_some(tally)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::path::Path;

    use genie_core::Region;

    use super::{Entry, Index};
    use crate::gzip;

    #[test]
    fn indexes_list_each_slice() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/cram30.cram.crai");
        let index = Index::read(File::open(path).unwrap()).unwrap();
        assert_eq!(index.entries.len(), 12);
        assert_eq!(index.entries[0].reference, 0);
        assert_eq!(index.entries.last().unwrap().reference, -1);
        let all = index.slices(0, Region::between(0, 120_000));
        assert_eq!(all.len(), 7);
        assert_eq!(index.slices(2, Region::between(0, 100)).len(), 0);
    }

    #[test]
    fn regions_find_the_slices_that_overlap_them() {
        let text = "0\t101\t100\t500\t10\t50\n0\t181\t100\t500\t60\t50\n1\t1\t50\t900\t5\t40\n";
        let index = Index::read(&gzip::compress(text.as_bytes())[..]).unwrap();
        assert_eq!(
            index.entries[1],
            Entry {
                reference: 0,
                start: 181,
                span: 100,
                container: 500,
                slice: 60,
                size: 50,
            }
        );
        assert_eq!(index.slices(0, Region::between(0, 100)), []);
        assert_eq!(index.slices(0, Region::between(100, 101)), [(500, 10)]);
        assert_eq!(
            index.slices(0, Region::between(150, 200)),
            [(500, 10), (500, 60)]
        );
        assert_eq!(index.slices(1, Region::between(0, 10)), [(900, 5)]);
        assert!(Index::read(&gzip::compress(b"0\t1\t2\n")[..]).is_err());
    }

    /// The bytes of each slice spread over the bases that it covers estimate the depth, as
    /// goleft indexcov finds it.
    #[test]
    fn slices_spread_their_bytes_over_their_bases() {
        let text = "0\t101\t100\t500\t10\t50\n0\t181\t100\t500\t60\t50\n1\t1\t50\t900\t5\t40\n";
        let index = Index::read(&gzip::compress(text.as_bytes())[..]).unwrap();
        let tally = index.density(0, Region::between(0, 300), 100).unwrap();
        let bytes: Vec<f64> = tally
            .summaries()
            .iter()
            .map(|summary| f64::from(summary.max) * 100.0 / 1e6)
            .collect();
        let wanted = [0.0, 50.0 + 0.2 * 50.0, 0.8 * 50.0];
        for (found, wanted) in bytes.iter().zip(wanted) {
            assert!((found - wanted).abs() < 1e-3, "{bytes:?}");
        }
        assert!((tally.total() - 100.0).abs() < 1e-9);
        assert!(index.density(3, Region::between(0, 10), 1).is_none());
    }
}
