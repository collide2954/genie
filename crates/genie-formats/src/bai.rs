use std::io::{self, BufRead, BufReader, Read, Write};

use genie_core::Region;

use crate::bam::{self, Header};
use crate::bgzf::{self, Bgzf};
use crate::binning::{Bins, Builder, Chunk, Scheme, put, read_count, read_u32};
use crate::{Error, Reason};

const MAGIC: &[u8; 4] = b"BAI\x01";
const CSI: &[u8; 4] = b"CSI\x01";
const SHIFT: u32 = 14;
const LONGEST: u64 = 1 << 29;
const LENGTH_MARGIN: u64 = 256;

pub struct Indexer {
    builder: Builder,
    scheme: Scheme,
    count: usize,
}

impl Indexer {
    pub fn new(header: &Header, start: u64) -> Self {
        let longest = header
            .references
            .iter()
            .map(|(_, length)| *length)
            .max()
            .unwrap_or(0);
        let scheme = if longest <= LONGEST {
            Scheme::BAI
        } else {
            Scheme::reaching(SHIFT, 0, longest + LENGTH_MARGIN)
        };
        Self {
            builder: Builder::new(scheme, scheme != Scheme::BAI, start),
            scheme,
            count: header.references.len(),
        }
    }

    pub fn add(&mut self, record: &[u8], end: u64) -> Result<(), Error> {
        let place = bam::place(record)?;
        let at = place
            .reference
            .map(|reference| (reference, place.start, place.end));
        self.builder
            .push(at, end, place.mapped)
            .map_err(|disorder| Error::Invalid(disorder.reason()))
    }

    pub fn finish(self, end: u64) -> Result<Vec<u8>, Error> {
        let csi = self.scheme != Scheme::BAI;
        let (mut references, unplaced) = self.builder.finish(end);
        if references.len() < self.count {
            references.resize_with(self.count, || Bins::empty(self.scheme, csi));
        }
        let mut bytes = Vec::new();
        if csi {
            bytes.extend(CSI);
            bytes.extend(self.scheme.shift().to_le_bytes());
            bytes.extend(self.scheme.depth().to_le_bytes());
            put(&mut bytes, 0);
        } else {
            bytes.extend(MAGIC);
        }
        put(&mut bytes, references.len());
        for bins in &references {
            bins.write(&mut bytes);
        }
        bytes.extend(unplaced.to_le_bytes());
        if !csi {
            return Ok(bytes);
        }
        let mut writer = bgzf::Writer::new(Vec::new());
        writer.write_all(&bytes)?;
        Ok(writer.finish()?)
    }
}

pub fn build(reader: impl Read) -> Result<Vec<u8>, Error> {
    let mut reader = Bgzf::new(reader);
    let header = bam::header(&mut reader)?;
    let mut end = reader.virtual_offset();
    let mut indexer = Indexer::new(&header, end);
    let mut buffer = Vec::new();
    while bam::raw(&mut reader, &mut buffer)? {
        end = reader.virtual_offset();
        indexer.add(&buffer, end)?;
    }
    indexer.finish(end)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Index {
    references: Vec<Bins>,
}

impl Index {
    pub fn read(reader: impl Read) -> Result<Self, Error> {
        let mut reader = BufReader::new(reader);
        if bgzf::sniff(reader.fill_buf()?) {
            return Self::read_csi(Bgzf::new(reader));
        }
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

    fn read_csi(mut reader: impl Read) -> Result<Self, Error> {
        let mut magic = [0; 4];
        reader.read_exact(&mut magic)?;
        if &magic != CSI {
            return Err(Error::Invalid(Reason::NotBai));
        }
        let shift = read_u32(&mut reader)?.cast_signed();
        let scheme = Scheme::new(shift, read_u32(&mut reader)?.cast_signed())?;
        let length = read_count(&mut reader)? as u64;
        io::copy(&mut reader.by_ref().take(length), &mut io::sink())?;
        let references = (0..read_count(&mut reader)?)
            .map(|_| Bins::read_csi(&mut reader, scheme))
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
pub mod tests {
    use std::io::Cursor;

    use genie_core::Region;

    use super::Index;
    use crate::{
        Error, Reason,
        binning::Chunk,
        tabix::tests::{Bins, CsiBins, csi},
    };

    fn put(bytes: &mut Vec<u8>, value: usize) {
        bytes.extend(i32::try_from(value).unwrap().to_le_bytes());
    }

    pub fn index(references: &[(Bins, &[u64])]) -> Vec<u8> {
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
    fn reads_csi_indexes_for_bam() {
        let bins: CsiBins = &[(4681, 0, &[(10, 20)])];
        let bytes = csi(14, 5, &[], &[&[], bins]);
        let index = Index::read(Cursor::new(bytes)).unwrap();
        assert_eq!(index.len(), 2);
        assert_eq!(
            index.chunks(1, region(0, 100)),
            [Chunk { start: 10, end: 20 }]
        );
    }

    #[test]
    fn rejects_files_that_are_not_bai() {
        let result = Index::read(Cursor::new(b"TBI\x01\0\0\0\0".to_vec()));
        assert!(matches!(result, Err(Error::Invalid(Reason::NotBai))));
    }
}
