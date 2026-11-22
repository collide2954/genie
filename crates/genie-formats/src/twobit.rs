use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, OnceLock};

use genie_core::Region;
use vstd::prelude::*;

use crate::{Error, Reason};

const SIGNATURE: u32 = 0x1A41_2743;
const CODES: &[u8; 4] = b"TCAG";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    Little,
    Big,
}

impl Order {
    fn word(self, bytes: [u8; 4]) -> u32 {
        match self {
            Self::Little => u32::from_le_bytes(bytes),
            Self::Big => u32::from_be_bytes(bytes),
        }
    }

    fn long(self, bytes: [u8; 8]) -> u64 {
        match self {
            Self::Little => u64::from_le_bytes(bytes),
            Self::Big => u64::from_be_bytes(bytes),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub length: u64,
    offset: u64,
}

#[derive(Debug)]
struct Layout {
    unknown: Vec<(u64, u64)>,
    masks: Masks,
    bases: u64,
}

/// Where the mask blocks of a sequence are in the file: their number, and the place of their
/// starts. Their sizes follow the starts. A chromosome can have hundreds of thousands, so a
/// fetch reads only the blocks near its region.
#[derive(Debug, Clone, Copy)]
struct Masks {
    count: u64,
    starts: u64,
}

/// The fewest and the most mask blocks that one read of a fetch takes.
const WINDOW: (u64, u64) = (64, 4_096);

#[derive(Debug)]
pub struct TwoBit {
    order: Order,
    entries: Vec<Entry>,
    layouts: Vec<OnceLock<Arc<Layout>>>,
}

impl TwoBit {
    pub fn read(reader: &mut (impl Read + Seek)) -> Result<Self, Error> {
        Self::read_with(reader, &HashMap::new())
    }

    pub fn read_with<R: Read + Seek>(
        reader: &mut R,
        sizes: &HashMap<String, u64>,
    ) -> Result<Self, Error> {
        Self::read_using(reader, sizes, |reader, offsets| {
            offsets
                .iter()
                .map(|&offset| {
                    reader.seek(SeekFrom::Start(offset))?;
                    bytes(reader)
                })
                .collect()
        })
    }

    pub fn read_using<R: Read + Seek>(
        reader: &mut R,
        sizes: &HashMap<String, u64>,
        words: impl FnOnce(&mut R, &[u64]) -> Result<Vec<[u8; 4]>, Error>,
    ) -> Result<Self, Error> {
        reader.seek(SeekFrom::Start(0))?;
        let signature = bytes(reader)?;
        let order = if u32::from_le_bytes(signature) == SIGNATURE {
            Order::Little
        } else if u32::from_be_bytes(signature) == SIGNATURE {
            Order::Big
        } else {
            return Err(Error::Invalid(Reason::NotTwoBit));
        };
        let version = order.word(bytes(reader)?);
        if version > 1 {
            return Err(Error::Invalid(Reason::UnsupportedVersion));
        }
        let count = order.word(bytes(reader)?);
        let _reserved: [u8; 4] = bytes(reader)?;
        let mut entries = Vec::new();
        for _ in 0..count {
            let [size] = bytes(reader)?;
            let mut name = vec![0; usize::from(size)];
            reader.read_exact(&mut name)?;
            let offset = if version == 1 {
                order.long(bytes(reader)?)
            } else {
                u64::from(order.word(bytes(reader)?))
            };
            entries.push(Entry {
                name: String::from_utf8_lossy(&name).into_owned(),
                length: 0,
                offset,
            });
        }
        let mut unknown = Vec::new();
        for (index, entry) in entries.iter_mut().enumerate() {
            match sizes.get(&entry.name) {
                Some(length) => entry.length = *length,
                None => unknown.push(index),
            }
        }
        let offsets: Vec<u64> = unknown.iter().map(|&index| entries[index].offset).collect();
        let found = words(reader, &offsets)?;
        if found.len() != unknown.len() {
            return Err(Error::Io(std::io::ErrorKind::UnexpectedEof.into()));
        }
        for (index, word) in unknown.into_iter().zip(found) {
            entries[index].length = u64::from(order.word(word));
        }
        let layouts = entries.iter().map(|_| OnceLock::new()).collect();
        Ok(Self {
            order,
            entries,
            layouts,
        })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn fetch(
        &self,
        reader: &mut (impl Read + Seek),
        index: usize,
        region: Region,
    ) -> Result<Vec<u8>, Error> {
        let region = region.clamp_to(self.entries[index].length);
        if region.is_empty() {
            return Ok(Vec::new());
        }
        let layout = self.layout(reader, index)?;
        let first = region.start() / 4;
        let last = (region.end() - 1) / 4;
        let mut packed = vec![0; (last - first + 1) as usize];
        reader.seek(SeekFrom::Start(layout.bases + first))?;
        reader.read_exact(&mut packed)?;
        let mut bases: Vec<u8> = (region.start()..region.end())
            .map(|position| {
                let byte = packed[(position / 4 - first) as usize];
                let code = (byte >> (6 - 2 * (position % 4))) & 0b11;
                CODES[usize::from(code)]
            })
            .collect();
        apply(&layout.unknown, region, &mut bases, |base| *base = b'N');
        let length = self.entries[index].length;
        let masked = self.masked(reader, layout.masks, region, length)?;
        apply(&masked, region, &mut bases, u8::make_ascii_lowercase);
        Ok(bases)
    }

    fn layout(&self, reader: &mut (impl Read + Seek), index: usize) -> Result<Arc<Layout>, Error> {
        if let Some(layout) = self.layouts[index].get() {
            return Ok(Arc::clone(layout));
        }
        let entry = &self.entries[index];
        reader.seek(SeekFrom::Start(entry.offset + 4))?;
        let unknown = blocks(reader, self.order, entry.length)?;
        let count = u64::from(self.order.word(bytes(reader)?));
        if count > entry.length {
            return Err(Error::Invalid(Reason::BadBlockCount));
        }
        let starts = reader.stream_position()?;
        reader.seek(SeekFrom::Start(starts + 8 * count))?;
        let _reserved: [u8; 4] = bytes(reader)?;
        let layout = Arc::new(Layout {
            unknown,
            masks: Masks { count, starts },
            bases: reader.stream_position()?,
        });
        Ok(Arc::clone(self.layouts[index].get_or_init(|| layout)))
    }

    /// The mask blocks that touch a region. A search in the starts finds the first block, and
    /// reads in windows that grow take the blocks from there to the end of the region.
    fn masked(
        &self,
        reader: &mut (impl Read + Seek),
        masks: Masks,
        region: Region,
        length: u64,
    ) -> Result<Vec<(u64, u64)>, Error> {
        if masks.count == 0 {
            return Ok(Vec::new());
        }
        let before = self.starts_before(reader, masks, region.start(), length)?;
        let mut index = before.saturating_sub(1);
        let mut take = WINDOW.0;
        let mut found = Vec::new();
        while index < masks.count {
            let take_now = take.min(masks.count - index);
            let starts = self.words_at(reader, masks.starts + 4 * index, take_now)?;
            let sizes =
                self.words_at(reader, masks.starts + 4 * (masks.count + index), take_now)?;
            for (start, size) in starts.into_iter().zip(sizes) {
                if start >= region.end() {
                    return Ok(found);
                }
                if start + size > region.start() {
                    found.push((start, start + size));
                }
            }
            index += take_now;
            take = (take * 2).min(WINDOW.1);
        }
        Ok(found)
    }

    /// The number of mask blocks that start before a position. The search starts where the
    /// position would be if the blocks were spread evenly, widens its steps until it holds
    /// the answer, and then halves them.
    fn starts_before(
        &self,
        reader: &mut (impl Read + Seek),
        masks: Masks,
        position: u64,
        length: u64,
    ) -> Result<u64, Error> {
        let count = masks.count;
        let mut start = |index: u64| -> Result<u64, Error> {
            Ok(self.words_at(reader, masks.starts + 4 * index, 1)?[0])
        };
        let share = u128::from(count) * u128::from(position) / u128::from(length.max(1));
        let guess = u64::try_from(share).unwrap_or(count).min(count - 1);
        let (mut low, mut high);
        let mut step = 1;
        if start(guess)? < position {
            low = guess + 1;
            loop {
                let probe = guess.saturating_add(step);
                if probe >= count {
                    high = count;
                    break;
                }
                if start(probe)? >= position {
                    high = probe;
                    break;
                }
                low = probe + 1;
                step *= 2;
            }
        } else {
            high = guess;
            loop {
                let Some(probe) = guess.checked_sub(step) else {
                    low = 0;
                    break;
                };
                if start(probe)? < position {
                    low = probe + 1;
                    break;
                }
                high = probe;
                step *= 2;
            }
        }
        while low < high {
            let middle = low + (high - low) / 2;
            if start(middle)? < position {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        Ok(low)
    }

    fn words_at(
        &self,
        reader: &mut (impl Read + Seek),
        offset: u64,
        count: u64,
    ) -> Result<Vec<u64>, Error> {
        reader.seek(SeekFrom::Start(offset))?;
        let size = usize::try_from(count * 4).map_err(|_| Error::Invalid(Reason::BadBlockCount))?;
        let mut raw = vec![0; size];
        reader.read_exact(&mut raw)?;
        Ok(words(&raw, self.order).collect())
    }
}

pub fn sniff(head: &[u8]) -> bool {
    head.first_chunk::<4>().is_some_and(|signature| {
        u32::from_le_bytes(*signature) == SIGNATURE || u32::from_be_bytes(*signature) == SIGNATURE
    })
}

fn bytes<const N: usize>(reader: &mut impl Read) -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn blocks(reader: &mut impl Read, order: Order, length: u64) -> Result<Vec<(u64, u64)>, Error> {
    let count = order.word(bytes(reader)?);
    if u64::from(count) > length {
        return Err(Error::Invalid(Reason::BadBlockCount));
    }
    let mut raw = vec![0; count as usize * 8];
    reader.read_exact(&mut raw)?;
    let (starts, sizes) = raw.split_at(count as usize * 4);
    Ok(words(starts, order)
        .zip(words(sizes, order))
        .map(|(start, size)| (start, start + size))
        .collect())
}

fn words(bytes: &[u8], order: Order) -> impl Iterator<Item = u64> + '_ {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(move |chunk| u64::from(order.word(*chunk)))
}

/// Changes the bases of a region, from its start, that are in blocks.
fn apply(blocks: &[(u64, u64)], region: Region, bases: &mut [u8], change: impl Fn(&mut u8)) {
    for (from, to) in parts(blocks, region) {
        let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) else {
            continue;
        };
        if let Some(part) = bases.get_mut(from..to) {
            part.iter_mut().for_each(&change);
        }
    }
}
verus! {

/// True when a position is in a block from its start to its end, without the end.
pub open spec fn spec_in(block: (u64, u64), position: int) -> bool {
    block.0 <= position < block.1
}

/// True when the parts of a region from `from` to `to`, without `to`, have a place.
pub open spec fn spec_covers(parts: Seq<(u64, u64)>, place: int) -> bool {
    exists|at: int| 0 <= at < parts.len() && #[trigger] spec_in(parts[at], place)
}

/// The parts of a region in blocks, as places from the start of the region: a place of the
/// region is in a part when its position is in a block. The blocks can come in any order.
fn parts(blocks: &[(u64, u64)], region: Region) -> (found: Vec<(u64, u64)>)
    ensures
        forall|at: int| 0 <= at < found@.len() ==> (#[trigger] found@[at]).0 < found@[at].1 <= region.spec_end() - region.spec_start(),
        forall|place: int| 0 <= place < region.spec_end() - region.spec_start() ==> (spec_covers(found@, place)
            <==> exists|block: int| 0 <= block < blocks@.len() && #[trigger] spec_in(blocks@[block], region.spec_start() + place)),
{
    let (start, end) = (region.start(), region.end());
    let mut found: Vec<(u64, u64)> = Vec::new();
    let mut at: usize = 0;
    while at < blocks.len()
        invariant
            at <= blocks.len(),
            start == region.spec_start(),
            end == region.spec_end(),
            start <= end,
            forall|part: int| 0 <= part < found@.len() ==> (#[trigger] found@[part]).0 < found@[part].1 <= end - start,
            forall|place: int| 0 <= place < end - start ==> (spec_covers(found@, place)
                <==> exists|block: int| 0 <= block < at && #[trigger] spec_in(blocks@[block], start + place)),
        decreases blocks.len() - at,
    {
        let (low, high) = blocks[at];
        let ghost old_found = found@;
        if start < end && low < end && start < high && low < high {
            let from = low.saturating_sub(start);
            let to = high.min(end) - start;
            found.push((from, to));
            proof {
                assert forall|part: int| 0 <= part < found@.len() implies (#[trigger] found@[part]).0
                    < found@[part].1 <= end - start by {
                    if part < old_found.len() {
                        assert(found@[part] == old_found[part]);
                    }
                }
                assert forall|place: int| 0 <= place < end - start implies (spec_covers(found@, place)
                    <==> exists|block: int| 0 <= block < at + 1 && #[trigger] spec_in(blocks@[block], start + place)) by {
                    if spec_covers(found@, place) {
                        let part = choose|part: int| 0 <= part < found@.len() && #[trigger] spec_in(found@[part], place);
                        if part < old_found.len() {
                            assert(found@[part] == old_found[part]);
                            assert(spec_covers(old_found, place));
                        } else {
                            assert(spec_in(blocks@[at as int], start + place));
                        }
                    }
                    if exists|block: int| 0 <= block < at + 1 && #[trigger] spec_in(blocks@[block], start + place) {
                        let block = choose|block: int| 0 <= block < at + 1 && #[trigger] spec_in(blocks@[block], start + place);
                        if block < at {
                            assert(spec_covers(old_found, place));
                            let part = choose|part: int| 0 <= part < old_found.len() && #[trigger] spec_in(old_found[part], place);
                            assert(found@[part] == old_found[part]);
                            assert(spec_in(found@[part], place));
                        } else {
                            assert(spec_in(found@[old_found.len() as int], place));
                        }
                    }
                }
            };
        } else {
            proof {
                assert forall|place: int| 0 <= place < end - start implies (spec_covers(found@, place)
                    <==> exists|block: int| 0 <= block < at + 1 && #[trigger] spec_in(blocks@[block], start + place)) by {
                    if exists|block: int| 0 <= block < at + 1 && #[trigger] spec_in(blocks@[block], start + place) {
                        let block = choose|block: int| 0 <= block < at + 1 && #[trigger] spec_in(blocks@[block], start + place);
                        assert(block < at);
                    }
                }
            };
        }
        at += 1;
    }
    found
}

} // verus!

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::Cursor;

    use genie_core::Region;

    use super::{TwoBit, sniff};
    use crate::{Error, Reason};

    struct Format {
        big_endian: bool,
        version: u32,
    }

    const PLAIN: Format = Format {
        big_endian: false,
        version: 0,
    };

    fn runs(bytes: &[u8], wanted: fn(u8) -> bool) -> Vec<(u32, u32)> {
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for (index, byte) in bytes.iter().enumerate() {
            if !wanted(*byte) {
                continue;
            }
            let index = u32::try_from(index).unwrap();
            match runs.last_mut() {
                Some((start, size)) if *start + *size == index => *size += 1,
                _ => runs.push((index, 1)),
            }
        }
        runs
    }

    fn encode(sequences: &[(&str, &str)], format: &Format) -> Vec<u8> {
        let word = |value: u32| {
            if format.big_endian {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            }
        };
        let mut records = Vec::new();
        for (_, bases) in sequences {
            let bases = bases.as_bytes();
            let mut record = word(u32::try_from(bases.len()).unwrap()).to_vec();
            for blocks in [
                runs(bases, |byte| byte.eq_ignore_ascii_case(&b'N')),
                runs(bases, |byte| byte.is_ascii_lowercase()),
            ] {
                record.extend(word(u32::try_from(blocks.len()).unwrap()));
                record.extend(blocks.iter().flat_map(|(start, _)| word(*start)));
                record.extend(blocks.iter().flat_map(|(_, size)| word(*size)));
            }
            record.extend(word(0));
            for chunk in bases.chunks(4) {
                let mut packed = 0_u8;
                for (slot, base) in chunk.iter().enumerate() {
                    let code = match base.to_ascii_uppercase() {
                        b'C' => 1,
                        b'A' => 2,
                        b'G' => 3,
                        _ => 0,
                    };
                    packed |= code << (6 - 2 * slot);
                }
                record.push(packed);
            }
            records.push(record);
        }
        let offset_size = if format.version == 1 { 8 } else { 4 };
        let index_size: usize = sequences
            .iter()
            .map(|(name, _)| 1 + name.len() + offset_size)
            .sum();
        let mut bytes = Vec::new();
        bytes.extend(word(0x1A41_2743));
        bytes.extend(word(format.version));
        bytes.extend(word(u32::try_from(sequences.len()).unwrap()));
        bytes.extend(word(0));
        let mut offset = 16 + index_size;
        for ((name, _), record) in sequences.iter().zip(&records) {
            bytes.push(u8::try_from(name.len()).unwrap());
            bytes.extend(name.bytes());
            if format.version == 1 {
                let value = u64::try_from(offset).unwrap();
                bytes.extend(if format.big_endian {
                    value.to_be_bytes()
                } else {
                    value.to_le_bytes()
                });
            } else {
                bytes.extend(word(u32::try_from(offset).unwrap()));
            }
            offset += record.len();
        }
        bytes.extend(records.concat());
        bytes
    }

    fn fetch(bytes: &[u8], index: usize, start: u64, end: u64) -> String {
        let mut reader = Cursor::new(bytes);
        let file = TwoBit::read(&mut reader).unwrap();
        let region = Region::new(start, end).unwrap();
        String::from_utf8(file.fetch(&mut reader, index, region).unwrap()).unwrap()
    }

    #[test]
    fn reads_names_and_lengths() {
        let bytes = encode(&[("chr1", "ACGTACGTA"), ("chrM", "GG")], &PLAIN);
        let file = TwoBit::read(&mut Cursor::new(&bytes)).unwrap();
        let entries: Vec<(&str, u64)> = file
            .entries()
            .iter()
            .map(|entry| (entry.name.as_str(), entry.length))
            .collect();
        assert_eq!(entries, [("chr1", 9), ("chrM", 2)]);
    }

    struct Seeks<R> {
        inner: R,
        targets: Vec<u64>,
    }

    impl<R: std::io::Read> std::io::Read for Seeks<R> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.inner.read(buffer)
        }
    }

    impl<R: std::io::Seek> std::io::Seek for Seeks<R> {
        fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
            let at = self.inner.seek(to)?;
            self.targets.push(at);
            Ok(at)
        }
    }

    #[test]
    fn known_sizes_skip_the_sequence_records() {
        let bytes = encode(&[("chr1", "ACGTACGTA"), ("chrM", "GG")], &PLAIN);
        let (first, second) = (34, 34 + 4 + 4 + 4 + 4 + 3);
        let sizes = HashMap::from([("chr1".to_owned(), 9)]);
        let mut reader = Seeks {
            inner: Cursor::new(&bytes),
            targets: Vec::new(),
        };
        let file = TwoBit::read_with(&mut reader, &sizes).unwrap();
        let entries: Vec<(&str, u64)> = file
            .entries()
            .iter()
            .map(|entry| (entry.name.as_str(), entry.length))
            .collect();
        assert_eq!(entries, [("chr1", 9), ("chrM", 2)]);
        assert!(!reader.targets.contains(&first), "{:?}", reader.targets);
        assert!(reader.targets.contains(&second), "{:?}", reader.targets);
        let region = Region::new(0, 9).unwrap();
        assert_eq!(file.fetch(&mut reader, 0, region).unwrap(), b"ACGTACGTA");
    }

    #[test]
    fn lengths_can_come_from_other_reads() {
        let bytes = encode(&[("chr1", "ACGTACGTA"), ("chrM", "GG")], &PLAIN);
        let sizes = HashMap::from([("chr1".to_owned(), 9)]);
        let mut asked = Vec::new();
        let file = TwoBit::read_using(&mut Cursor::new(&bytes), &sizes, |_, offsets| {
            asked.extend_from_slice(offsets);
            Ok(vec![7_u32.to_le_bytes()])
        })
        .unwrap();
        assert_eq!(asked, [34 + 4 + 4 + 4 + 4 + 3]);
        let entries: Vec<(&str, u64)> = file
            .entries()
            .iter()
            .map(|entry| (entry.name.as_str(), entry.length))
            .collect();
        assert_eq!(entries, [("chr1", 9), ("chrM", 7)]);
        let short = TwoBit::read_using(&mut Cursor::new(&bytes), &sizes, |_, _| Ok(Vec::new()));
        assert!(short.is_err());
    }

    #[test]
    fn decodes_bases_across_byte_boundaries() {
        let bytes = encode(&[("chr1", "ACGTACGTA"), ("chr2", "TTGCA")], &PLAIN);
        assert_eq!(fetch(&bytes, 0, 0, 9), "ACGTACGTA");
        assert_eq!(fetch(&bytes, 0, 2, 7), "GTACG");
        assert_eq!(fetch(&bytes, 1, 1, 5), "TGCA");
    }

    #[test]
    fn n_blocks_read_as_n() {
        let bytes = encode(&[("chr1", "ACNNNTG")], &PLAIN);
        assert_eq!(fetch(&bytes, 0, 0, 7), "ACNNNTG");
        assert_eq!(fetch(&bytes, 0, 3, 6), "NNT");
    }

    /// Blocks out of order, as a damaged file can give, make no panic: genie changes the bases of
    /// each block in the region that it finds.
    #[test]
    fn blocks_out_of_order_make_no_panic() {
        let mut bases = *b"AAAAAAAAAA";
        let region = Region::between(30, 40);
        super::apply(
            &[(35, 37), (10, 20), (31, 32)],
            region,
            &mut bases,
            |base| {
                *base = b'N';
            },
        );
        assert_eq!(&bases, b"ANAAANNAAA");
    }

    #[test]
    fn mask_blocks_read_as_lowercase() {
        let bytes = encode(&[("chr1", "ACgtaCGnnA")], &PLAIN);
        assert_eq!(fetch(&bytes, 0, 0, 10), "ACgtaCGnnA");
        assert_eq!(fetch(&bytes, 0, 3, 8), "taCGn");
    }

    /// Counts the bytes that a reader gives.
    struct Counted<R> {
        inner: R,
        read: usize,
    }

    impl<R: std::io::Read> std::io::Read for Counted<R> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let count = self.inner.read(buffer)?;
            self.read += count;
            Ok(count)
        }
    }

    impl<R: std::io::Seek> std::io::Seek for Counted<R> {
        fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(to)
        }
    }

    /// A long sequence, as a chromosome in a genome on the web, has many mask blocks. A fetch
    /// reads only the mask blocks near its region.
    #[test]
    fn fetches_read_only_the_mask_blocks_near_their_region() {
        let bases: String = (0..200_000)
            .map(|index| if index % 10 < 2 { 'a' } else { 'C' })
            .collect();
        let bytes = encode(&[("chr1", &bases)], &PLAIN);
        let mut reader = Counted {
            inner: Cursor::new(&bytes),
            read: 0,
        };
        let file = TwoBit::read(&mut reader).unwrap();
        for (start, end) in [
            (99_995, 100_013),
            (0, 25),
            (199_990, 200_000),
            (12_345, 12_351),
            (0, 200_000),
        ] {
            reader.read = 0;
            let found = file
                .fetch(&mut reader, 0, Region::between(start, end))
                .unwrap();
            let wanted = &bases.as_bytes()[start as usize..end as usize];
            assert_eq!(found, wanted, "{start}-{end}");
            if end - start < 100 {
                assert!(reader.read < 2_000, "{start}-{end}: {} bytes", reader.read);
            }
        }
    }

    #[test]
    fn reads_every_byte_order_and_version() {
        for (big_endian, version) in [(true, 0), (false, 1), (true, 1)] {
            let format = Format {
                big_endian,
                version,
            };
            let bytes = encode(&[("chr1", "ACGTACGTA"), ("chr2", "GG")], &format);
            assert_eq!(fetch(&bytes, 0, 0, 9), "ACGTACGTA");
            assert_eq!(fetch(&bytes, 1, 0, 2), "GG");
        }
    }

    #[test]
    fn clamps_to_the_sequence_end() {
        let bytes = encode(&[("chr1", "ACGTACGTA")], &PLAIN);
        assert_eq!(fetch(&bytes, 0, 7, 20), "TA");
        assert_eq!(fetch(&bytes, 0, 30, 40), "");
    }

    #[test]
    fn rejects_other_files_and_versions() {
        let other = b"this is not a 2bit file".to_vec();
        assert!(matches!(
            TwoBit::read(&mut Cursor::new(&other)),
            Err(Error::Invalid(Reason::NotTwoBit))
        ));
        let future = encode(
            &[("chr1", "A")],
            &Format {
                big_endian: false,
                version: 2,
            },
        );
        assert!(matches!(
            TwoBit::read(&mut Cursor::new(&future)),
            Err(Error::Invalid(Reason::UnsupportedVersion))
        ));
    }

    #[test]
    fn sniff_recognizes_both_byte_orders() {
        assert!(sniff(&encode(&[("c", "A")], &PLAIN)));
        let big = Format {
            big_endian: true,
            version: 0,
        };
        assert!(sniff(&encode(&[("c", "A")], &big)));
    }

    #[test]
    fn sniff_rejects_other_data() {
        assert!(!sniff(b">chr1\nACGT\n"));
        assert!(!sniff(b"AC"));
        assert!(!sniff(b""));
    }

    #[test]
    fn invalid_errors_read_as_sentences() {
        assert_eq!(
            Error::Invalid(Reason::NotTwoBit).to_string(),
            "The file is not a 2bit file."
        );
    }
}
