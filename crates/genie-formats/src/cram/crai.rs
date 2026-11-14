//! CRAI indexes: gzip text with one line per slice and reference.

use std::io::Read;

use genie_core::Region;

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
}
