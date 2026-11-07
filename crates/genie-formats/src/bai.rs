use std::io::{BufReader, Read};

use genie_core::Region;

use crate::binning::{Bins, Chunk, read_count};
use crate::{Error, Reason};

const MAGIC: &[u8; 4] = b"BAI\x01";

#[derive(Debug, Default)]
pub struct Index {
    references: Vec<Bins>,
}

impl Index {
    pub fn read(reader: impl Read) -> Result<Self, Error> {
        let mut reader = BufReader::new(reader);
        let mut magic = [0; 4];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(Error::Invalid(Reason::NotBai));
        }
        let count = read_count(&mut reader)?;
        let references = (0..count)
            .map(|_| Bins::read(&mut reader))
            .collect::<Result<_, _>>()?;
        Ok(Self { references })
    }

    pub fn len(&self) -> usize {
        self.references.len()
    }

    pub fn is_empty(&self) -> bool {
        self.references.is_empty()
    }

    pub fn chunks(&self, reference: usize, region: Region) -> Vec<Chunk> {
        self.references
            .get(reference)
            .map_or_else(Vec::new, |bins| bins.chunks(region))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::Region;

    use super::Index;
    use crate::{Error, Reason, binning::Chunk, tabix::tests::Bins};

    fn put(bytes: &mut Vec<u8>, value: usize) {
        bytes.extend(i32::try_from(value).unwrap().to_le_bytes());
    }

    fn index(references: &[(Bins, &[u64])]) -> Vec<u8> {
        let mut bytes = b"BAI\x01".to_vec();
        put(&mut bytes, references.len());
        for (bins, intervals) in references {
            put(&mut bytes, bins.len());
            for (bin, chunks) in *bins {
                bytes.extend(bin.to_le_bytes());
                put(&mut bytes, chunks.len());
                for (start, end) in *chunks {
                    bytes.extend(start.to_le_bytes());
                    bytes.extend(end.to_le_bytes());
                }
            }
            put(&mut bytes, intervals.len());
            for offset in *intervals {
                bytes.extend(offset.to_le_bytes());
            }
        }
        bytes.extend(0_u64.to_le_bytes());
        bytes
    }

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    #[test]
    fn finds_chunks_by_reference_number() {
        let bins: Bins = &[(4681, &[(10, 20)]), (37_450, &[(1, 2), (3, 4)])];
        let bytes = index(&[(&[], &[]), (bins, &[0])]);
        let index = Index::read(Cursor::new(bytes)).unwrap();
        assert_eq!(index.len(), 2);
        assert_eq!(
            index.chunks(1, region(0, 100)),
            [Chunk { start: 10, end: 20 }]
        );
        assert_eq!(index.chunks(0, region(0, 100)), []);
        assert_eq!(index.chunks(7, region(0, 100)), []);
    }

    #[test]
    fn rejects_files_that_are_not_bai() {
        let result = Index::read(Cursor::new(b"TBI\x01\0\0\0\0".to_vec()));
        assert!(matches!(result, Err(Error::Invalid(Reason::NotBai))));
    }
}
