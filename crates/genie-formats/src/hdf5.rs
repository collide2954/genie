//! A reader of the parts of HDF5 files that cooler files use: groups, and one-dimensional
//! datasets of numbers and fixed-length texts, stored whole or in chunks that the deflate and
//! shuffle filters compress.

use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;

use crate::{Error, Reason};

const SIGNATURE: &[u8; 8] = b"\x89HDF\r\n\x1a\n";
/// The places where a superblock can start: after a user block of 0, 512, 1024, 2048 … bytes.
const STARTS: [u64; 6] = [0, 512, 1_024, 2_048, 4_096, 8_192];
/// The most bytes that genie reads at once, so that a broken size cannot fill the memory.
const MOST: u64 = 1 << 28;
/// The most blocks of messages, nodes and steps that genie follows, so that a broken file
/// cannot make it loop.
const STEPS: usize = 100_000;
/// The message types that genie reads.
const DATASPACE: u16 = 0x0001;
const DATATYPE: u16 = 0x0003;
const LAYOUT: u16 = 0x0008;
const FILTERS: u16 = 0x000B;
const LINK: u16 = 0x0006;
const CONTINUATION: u16 = 0x0010;
const SYMBOLS: u16 = 0x0011;

fn invalid() -> Error {
    Error::Invalid(Reason::BadHdf5)
}

fn unknown() -> Error {
    Error::Invalid(Reason::Hdf5Feature)
}

/// Bytes of a file at an address.
fn read_at(reader: &mut (impl Read + Seek), address: u64, length: u64) -> Result<Vec<u8>, Error> {
    if length > MOST {
        return Err(invalid());
    }
    reader.seek(SeekFrom::Start(address))?;
    let mut bytes = vec![0; usize::try_from(length).map_err(|_| invalid())?];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// A reader of the little-endian fields of a part of a file.
struct Fields<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Fields<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let part = self
            .bytes
            .get(self.at..self.at.checked_add(count).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        self.at += count;
        Ok(part)
    }

    fn skip(&mut self, count: usize) -> Result<(), Error> {
        self.take(count).map(|_| ())
    }

    fn number(&mut self, size: usize) -> Result<u64, Error> {
        let part = self.take(size)?;
        if size > 8 {
            return Err(invalid());
        }
        let mut bytes = [0; 8];
        bytes[..size].copy_from_slice(part);
        Ok(u64::from_le_bytes(bytes))
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, Error> {
        self.number(2).map(|value| value as u16)
    }

    fn u32(&mut self) -> Result<u32, Error> {
        self.number(4).map(|value| value as u32)
    }

    fn left(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }
}

/// The parts of a value of a type, and the value as a float or a whole number.
fn decode(kind: Kind, bytes: &[u8]) -> (f64, i64) {
    let ordered: Vec<u8> = match kind {
        Kind::Integer { big: true, .. } | Kind::Float { big: true, .. } => {
            bytes.iter().rev().copied().collect()
        }
        _ => bytes.to_vec(),
    };
    match kind {
        Kind::Integer { size, signed, .. } => {
            let mut buffer = [0; 8];
            buffer[..size.min(8)].copy_from_slice(&ordered[..size.min(8)]);
            let raw = u64::from_le_bytes(buffer);
            let value = if signed && size < 8 {
                let shift = 64 - size * 8;
                (raw << shift).cast_signed() >> shift
            } else {
                raw.cast_signed()
            };
            (value as f64, value)
        }
        Kind::Float { size: 4, .. } => {
            let value = f64::from(f32::from_le_bytes([
                ordered[0], ordered[1], ordered[2], ordered[3],
            ]));
            (value, value as i64)
        }
        Kind::Float { .. } => {
            let mut buffer = [0; 8];
            buffer.copy_from_slice(&ordered[..8]);
            let value = f64::from_le_bytes(buffer);
            (value, value as i64)
        }
        Kind::Text { .. } => (f64::NAN, 0),
    }
}

impl Kind {
    fn size(self) -> usize {
        match self {
            Self::Integer { size, .. } | Self::Float { size, .. } | Self::Text { size } => size,
        }
    }

    fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut fields = Fields::new(data);
        let class = fields.u8()? & 0x0F;
        let bits = fields.take(3)?;
        let size = fields.u32()? as usize;
        let big = bits[0] & 1 != 0;
        match (class, size) {
            (0, 1 | 2 | 4 | 8) => Ok(Self::Integer {
                size,
                signed: bits[0] & 0x08 != 0,
                big,
            }),
            (1, 4 | 8) => Ok(Self::Float { size, big }),
            (3, 1..) => Ok(Self::Text { size }),
            _ => Err(unknown()),
        }
    }
}

/// The values of a chunk after the filters, in the reverse order of the pipeline.
fn unfilter(
    mut data: Vec<u8>,
    filters: &[Filter],
    mask: u32,
    size: usize,
) -> Result<Vec<u8>, Error> {
    for (place, filter) in filters.iter().enumerate().rev() {
        if mask & (1 << place) != 0 {
            continue;
        }
        data = match filter {
            Filter::Deflate => {
                miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&data, MOST as usize)
                    .map_err(|_| invalid())?
            }
            Filter::Shuffle => unshuffle(&data, size),
            Filter::Fletcher => {
                let keep = data.len().checked_sub(4).ok_or_else(invalid)?;
                data.truncate(keep);
                data
            }
        };
    }
    Ok(data)
}

/// The bytes of values that the shuffle filter stored: the first byte of each value, then the
/// second, and so on. Bytes after the last whole value stay at the end.
fn unshuffle(data: &[u8], size: usize) -> Vec<u8> {
    if size <= 1 {
        return data.to_vec();
    }
    let count = data.len() / size;
    let mut out = data.to_vec();
    for value in 0..count {
        for byte in 0..size {
            out[value * size + byte] = data[byte * count + value];
        }
    }
    out
}

/// An open HDF5 file: the sizes of its addresses and lengths, and its root group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hdf5 {
    offsets: usize,
    lengths: usize,
    base: u64,
    root: u64,
}

/// The type of the values of a dataset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Integer {
        size: usize,
        signed: bool,
        big: bool,
    },
    Float {
        size: usize,
        big: bool,
    },
    Text {
        size: usize,
    },
}

/// Where the values of a dataset are.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Layout {
    Compact(Vec<u8>),
    Contiguous {
        address: u64,
    },
    /// The number of values of a chunk, and the chunks in order.
    Chunked {
        chunk: u64,
        chunks: Vec<Chunk>,
    },
}

/// A chunk of a dataset: its first value, its size in the file, the filters that it skips and
/// its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Chunk {
    first: u64,
    stored: u64,
    mask: u32,
    address: u64,
}

/// A filter of the chunks of a dataset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    Deflate,
    Shuffle,
    Fletcher,
}

/// A one-dimensional dataset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dataset {
    /// The number of values.
    pub len: u64,
    kind: Kind,
    layout: Layout,
    filters: Vec<Filter>,
}

impl Hdf5 {
    /// Reads the superblock: of version 0 or 1 with the symbol table entry of the root group, or
    /// of version 2 or 3 with the address of its object header.
    pub fn open(reader: &mut (impl Read + Seek)) -> Result<Self, Error> {
        for start in STARTS {
            reader.seek(SeekFrom::Start(start))?;
            let mut head = vec![0; 24];
            if reader.read_exact(&mut head).is_err() {
                break;
            }
            if !head.starts_with(SIGNATURE) {
                continue;
            }
            let version = head[8];
            let (offsets, lengths) = match version {
                0 | 1 => (usize::from(head[13]), usize::from(head[14])),
                2 | 3 => (usize::from(head[9]), usize::from(head[10])),
                _ => return Err(unknown()),
            };
            if !matches!(offsets, 2 | 4 | 8) || !matches!(lengths, 2 | 4 | 8) {
                return Err(invalid());
            }
            let rest = read_at(reader, start, 24 + 4 * 8 + 6 * 8 + 8)?;
            let mut fields = Fields::new(&rest);
            let (base, root) = if version <= 1 {
                fields.skip(24 + if version == 1 { 4 } else { 0 })?;
                let base = fields.number(offsets)?;
                fields.skip(3 * offsets + offsets)?;
                (base, fields.number(offsets)?)
            } else {
                fields.skip(12)?;
                let base = fields.number(offsets)?;
                fields.skip(2 * offsets)?;
                (base, fields.number(offsets)?)
            };
            return Ok(Self {
                offsets,
                lengths,
                base,
                root,
            });
        }
        Err(Error::Invalid(Reason::NotHdf5))
    }

    fn address(&self, address: u64) -> u64 {
        self.base.saturating_add(address)
    }

    /// The messages of an object header of version 1 or 2, with the messages of its
    /// continuation blocks.
    fn messages(
        &self,
        reader: &mut (impl Read + Seek),
        address: u64,
    ) -> Result<Vec<(u16, Vec<u8>)>, Error> {
        let address = self.address(address);
        let head = read_at(reader, address, 16)?;
        let mut found = Vec::new();
        let mut blocks = Vec::new();
        if head.starts_with(b"OHDR") {
            let flags = head[5];
            let mut skip = 6;
            if flags & 0x20 != 0 {
                skip += 16;
            }
            if flags & 0x10 != 0 {
                skip += 4;
            }
            let width = 1_usize << (flags & 3);
            let start = read_at(reader, address + skip as u64, width as u64)?;
            let size = Fields::new(&start).number(width)?;
            let body = read_at(reader, address + (skip + width) as u64, size)?;
            self.parse_v2(&body, flags, &mut found, &mut blocks)?;
            for _ in 0..STEPS {
                let Some((at, length)) = blocks.pop() else {
                    break;
                };
                let block = read_at(reader, self.address(at), length)?;
                if !block.starts_with(b"OCHK") || block.len() < 8 {
                    return Err(invalid());
                }
                self.parse_v2(&block[4..block.len() - 4], flags, &mut found, &mut blocks)?;
            }
            return Ok(found);
        }
        if head[0] != 1 {
            return Err(unknown());
        }
        let size = u64::from(Fields::new(&head[8..12]).u32()?);
        let body = read_at(reader, address + 16, size)?;
        self.parse_v1(&body, &mut found, &mut blocks)?;
        for _ in 0..STEPS {
            let Some((at, length)) = blocks.pop() else {
                break;
            };
            let block = read_at(reader, self.address(at), length)?;
            self.parse_v1(&block, &mut found, &mut blocks)?;
        }
        Ok(found)
    }

    fn continuation(&self, data: &[u8]) -> Result<(u64, u64), Error> {
        let mut fields = Fields::new(data);
        Ok((fields.number(self.offsets)?, fields.number(self.lengths)?))
    }

    fn parse_v1(
        &self,
        body: &[u8],
        found: &mut Vec<(u16, Vec<u8>)>,
        blocks: &mut Vec<(u64, u64)>,
    ) -> Result<(), Error> {
        let mut fields = Fields::new(body);
        while fields.left() >= 8 {
            let kind = fields.u16()?;
            let size = usize::from(fields.u16()?);
            fields.skip(4)?;
            let data = fields.take(size)?;
            if kind == CONTINUATION {
                blocks.push(self.continuation(data)?);
            } else {
                found.push((kind, data.to_vec()));
            }
        }
        Ok(())
    }

    fn parse_v2(
        &self,
        body: &[u8],
        flags: u8,
        found: &mut Vec<(u16, Vec<u8>)>,
        blocks: &mut Vec<(u64, u64)>,
    ) -> Result<(), Error> {
        let order = if flags & 0x04 != 0 { 2 } else { 0 };
        let mut fields = Fields::new(body);
        while fields.left() >= 4 + order {
            let kind = u16::from(fields.u8()?);
            let size = usize::from(fields.u16()?);
            fields.skip(1 + order)?;
            let data = fields.take(size)?;
            if kind == CONTINUATION {
                blocks.push(self.continuation(data)?);
            } else {
                found.push((kind, data.to_vec()));
            }
        }
        Ok(())
    }

    /// The members of the group at an object header: their names and the addresses of their
    /// object headers.
    fn group(
        &self,
        reader: &mut (impl Read + Seek),
        address: u64,
    ) -> Result<Vec<(String, u64)>, Error> {
        let messages = self.messages(reader, address)?;
        if let Some((_, data)) = messages.iter().find(|(kind, _)| *kind == SYMBOLS) {
            let mut fields = Fields::new(data);
            let (tree, heap) = (fields.number(self.offsets)?, fields.number(self.offsets)?);
            return self.symbols(reader, tree, heap);
        }
        messages
            .iter()
            .filter(|(kind, _)| *kind == LINK)
            .map(|(_, data)| self.link(data))
            .filter_map(Result::transpose)
            .collect()
    }

    /// A hard link of a group in the new style: its name and the address of its object header.
    fn link(&self, data: &[u8]) -> Result<Option<(String, u64)>, Error> {
        let mut fields = Fields::new(data);
        let _version = fields.u8()?;
        let flags = fields.u8()?;
        let kind = if flags & 0x08 != 0 { fields.u8()? } else { 0 };
        if flags & 0x04 != 0 {
            fields.skip(8)?;
        }
        if flags & 0x10 != 0 {
            fields.skip(1)?;
        }
        let width = 1_usize << (flags & 3);
        let length = usize::try_from(fields.number(width)?).map_err(|_| invalid())?;
        let name = String::from_utf8_lossy(fields.take(length)?).into_owned();
        if kind != 0 {
            return Ok(None);
        }
        Ok(Some((name, fields.number(self.offsets)?)))
    }

    /// The members of an old-style group from its B-tree of symbol table nodes and its local heap
    /// of names.
    fn symbols(
        &self,
        reader: &mut (impl Read + Seek),
        tree: u64,
        heap: u64,
    ) -> Result<Vec<(String, u64)>, Error> {
        let prefix = read_at(
            reader,
            self.address(heap),
            8 + 2 * self.lengths as u64 + self.offsets as u64,
        )?;
        if !prefix.starts_with(b"HEAP") {
            return Err(invalid());
        }
        let mut fields = Fields::new(&prefix[8..]);
        let size = fields.number(self.lengths)?;
        fields.skip(self.lengths)?;
        let names = read_at(reader, self.address(fields.number(self.offsets)?), size)?;
        let name = |offset: u64| -> Result<String, Error> {
            let start = usize::try_from(offset).map_err(|_| invalid())?;
            let rest = names.get(start..).ok_or_else(invalid)?;
            let end = rest
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(rest.len());
            Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
        };
        let mut members = Vec::new();
        for node in self.leaves(reader, tree, 0)? {
            let marker = read_at(reader, self.address(node.1), 8)?;
            if !marker.starts_with(b"SNOD") {
                return Err(invalid());
            }
            let count = u64::from(u16::from_le_bytes([marker[6], marker[7]]));
            let entry = 2 * self.offsets as u64 + 24;
            let entries = read_at(reader, self.address(node.1) + 8, count * entry)?;
            let mut fields = Fields::new(&entries);
            for _ in 0..count {
                let offset = fields.number(self.offsets)?;
                let address = fields.number(self.offsets)?;
                fields.skip(24)?;
                members.push((name(offset)?, address));
            }
        }
        Ok(members)
    }

    /// The children of the leaves of a version 1 B-tree, in order, with the key before each of
    /// them: for a group, the address of each symbol table node; for chunks, the size, the filter
    /// mask and the first value of each chunk.
    fn leaves(
        &self,
        reader: &mut (impl Read + Seek),
        tree: u64,
        dimensions: usize,
    ) -> Result<Vec<(Vec<u8>, u64)>, Error> {
        let key = if dimensions == 0 {
            self.lengths
        } else {
            8 + 8 * dimensions
        };
        let mut found = Vec::new();
        let mut waiting = vec![tree];
        let mut steps = 0;
        while let Some(node) = waiting.pop() {
            steps += 1;
            if steps > STEPS {
                return Err(invalid());
            }
            let head = read_at(reader, self.address(node), 8 + 2 * self.offsets as u64)?;
            if !head.starts_with(b"TREE") {
                return Err(invalid());
            }
            let level = head[5];
            let count = u64::from(u16::from_le_bytes([head[6], head[7]]));
            let body = read_at(
                reader,
                self.address(node) + head.len() as u64,
                count * (key + self.offsets) as u64 + key as u64,
            )?;
            let mut fields = Fields::new(&body);
            let mut children = Vec::new();
            for _ in 0..count {
                let key = fields.take(key)?.to_vec();
                children.push((key, fields.number(self.offsets)?));
            }
            if level == 0 {
                found.extend(children);
            } else {
                waiting.extend(children.into_iter().rev().map(|(_, child)| child));
            }
        }
        Ok(found)
    }

    /// The object header of a path from the root.
    fn find(&self, reader: &mut (impl Read + Seek), path: &str) -> Result<u64, Error> {
        let mut address = self.root;
        for part in path.split('/').filter(|part| !part.is_empty()) {
            address = self
                .group(reader, address)?
                .into_iter()
                .find(|(name, _)| name == part)
                .map(|(_, address)| address)
                .ok_or_else(invalid)?;
        }
        Ok(address)
    }

    /// The names of the members of a group, by its path from the root, such as "resolutions".
    pub fn members(
        &self,
        reader: &mut (impl Read + Seek),
        path: &str,
    ) -> Result<Vec<String>, Error> {
        let address = self.find(reader, path)?;
        Ok(self
            .group(reader, address)?
            .into_iter()
            .map(|(name, _)| name)
            .collect())
    }

    /// A dataset by its path from the root, such as "chroms/name".
    pub fn dataset(&self, reader: &mut (impl Read + Seek), path: &str) -> Result<Dataset, Error> {
        let address = self.find(reader, path)?;
        let messages = self.messages(reader, address)?;
        let message = |wanted: u16| {
            messages
                .iter()
                .find(|(kind, _)| *kind == wanted)
                .map(|(_, data)| data.as_slice())
        };
        let space = message(DATASPACE).ok_or_else(invalid)?;
        let len = self.length(space)?;
        let kind = Kind::parse(message(DATATYPE).ok_or_else(invalid)?)?;
        let layout = self.layout(reader, message(LAYOUT).ok_or_else(invalid)?)?;
        let filters = message(FILTERS).map_or(Ok(Vec::new()), filters)?;
        Ok(Dataset {
            len,
            kind,
            layout,
            filters,
        })
    }

    /// The number of values of a one-dimensional dataspace.
    fn length(&self, data: &[u8]) -> Result<u64, Error> {
        let mut fields = Fields::new(data);
        let version = fields.u8()?;
        let rank = fields.u8()?;
        let _flags = fields.u8()?;
        match version {
            1 => fields.skip(5)?,
            2 => fields.skip(1)?,
            _ => return Err(unknown()),
        }
        match rank {
            0 => Ok(1),
            1 => fields.number(self.lengths),
            _ => Err(unknown()),
        }
    }

    fn layout(&self, reader: &mut (impl Read + Seek), data: &[u8]) -> Result<Layout, Error> {
        let mut fields = Fields::new(data);
        let version = fields.u8()?;
        let class = fields.u8()?;
        match (version, class) {
            (3 | 4, 0) => {
                let size = usize::from(fields.u16()?);
                Ok(Layout::Compact(fields.take(size)?.to_vec()))
            }
            (3 | 4, 1) => Ok(Layout::Contiguous {
                address: fields.number(self.offsets)?,
            }),
            (3, 2) => {
                let dimensions = fields.u8()?;
                if dimensions != 2 {
                    return Err(unknown());
                }
                let tree = fields.number(self.offsets)?;
                let chunk = u64::from(fields.u32()?);
                let chunks = self
                    .leaves(reader, tree, 2)?
                    .into_iter()
                    .map(|(key, address)| {
                        let mut fields = Fields::new(&key);
                        Ok(Chunk {
                            stored: u64::from(fields.u32()?),
                            mask: fields.u32()?,
                            first: fields.number(8)?,
                            address,
                        })
                    })
                    .collect::<Result<Vec<Chunk>, Error>>()?;
                Ok(Layout::Chunked { chunk, chunks })
            }
            _ => Err(unknown()),
        }
    }
}

/// The filters of a pipeline message of version 1 or 2.
fn filters(data: &[u8]) -> Result<Vec<Filter>, Error> {
    let mut fields = Fields::new(data);
    let version = fields.u8()?;
    let count = fields.u8()?;
    if version == 1 {
        fields.skip(6)?;
    }
    let mut found = Vec::new();
    for _ in 0..count {
        let id = fields.u16()?;
        let named = version == 1 || id >= 256;
        let length = if named { usize::from(fields.u16()?) } else { 0 };
        let _flags = fields.u16()?;
        let values = usize::from(fields.u16()?);
        if version == 1 {
            fields.skip(length.next_multiple_of(8))?;
        } else {
            fields.skip(length)?;
        }
        fields.skip(4 * values)?;
        if version == 1 && values % 2 == 1 {
            fields.skip(4)?;
        }
        found.push(match id {
            1 => Filter::Deflate,
            2 => Filter::Shuffle,
            3 => Filter::Fletcher,
            _ => return Err(unknown()),
        });
    }
    Ok(found)
}

impl Dataset {
    /// The bytes of the values in a range.
    fn bytes(
        &self,
        file: &Hdf5,
        reader: &mut (impl Read + Seek),
        range: &Range<u64>,
    ) -> Result<Vec<u8>, Error> {
        if range.start > range.end || range.end > self.len {
            return Err(invalid());
        }
        let size = self.kind.size() as u64;
        let (start, end) = (range.start * size, range.end * size);
        match &self.layout {
            Layout::Compact(data) => data
                .get(
                    usize::try_from(start).map_err(|_| invalid())?
                        ..usize::try_from(end).map_err(|_| invalid())?,
                )
                .map(<[u8]>::to_vec)
                .ok_or_else(invalid),
            Layout::Contiguous { address } => {
                read_at(reader, file.address(*address) + start, end - start)
            }
            Layout::Chunked { chunk, chunks } => {
                let mut out = Vec::with_capacity(usize::try_from(end - start).unwrap_or(0));
                let wanted = chunks.iter().filter(|part| {
                    part.first < range.end && part.first.saturating_add(*chunk) > range.start
                });
                for part in wanted {
                    let (first, last) = (part.first, part.first.saturating_add(*chunk));
                    let raw = read_at(reader, file.address(part.address), part.stored)?;
                    let values = unfilter(raw, &self.filters, part.mask, size as usize)?;
                    let from = (range.start.max(first) - first) * size;
                    let to = (range.end.min(last) - first) * size;
                    let part = values
                        .get(
                            usize::try_from(from).map_err(|_| invalid())?
                                ..usize::try_from(to).map_err(|_| invalid())?,
                        )
                        .ok_or_else(invalid)?;
                    out.extend_from_slice(part);
                }
                if out.len() as u64 != end - start {
                    return Err(invalid());
                }
                Ok(out)
            }
        }
    }

    /// The values in a range as whole numbers. Floats lose their fraction.
    pub fn integers(
        &self,
        file: &Hdf5,
        reader: &mut (impl Read + Seek),
        range: Range<u64>,
    ) -> Result<Vec<i64>, Error> {
        if matches!(self.kind, Kind::Text { .. }) {
            return Err(invalid());
        }
        let bytes = self.bytes(file, reader, &range)?;
        Ok(bytes
            .chunks_exact(self.kind.size())
            .map(|value| decode(self.kind, value).1)
            .collect())
    }

    /// The values in a range as floats.
    pub fn floats(
        &self,
        file: &Hdf5,
        reader: &mut (impl Read + Seek),
        range: Range<u64>,
    ) -> Result<Vec<f64>, Error> {
        if matches!(self.kind, Kind::Text { .. }) {
            return Err(invalid());
        }
        let bytes = self.bytes(file, reader, &range)?;
        Ok(bytes
            .chunks_exact(self.kind.size())
            .map(|value| decode(self.kind, value).0)
            .collect())
    }

    /// The texts in a range, without the padding at their end.
    pub fn texts(
        &self,
        file: &Hdf5,
        reader: &mut (impl Read + Seek),
        range: Range<u64>,
    ) -> Result<Vec<String>, Error> {
        if !matches!(self.kind, Kind::Text { .. }) {
            return Err(invalid());
        }
        let bytes = self.bytes(file, reader, &range)?;
        Ok(bytes
            .chunks_exact(self.kind.size())
            .map(|value| {
                let end = value
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(value.len());
                String::from_utf8_lossy(&value[..end]).trim_end().to_owned()
            })
            .collect())
    }
}

#[cfg(test)]
pub mod tests {
    use std::io::Cursor;

    use super::Hdf5;
    use crate::{Error, Reason};

    const SIGNATURE: &[u8; 8] = b"\x89HDF\r\n\x1a\n";
    const UNDEFINED: u64 = u64::MAX;

    /// A writer of small HDF5 files as the specification describes them: a version 0
    /// superblock, version 1 object headers, groups with symbol tables, and datasets stored whole
    /// or in chunks with a version 1 B-tree.
    pub struct Builder {
        bytes: Vec<u8>,
    }

    /// The type of the values of a dataset for the builder.
    #[derive(Debug, Clone, Copy)]
    pub enum Values<'a> {
        I32(&'a [i32]),
        I64(&'a [i64]),
        F64(&'a [f64]),
        Text(&'a [&'a str], usize),
    }

    impl Values<'_> {
        fn len(&self) -> usize {
            match self {
                Self::I32(values) => values.len(),
                Self::I64(values) => values.len(),
                Self::F64(values) => values.len(),
                Self::Text(values, _) => values.len(),
            }
        }

        fn size(&self) -> usize {
            match self {
                Self::I32(_) => 4,
                Self::I64(_) | Self::F64(_) => 8,
                Self::Text(_, size) => *size,
            }
        }

        fn bytes(&self) -> Vec<u8> {
            match self {
                Self::I32(values) => values
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect(),
                Self::I64(values) => values
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect(),
                Self::F64(values) => values
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect(),
                Self::Text(values, size) => values
                    .iter()
                    .flat_map(|value| {
                        let mut bytes = value.as_bytes().to_vec();
                        bytes.resize(*size, 0);
                        bytes
                    })
                    .collect(),
            }
        }

        /// The datatype message.
        fn datatype(&self) -> Vec<u8> {
            let (class, bits, size, properties): (u8, [u8; 3], usize, Vec<u8>) = match self {
                Self::I32(_) => (0, [0x08, 0, 0], 4, [0, 0, 32, 0].to_vec()),
                Self::I64(_) => (0, [0x08, 0, 0], 8, [0, 0, 64, 0].to_vec()),
                Self::F64(_) => (
                    1,
                    [0x20, 63, 0],
                    8,
                    [0, 0, 64, 0, 52, 11, 0, 52, 0xFF, 0x03, 0, 0].to_vec(),
                ),
                Self::Text(_, size) => (3, [0, 0, 0], *size, Vec::new()),
            };
            let mut message = vec![0x10 | class, bits[0], bits[1], bits[2]];
            message.extend((size as u32).to_le_bytes());
            message.extend(properties);
            message
        }
    }

    fn pad(mut data: Vec<u8>) -> Vec<u8> {
        data.resize(data.len().next_multiple_of(8), 0);
        data
    }

    impl Default for Builder {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Builder {
        pub fn new() -> Self {
            Self { bytes: vec![0; 96] }
        }

        fn at(&self) -> u64 {
            self.bytes.len() as u64
        }

        fn put(&mut self, data: &[u8]) -> u64 {
            let at = self.at();
            self.bytes.extend_from_slice(data);
            at
        }

        /// An object header of version 1 with its messages.
        fn object(&mut self, messages: &[(u16, Vec<u8>)]) -> u64 {
            let mut body = Vec::new();
            for (kind, data) in messages {
                let data = pad(data.clone());
                body.extend(kind.to_le_bytes());
                body.extend((data.len() as u16).to_le_bytes());
                body.extend([0, 0, 0, 0]);
                body.extend(data);
            }
            let mut header = vec![1, 0];
            header.extend((messages.len() as u16).to_le_bytes());
            header.extend(1_u32.to_le_bytes());
            header.extend((body.len() as u32).to_le_bytes());
            header.extend([0; 4]);
            header.extend(body);
            self.put(&header)
        }

        fn dataspace(count: usize) -> Vec<u8> {
            let mut message = vec![1, 1, 0, 0, 0, 0, 0, 0];
            message.extend((count as u64).to_le_bytes());
            message
        }

        /// A dataset stored whole.
        pub fn contiguous(&mut self, values: Values<'_>) -> u64 {
            let data = values.bytes();
            let address = self.put(&data);
            let mut layout = vec![3, 1];
            layout.extend(address.to_le_bytes());
            layout.extend((data.len() as u64).to_le_bytes());
            self.object(&[
                (1, Self::dataspace(values.len())),
                (3, values.datatype()),
                (8, layout),
            ])
        }

        /// A dataset in chunks of `chunk` values, with the deflate and shuffle filters when
        /// `compress` is true.
        pub fn chunked(&mut self, values: Values<'_>, chunk: usize, compress: bool) -> u64 {
            let size = values.size();
            let data = values.bytes();
            let mut entries = Vec::new();
            for (index, part) in data.chunks(chunk * size).enumerate() {
                let mut part = part.to_vec();
                part.resize(chunk * size, 0);
                let stored = if compress {
                    let shuffled = shuffle(&part, size);
                    miniz_oxide::deflate::compress_to_vec_zlib(&shuffled, 6)
                } else {
                    part
                };
                let address = self.put(&stored);
                entries.push(((index * chunk) as u64, stored.len() as u32, address));
            }
            let mut tree = b"TREE".to_vec();
            tree.extend([1, 0]);
            tree.extend((entries.len() as u16).to_le_bytes());
            tree.extend(UNDEFINED.to_le_bytes());
            tree.extend(UNDEFINED.to_le_bytes());
            let key = |tree: &mut Vec<u8>, stored: u32, offset: u64| {
                tree.extend(stored.to_le_bytes());
                tree.extend(0_u32.to_le_bytes());
                tree.extend(offset.to_le_bytes());
                tree.extend(0_u64.to_le_bytes());
            };
            for (offset, stored, address) in &entries {
                key(&mut tree, *stored, *offset);
                tree.extend(address.to_le_bytes());
            }
            key(&mut tree, 0, values.len() as u64);
            let tree = self.put(&tree);
            let mut layout = vec![3, 2, 2];
            layout.extend(tree.to_le_bytes());
            layout.extend((chunk as u32).to_le_bytes());
            layout.extend((size as u32).to_le_bytes());
            let mut messages = vec![
                (1, Self::dataspace(values.len())),
                (3, values.datatype()),
                (8, layout),
            ];
            if compress {
                let mut filters = vec![1, 2, 0, 0, 0, 0, 0, 0];
                for (id, name, value) in [(2_u16, "shuffle", size as u32), (1, "deflate", 6)] {
                    let name = pad(format!("{name}\0").into_bytes());
                    filters.extend(id.to_le_bytes());
                    filters.extend((name.len() as u16).to_le_bytes());
                    filters.extend(0_u16.to_le_bytes());
                    filters.extend(1_u16.to_le_bytes());
                    filters.extend(name);
                    filters.extend(value.to_le_bytes());
                    filters.extend([0; 4]);
                }
                messages.push((11, filters));
            }
            self.object(&messages)
        }

        /// A group with a symbol table: a local heap of names, a node and a B-tree.
        pub fn group(&mut self, members: &[(&str, u64)]) -> u64 {
            let mut names = vec![0_u8; 8];
            let mut offsets = Vec::new();
            for (name, _) in members {
                offsets.push(names.len() as u64);
                names.extend(pad(format!("{name}\0").into_bytes()));
            }
            let data = self.put(&names);
            let mut heap = b"HEAP".to_vec();
            heap.extend([0, 0, 0, 0]);
            heap.extend((names.len() as u64).to_le_bytes());
            heap.extend(UNDEFINED.to_le_bytes());
            heap.extend(data.to_le_bytes());
            let heap = self.put(&heap);
            let mut node = b"SNOD".to_vec();
            node.extend([1, 0]);
            node.extend((members.len() as u16).to_le_bytes());
            for ((_, address), offset) in members.iter().zip(&offsets) {
                node.extend(offset.to_le_bytes());
                node.extend(address.to_le_bytes());
                node.extend([0; 24]);
            }
            let node = self.put(&node);
            let mut tree = b"TREE".to_vec();
            tree.extend([0, 0, 1, 0]);
            tree.extend(UNDEFINED.to_le_bytes());
            tree.extend(UNDEFINED.to_le_bytes());
            tree.extend(0_u64.to_le_bytes());
            tree.extend(node.to_le_bytes());
            tree.extend(offsets.last().copied().unwrap_or(0).to_le_bytes());
            let tree = self.put(&tree);
            let mut table = tree.to_le_bytes().to_vec();
            table.extend(heap.to_le_bytes());
            self.object(&[(17, table)])
        }

        /// The file with its superblock, and `root` as its root group.
        pub fn finish(mut self, root: u64) -> Vec<u8> {
            let end = self.at();
            let mut block = SIGNATURE.to_vec();
            block.extend([0, 0, 0, 0, 0, 8, 8, 0]);
            block.extend(4_u16.to_le_bytes());
            block.extend(16_u16.to_le_bytes());
            block.extend(0_u32.to_le_bytes());
            block.extend(0_u64.to_le_bytes());
            block.extend(UNDEFINED.to_le_bytes());
            block.extend(end.to_le_bytes());
            block.extend(UNDEFINED.to_le_bytes());
            block.extend(0_u64.to_le_bytes());
            block.extend(root.to_le_bytes());
            block.extend([0; 24]);
            self.bytes[..96].copy_from_slice(&block);
            self.bytes
        }
    }

    /// The bytes of values as the shuffle filter stores them: the first byte of each value,
    /// then the second, and so on.
    pub fn shuffle(data: &[u8], size: usize) -> Vec<u8> {
        let count = data.len() / size;
        let mut out = vec![0; data.len()];
        for value in 0..count {
            for byte in 0..size {
                out[byte * count + value] = data[value * size + byte];
            }
        }
        out
    }

    fn sample() -> Vec<u8> {
        let mut file = Builder::new();
        let names = file.contiguous(Values::Text(&["chr1", "chrX"], 5));
        let lengths = file.contiguous(Values::I64(&[1_000, 500]));
        let chroms = file.group(&[("name", names), ("length", lengths)]);
        let counts: Vec<i32> = (0..25).map(|value| value * 3).collect();
        let packed = file.chunked(Values::I32(&counts), 10, true);
        let plain = file.chunked(Values::I32(&counts), 10, false);
        let weights = file.contiguous(Values::F64(&[0.5, 1.5, f64::NAN]));
        let data = file.group(&[("packed", packed), ("plain", plain), ("weight", weights)]);
        let root = file.group(&[("chroms", chroms), ("data", data)]);
        file.finish(root)
    }

    #[test]
    fn groups_name_their_members() {
        let bytes = sample();
        let mut reader = Cursor::new(&bytes);
        let file = Hdf5::open(&mut reader).unwrap();
        assert_eq!(file.members(&mut reader, "").unwrap(), ["chroms", "data"]);
        assert_eq!(
            file.members(&mut reader, "chroms").unwrap(),
            ["name", "length"]
        );
        assert!(file.members(&mut reader, "nothing").is_err());
    }

    #[test]
    fn datasets_give_texts_and_numbers() {
        let bytes = sample();
        let mut reader = Cursor::new(&bytes);
        let file = Hdf5::open(&mut reader).unwrap();
        let names = file.dataset(&mut reader, "chroms/name").unwrap();
        assert_eq!(names.len, 2);
        assert_eq!(
            names.texts(&file, &mut reader, 0..2).unwrap(),
            ["chr1", "chrX"]
        );
        let lengths = file.dataset(&mut reader, "chroms/length").unwrap();
        assert_eq!(
            lengths.integers(&file, &mut reader, 0..2).unwrap(),
            [1_000, 500]
        );
        let weights = file.dataset(&mut reader, "data/weight").unwrap();
        let found = weights.floats(&file, &mut reader, 1..3).unwrap();
        assert!((found[0] - 1.5).abs() < f64::EPSILON && found[1].is_nan());
    }

    #[test]
    fn chunks_with_and_without_filters_give_any_range() {
        let bytes = sample();
        let mut reader = Cursor::new(&bytes);
        let file = Hdf5::open(&mut reader).unwrap();
        for path in ["data/packed", "data/plain"] {
            let counts = file.dataset(&mut reader, path).unwrap();
            assert_eq!(counts.len, 25, "{path}");
            assert_eq!(
                counts.integers(&file, &mut reader, 8..13).unwrap(),
                [24, 27, 30, 33, 36],
                "{path}"
            );
            assert_eq!(
                counts.integers(&file, &mut reader, 20..25).unwrap(),
                [60, 63, 66, 69, 72],
                "{path}"
            );
            assert!(
                counts.integers(&file, &mut reader, 20..26).is_err(),
                "{path}"
            );
        }
    }

    #[test]
    fn other_files_say_what_they_are_not() {
        let mut reader = Cursor::new(b"not an HDF5 file at all, only text".to_vec());
        assert_eq!(
            Hdf5::open(&mut reader).unwrap_err().to_string(),
            Error::Invalid(Reason::NotHdf5).to_string()
        );
        let mut bytes = sample();
        bytes.truncate(400);
        let mut reader = Cursor::new(&bytes);
        let file = Hdf5::open(&mut reader);
        assert!(file.is_err() || file.unwrap().members(&mut reader, "").is_err());
    }
}
