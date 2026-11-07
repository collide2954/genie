use std::io::{Read, Seek, SeekFrom};

use genie_core::Region;
use genie_core::signal::Summary;
use miniz_oxide::inflate::decompress_to_vec_zlib_with_limit;

use crate::{Error, Reason};

const MAGIC: u32 = 0x888F_FC26;
const CHROMOSOMES: u32 = 0x78CA_8C91;
const BLOCKS: u32 = 0x2468_ACE0;
const HEADER: usize = 64;
const DEPTH: usize = 32;
const KEY_LIMIT: usize = 256;
const BLOCK_LIMIT: u64 = 1 << 26;

#[derive(Debug, Clone)]
pub struct Index {
    big: bool,
    buffer: u32,
    names: Vec<String>,
    ids: Vec<u32>,
    full: u64,
}

struct Bytes<'a> {
    data: &'a [u8],
    at: usize,
    big: bool,
}

impl<'a> Bytes<'a> {
    fn new(data: &'a [u8], big: bool) -> Self {
        Self { data, at: 0, big }
    }

    fn skip(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let bytes = self
            .data
            .get(self.at..self.at + count)
            .ok_or(Error::Invalid(Reason::BadBlock))?;
        self.at += count;
        Ok(bytes)
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut bytes = [0; N];
        bytes.copy_from_slice(self.skip(N)?);
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let bytes = self.take()?;
        Ok(if self.big {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        })
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let bytes = self.take()?;
        Ok(if self.big {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        })
    }

    fn u64(&mut self) -> Result<u64, Error> {
        let bytes = self.take()?;
        Ok(if self.big {
            u64::from_be_bytes(bytes)
        } else {
            u64::from_le_bytes(bytes)
        })
    }

    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }
}

fn chunk(reader: &mut (impl Read + Seek), offset: u64, size: usize) -> Result<Vec<u8>, Error> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn node(
    reader: &mut (impl Read + Seek),
    offset: u64,
    big: bool,
    item: impl Fn(bool) -> usize,
) -> Result<(bool, usize, Vec<u8>), Error> {
    let head = chunk(reader, offset, 4)?;
    let mut bytes = Bytes::new(&head, big);
    let leaf = bytes.u8()? != 0;
    let _reserved = bytes.u8()?;
    let count = usize::from(bytes.u16()?);
    let items = chunk(reader, offset + 4, count * item(leaf))?;
    Ok((leaf, count, items))
}

fn chromosomes(
    reader: &mut (impl Read + Seek),
    offset: u64,
    big: bool,
) -> Result<Vec<(String, u32)>, Error> {
    let head = chunk(reader, offset, 32)?;
    let mut bytes = Bytes::new(&head, big);
    let bad = Error::Invalid(Reason::BadIndex);
    if bytes.u32()? != CHROMOSOMES {
        return Err(bad);
    }
    let _block = bytes.u32()?;
    let key = bytes.u32()? as usize;
    if key > KEY_LIMIT || bytes.u32()? != 8 {
        return Err(bad);
    }
    let mut found = Vec::new();
    let mut nodes = vec![(offset + 32, 0)];
    while let Some((at, depth)) = nodes.pop() {
        if depth > DEPTH {
            return Err(Error::Invalid(Reason::BadIndex));
        }
        let (leaf, count, items) = node(reader, at, big, |_| key + 8)?;
        let mut bytes = Bytes::new(&items, big);
        for _ in 0..count {
            let name = bytes.skip(key)?;
            if leaf {
                let name = name.split(|byte| *byte == 0).next().unwrap_or_default();
                let id = bytes.u32()?;
                let _length = bytes.u32()?;
                found.push((String::from_utf8_lossy(name).into_owned(), id));
            } else {
                nodes.push((bytes.u64()?, depth + 1));
            }
        }
    }
    Ok(found)
}

fn blocks(
    reader: &mut (impl Read + Seek),
    offset: u64,
    big: bool,
    id: u32,
    region: Region,
) -> Result<Vec<(u64, u64)>, Error> {
    let head = chunk(reader, offset, 48)?;
    if Bytes::new(&head, big).u32()? != BLOCKS {
        return Err(Error::Invalid(Reason::BadIndex));
    }
    let (first, last) = ((id, region.start()), (id, region.end()));
    let mut found = Vec::new();
    let mut nodes = vec![(offset + 48, 0)];
    while let Some((at, depth)) = nodes.pop() {
        if depth > DEPTH {
            return Err(Error::Invalid(Reason::BadIndex));
        }
        let (leaf, count, items) = node(reader, at, big, |leaf| if leaf { 32 } else { 24 })?;
        let mut bytes = Bytes::new(&items, big);
        let mut children = Vec::new();
        for _ in 0..count {
            let start = (bytes.u32()?, u64::from(bytes.u32()?));
            let end = (bytes.u32()?, u64::from(bytes.u32()?));
            let overlaps = first < end && start < last;
            let target = bytes.u64()?;
            if leaf {
                let size = bytes.u64()?;
                if overlaps {
                    found.push((target, size));
                }
            } else if overlaps {
                children.push((target, depth + 1));
            }
        }
        nodes.extend(children.into_iter().rev());
    }
    Ok(found)
}

fn section(block: &[u8], big: bool, id: u32, region: Region) -> Result<Vec<Summary>, Error> {
    let mut bytes = Bytes::new(block, big);
    let chromosome = bytes.u32()?;
    let start = bytes.u32()?;
    let _end = bytes.u32()?;
    let step = bytes.u32()?;
    let span = bytes.u32()?;
    let kind = bytes.u8()?;
    let _reserved = bytes.u8()?;
    let count = bytes.u16()?;
    let mut found = Vec::new();
    if chromosome != id {
        return Ok(found);
    }
    let mut position = start;
    for _ in 0..count {
        let (from, to) = match kind {
            1 => (bytes.u32()?, bytes.u32()?),
            2 => {
                let from = bytes.u32()?;
                (from, from.saturating_add(span))
            }
            3 => {
                let from = position;
                position = position.saturating_add(step);
                (from, from.saturating_add(span))
            }
            _ => return Err(Error::Invalid(Reason::BadBlock)),
        };
        let value = bytes.f32()?;
        let clipped =
            Region::new(u64::from(from), u64::from(to)).and_then(|span| span.intersection(region));
        found.extend(clipped.map(|span| Summary::single(span, value)));
    }
    Ok(found)
}

impl Index {
    pub fn read(reader: &mut (impl Read + Seek)) -> Result<Self, Error> {
        let header = chunk(reader, 0, HEADER)?;
        let magic = [header[0], header[1], header[2], header[3]];
        let big = if u32::from_le_bytes(magic) == MAGIC {
            false
        } else if u32::from_be_bytes(magic) == MAGIC {
            true
        } else {
            return Err(Error::Invalid(Reason::NotBigWig));
        };
        let mut bytes = Bytes::new(&header, big);
        let _magic = bytes.u32()?;
        let _version = bytes.u16()?;
        let _levels = bytes.u16()?;
        let tree = bytes.u64()?;
        let _data = bytes.u64()?;
        let full = bytes.u64()?;
        let _fields = bytes.u32()?;
        let _schema = bytes.u64()?;
        let _total = bytes.u64()?;
        let buffer = bytes.u32()?;
        let (names, ids) = chromosomes(reader, tree, big)?.into_iter().unzip();
        Ok(Self {
            big,
            buffer,
            names,
            ids,
            full,
        })
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    fn id(&self, contig: &str) -> Option<u32> {
        let slot = self.names.iter().position(|name| name == contig)?;
        Some(self.ids[slot])
    }

    fn block(
        &self,
        reader: &mut (impl Read + Seek),
        offset: u64,
        size: u64,
    ) -> Result<Vec<u8>, Error> {
        if size > BLOCK_LIMIT {
            return Err(Error::Invalid(Reason::BadBlock));
        }
        let raw = chunk(reader, offset, size as usize)?;
        if self.buffer == 0 {
            return Ok(raw);
        }
        decompress_to_vec_zlib_with_limit(&raw, self.buffer as usize)
            .map_err(|_| Error::Invalid(Reason::BadBlock))
    }

    pub fn records(
        &self,
        reader: &mut (impl Read + Seek),
        contig: &str,
        region: Region,
    ) -> Result<Vec<Summary>, Error> {
        let Some(id) = self.id(contig) else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for (offset, size) in blocks(reader, self.full, self.big, id, region)? {
            let block = self.block(reader, offset, size)?;
            found.extend(section(&block, self.big, id, region)?);
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::Index;
    use crate::{Error, Reason};

    #[test]
    fn other_files_are_not_bigwig() {
        let error = Index::read(&mut Cursor::new(vec![0; 64])).unwrap_err();
        assert!(matches!(error, Error::Invalid(Reason::NotBigWig)));
    }
}
