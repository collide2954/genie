use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, OnceLock};

use genie_core::Region;

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
    masked: Vec<(u64, u64)>,
    bases: u64,
}

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

    pub fn read_with(
        reader: &mut (impl Read + Seek),
        sizes: &HashMap<String, u64>,
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
        for entry in &mut entries {
            if let Some(length) = sizes.get(&entry.name) {
                entry.length = *length;
                continue;
            }
            reader.seek(SeekFrom::Start(entry.offset))?;
            entry.length = u64::from(order.word(bytes(reader)?));
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
        apply(&layout.masked, region, &mut bases, u8::make_ascii_lowercase);
        Ok(bases)
    }

    fn layout(&self, reader: &mut (impl Read + Seek), index: usize) -> Result<Arc<Layout>, Error> {
        if let Some(layout) = self.layouts[index].get() {
            return Ok(Arc::clone(layout));
        }
        let entry = &self.entries[index];
        reader.seek(SeekFrom::Start(entry.offset + 4))?;
        let unknown = blocks(reader, self.order, entry.length)?;
        let masked = blocks(reader, self.order, entry.length)?;
        let _reserved: [u8; 4] = bytes(reader)?;
        let layout = Arc::new(Layout {
            unknown,
            masked,
            bases: reader.stream_position()?,
        });
        Ok(Arc::clone(self.layouts[index].get_or_init(|| layout)))
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

fn apply(blocks: &[(u64, u64)], region: Region, bases: &mut [u8], change: impl Fn(&mut u8)) {
    let first = blocks.partition_point(|&(_, end)| end <= region.start());
    for &(start, end) in &blocks[first..] {
        if start >= region.end() {
            break;
        }
        let from = (start.max(region.start()) - region.start()) as usize;
        let to = (end.min(region.end()) - region.start()) as usize;
        bases[from..to].iter_mut().for_each(&change);
    }
}

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

    #[test]
    fn mask_blocks_read_as_lowercase() {
        let bytes = encode(&[("chr1", "ACgtaCGnnA")], &PLAIN);
        assert_eq!(fetch(&bytes, 0, 0, 10), "ACgtaCGnnA");
        assert_eq!(fetch(&bytes, 0, 3, 8), "taCGn");
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
