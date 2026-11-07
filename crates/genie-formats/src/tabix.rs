use std::io::{BufRead, Read, Seek};

use genie_core::Region;

pub use crate::binning::Chunk;
use crate::binning::{Bins, read_count, read_u32};
use crate::{Error, Reason, bgzf::Bgzf};

const MAGIC: &[u8; 4] = b"TBI\x01";
const ZERO_BASED: u32 = 0x10000;

#[derive(Debug, Default)]
pub struct Index {
    zero_based: bool,
    columns: [usize; 3],
    meta: u8,
    names: Vec<String>,
    references: Vec<Bins>,
}

impl Index {
    pub fn read(reader: impl Read) -> Result<Self, Error> {
        let mut reader = Bgzf::new(reader);
        let mut magic = [0; 4];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(Error::Invalid(Reason::NotTabix));
        }
        let count = read_count(&mut reader)?;
        let format = read_u32(&mut reader)?;
        let columns = [
            read_count(&mut reader)?,
            read_count(&mut reader)?,
            read_count(&mut reader)?,
        ];
        let meta = read_u32(&mut reader)? as u8;
        read_count(&mut reader)?;
        let length = read_count(&mut reader)?;
        let mut text = Vec::new();
        reader.by_ref().take(length as u64).read_to_end(&mut text)?;
        let names = names(&text).ok_or(Error::Invalid(Reason::BadIndex))?;
        if text.len() != length || names.len() != count || columns[0] == 0 || columns[1] == 0 {
            return Err(Error::Invalid(Reason::BadIndex));
        }
        let references = (0..count)
            .map(|_| Bins::read(&mut reader))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            zero_based: format & ZERO_BASED != 0,
            columns,
            meta,
            names,
            references,
        })
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn chunks(&self, name: &str, region: Region) -> Vec<Chunk> {
        self.names
            .iter()
            .position(|known| known == name)
            .map_or_else(Vec::new, |id| self.references[id].chunks(region))
    }

    pub fn query<R: Read + Seek>(
        &self,
        reader: &mut Bgzf<R>,
        name: &str,
        region: Region,
        mut each: impl FnMut(&str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut bytes = Vec::new();
        for chunk in self.chunks(name, region) {
            if reader.virtual_offset() != chunk.start {
                reader.seek(chunk.start)?;
            }
            while reader.virtual_offset() < chunk.end {
                bytes.clear();
                if reader.read_until(b'\n', &mut bytes)? == 0 {
                    break;
                }
                let text = String::from_utf8_lossy(&bytes);
                let line = text.trim_end_matches(['\n', '\r']);
                if line
                    .as_bytes()
                    .first()
                    .is_none_or(|first| *first == self.meta)
                {
                    continue;
                }
                let (record, span) = self.locate(line).ok_or(Error::Invalid(Reason::BadNumber))?;
                if record != name || span.start() >= region.end() {
                    return Ok(());
                }
                if span.end() > region.start() {
                    each(line)?;
                }
            }
        }
        Ok(())
    }

    fn locate<'a>(&self, line: &'a str) -> Option<(&'a str, Region)> {
        let [name, start, end] = self.columns;
        let field = |column: usize| line.split('\t').nth(column - 1);
        let first: u64 = field(start)?.parse().ok()?;
        let first = if self.zero_based {
            first
        } else {
            first.saturating_sub(1)
        };
        let last = if end == 0 {
            first + 1
        } else {
            field(end)?.parse().ok()?
        };
        Some((field(name)?, Region::new(first, last.max(first))?))
    }
}

fn names(text: &[u8]) -> Option<Vec<String>> {
    match text.split_last() {
        None => Some(Vec::new()),
        Some((0, rest)) => Some(
            rest.split(|byte| *byte == 0)
                .map(|name| String::from_utf8_lossy(name).into_owned())
                .collect(),
        ),
        Some(_) => None,
    }
}

#[cfg(test)]
pub mod tests {
    use std::io::Cursor;

    use genie_core::Region;

    use super::{Chunk, Index};
    use crate::{
        Error, Reason,
        bgzf::{
            Bgzf,
            tests::{block, file},
        },
    };

    pub const BED: [usize; 4] = [0x10000, 1, 2, 3];
    pub const GFF: [usize; 4] = [0, 1, 4, 5];

    pub type Bins<'a> = &'a [(u32, &'a [(u64, u64)])];

    fn put(bytes: &mut Vec<u8>, value: usize) {
        bytes.extend(i32::try_from(value).unwrap().to_le_bytes());
    }

    pub fn index(preset: [usize; 4], names: &[&str], references: &[(Bins, &[u64])]) -> Index {
        let text: Vec<u8> = names
            .iter()
            .flat_map(|name| name.bytes().chain([0]))
            .collect();
        let mut bytes = b"TBI\x01".to_vec();
        put(&mut bytes, names.len());
        for value in preset {
            put(&mut bytes, value);
        }
        for value in [usize::from(b'#'), 0, text.len()] {
            put(&mut bytes, value);
        }
        bytes.extend(text);
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
        Index::read(Cursor::new(file(&[&bytes]))).unwrap()
    }

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    fn chunk(start: u64, end: u64) -> Chunk {
        Chunk { start, end }
    }

    #[test]
    fn reads_reference_names() {
        let index = index(BED, &["chr1", "chr2"], &[(&[], &[]), (&[], &[])]);
        assert_eq!(index.names(), ["chr1", "chr2"]);
    }

    #[test]
    fn rejects_files_that_are_not_indexes() {
        let result = Index::read(Cursor::new(file(&[b"BAI\x01\0\0\0\0"])));
        assert!(matches!(result, Err(Error::Invalid(Reason::NotTabix))));
    }

    #[test]
    fn reports_plain_files_as_not_bgzip() {
        let result = Index::read(Cursor::new(b"TBI\x01\0\0\0\0".to_vec()));
        assert!(matches!(result, Err(Error::Invalid(Reason::NotBgzf))));
    }

    #[test]
    fn merges_chunks_of_overlapping_bins() {
        let bins: Bins = &[(0, &[(10, 20)]), (4681, &[(15, 30)]), (4682, &[(40, 50)])];
        let index = index(BED, &["chr1"], &[(bins, &[0, 0])]);
        assert_eq!(index.chunks("chr1", region(0, 100)), [chunk(10, 30)]);
        assert_eq!(
            index.chunks("chr1", region(0, 20_000)),
            [chunk(10, 30), chunk(40, 50)]
        );
    }

    #[test]
    fn drops_chunks_before_the_linear_index() {
        let bins: Bins = &[(0, &[(10, 20)]), (4682, &[(40, 50)])];
        let index = index(BED, &["chr1"], &[(bins, &[0, 35])]);
        assert_eq!(
            index.chunks("chr1", region(16_384, 16_500)),
            [chunk(40, 50)]
        );
    }

    #[test]
    fn has_no_chunks_for_unknown_names() {
        let bins: Bins = &[(0, &[(10, 20)])];
        let index = index(BED, &["chr1"], &[(bins, &[0])]);
        assert_eq!(index.chunks("chr9", region(0, 100)), []);
    }

    fn lines(preset: [usize; 4], parts: &[&[u8]], query: Region) -> Vec<String> {
        let end = (parts.iter().map(|part| block(part).len()).sum::<usize>() as u64) << 16;
        let bins: Bins = &[(4681, &[(0, end)])];
        let index = index(preset, &["chr1"], &[(bins, &[0])]);
        let mut reader = Bgzf::new(Cursor::new(file(parts)));
        let mut found = Vec::new();
        index
            .query(&mut reader, "chr1", query, |line| {
                found.push(line.to_owned());
                Ok(())
            })
            .unwrap();
        found
    }

    #[test]
    fn queries_lines_that_overlap_a_region() {
        let parts: &[&[u8]] = &[
            b"#chrom\nchr1\t10\t20\ta\nchr1\t30\t40\tb\n",
            b"chr1\t50\t60\tc\n",
        ];
        assert_eq!(
            lines(BED, parts, region(35, 55)),
            ["chr1\t30\t40\tb", "chr1\t50\t60\tc"]
        );
        assert_eq!(lines(BED, parts, region(0, 15)), ["chr1\t10\t20\ta"]);
        assert_eq!(lines(BED, parts, region(20, 30)), Vec::<String>::new());
    }

    #[test]
    fn converts_one_based_columns() {
        let parts: &[&[u8]] = &[b"chr1\t.\tgene\t11\t20\t.\t+\t.\tID=a\r\n"];
        assert_eq!(
            lines(GFF, parts, region(19, 25)),
            ["chr1\t.\tgene\t11\t20\t.\t+\t.\tID=a"]
        );
        assert_eq!(lines(GFF, parts, region(20, 25)), Vec::<String>::new());
        assert_eq!(lines(GFF, parts, region(0, 10)), Vec::<String>::new());
    }
}
