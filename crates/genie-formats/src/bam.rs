use std::io::{self, ErrorKind, Read, Seek};

use genie_core::alignment::{Alignment, Mate, Op, REVERSE, Tags, parts};
use genie_core::{Region, Strand, modification};

use crate::bai::Index;
use crate::bgzf::Bgzf;
use crate::{Error, Reason};

const MAGIC: &[u8; 4] = b"BAM\x01";
const CODES: &[u8; 16] = b"=ACMGRSVTWYHKDBN";
const OPS: [Op; 9] = [
    Op::Match,
    Op::Insertion,
    Op::Deletion,
    Op::Skip,
    Op::SoftClip,
    Op::HardClip,
    Op::Padding,
    Op::Equal,
    Op::Diff,
];
const FIXED: usize = 32;
const UNMAPPED: u16 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub references: Vec<(String, u64)>,
}

pub fn header(reader: &mut impl Read) -> Result<Header, Error> {
    let mut magic = [0; 4];
    reader.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(Error::Invalid(Reason::NotBam));
    }
    let text = count(reader)? as u64;
    if io::copy(&mut reader.by_ref().take(text), &mut io::sink())? != text {
        return Err(bad());
    }
    let references = (0..count(reader)?)
        .map(|_| {
            let mut name = Vec::new();
            let length = count(reader)?;
            reader.by_ref().take(length as u64).read_to_end(&mut name)?;
            if name.len() != length || name.pop() != Some(0) {
                return Err(bad());
            }
            let size = u64::from(word(reader)?);
            Ok((String::from_utf8_lossy(&name).into_owned(), size))
        })
        .collect::<Result<_, Error>>()?;
    Ok(Header { references })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub reference: Option<usize>,
    pub start: u64,
    pub end: u64,
    pub mapped: bool,
}

pub fn record(
    reader: &mut impl Read,
    buffer: &mut Vec<u8>,
) -> Result<Option<(i32, Alignment)>, Error> {
    if !raw(reader, buffer)? {
        return Ok(None);
    }
    decode(buffer).map(Some)
}

pub fn raw(reader: &mut impl Read, buffer: &mut Vec<u8>) -> Result<bool, Error> {
    let mut size = [0; 4];
    let mut filled = 0;
    while filled < size.len() {
        match reader.read(&mut size[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    match filled {
        0 => return Ok(false),
        4 => {}
        _ => return Err(bad()),
    }
    let size = usize::try_from(i32::from_le_bytes(size)).map_err(|_| bad())?;
    if size < FIXED {
        return Err(bad());
    }
    buffer.clear();
    reader.by_ref().take(size as u64).read_to_end(buffer)?;
    if buffer.len() != size {
        return Err(bad());
    }
    Ok(true)
}

pub fn place(record: &[u8]) -> Result<Place, Error> {
    let fixed = record.get(..FIXED).ok_or_else(bad)?;
    let int =
        |at: usize| i32::from_le_bytes([fixed[at], fixed[at + 1], fixed[at + 2], fixed[at + 3]]);
    let short = |at: usize| u16::from_le_bytes([fixed[at], fixed[at + 1]]);
    let mapped = short(14) & UNMAPPED == 0;
    let operations = usize::from(short(12));
    let from = FIXED + usize::from(fixed[8]);
    let cigar = record.get(from..from + operations * 4).ok_or_else(bad)?;
    let length = if mapped && operations > 0 {
        cigar
            .as_chunks::<4>()
            .0
            .iter()
            .map(|operation| u32::from_le_bytes(*operation))
            .filter(|value| matches!(value & 0xF, 0 | 2 | 3 | 7 | 8))
            .map(|value| u64::from(value >> 4))
            .sum()
    } else {
        1
    };
    let start = u64::try_from(int(4)).unwrap_or(0);
    Ok(Place {
        reference: usize::try_from(int(0)).ok(),
        start,
        end: start + length,
        mapped,
    })
}

pub fn query<R: Read + Seek>(
    index: &Index,
    reader: &mut Bgzf<R>,
    reference: usize,
    region: Region,
    mut each: impl FnMut(Alignment) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut buffer = Vec::new();
    for chunk in index.chunks(reference, region) {
        if reader.virtual_offset() != chunk.start {
            reader.seek(chunk.start)?;
        }
        while reader.virtual_offset() < chunk.end {
            let Some((id, alignment)) = record(reader, &mut buffer)? else {
                break;
            };
            if usize::try_from(id).ok() != Some(reference) || alignment.start >= region.end() {
                return Ok(());
            }
            if alignment.span().overlaps(region) {
                each(alignment)?;
            }
        }
    }
    Ok(())
}

struct Fields<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Fields<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8], Error> {
        let part = self.bytes.get(self.at..self.at + size).ok_or_else(bad)?;
        self.at += size;
        Ok(part)
    }
}

fn decode(bytes: &[u8]) -> Result<(i32, Alignment), Error> {
    let int =
        |at: usize| i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let short = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let length = usize::try_from(int(16)).map_err(|_| bad())?;
    let mut fields = Fields { bytes, at: FIXED };
    let name = fields.take(usize::from(bytes[8]))?;
    let name = String::from_utf8_lossy(name.strip_suffix(&[0]).unwrap_or(name)).into_owned();
    let (operations, _) = fields.take(usize::from(short(12)) * 4)?.as_chunks::<4>();
    let cigar = operations
        .iter()
        .map(|operation| {
            let value = u32::from_le_bytes(*operation);
            let op = OPS.get((value & 0xF) as usize).ok_or_else(bad)?;
            Ok((*op, value >> 4))
        })
        .collect::<Result<_, Error>>()?;
    let cigar = long_cigar(cigar, length, || &bytes[fields.at..]);
    let packed = fields.take(length.div_ceil(2))?;
    let bases: Vec<u8> = (0..length)
        .map(|index| {
            let byte = packed[index / 2];
            let nibble = if index % 2 == 0 {
                byte >> 4
            } else {
                byte & 0xF
            };
            CODES[usize::from(nibble)]
        })
        .collect();
    let qualities = fields.take(length)?;
    let qualities = if qualities.first() == Some(&0xFF) {
        Vec::new()
    } else {
        qualities.to_vec()
    };
    let mate = usize::try_from(int(20))
        .ok()
        .zip(u64::try_from(int(24)).ok())
        .map(|(contig, start)| Mate {
            contig,
            start,
            insert: i64::from(int(28)),
        });
    let flags = short(14);
    let clipped = cigar.iter().any(|(op, _)| *op == Op::HardClip);
    let tags = tags(&bytes[fields.at..], &bases, flags & REVERSE != 0, clipped);
    let alignment = Alignment {
        name,
        start: u64::try_from(int(4)).unwrap_or(0),
        flags,
        mapq: bytes[9],
        cigar,
        bases,
        qualities,
        mate,
        tags,
    };
    Ok((int(0), alignment))
}

fn integer(kind: u8, value: &[u8]) -> Option<i64> {
    let pair = || [value[0], value[1]];
    let quad = || [value[0], value[1], value[2], value[3]];
    Some(match kind {
        b'c' => i64::from(value[0].cast_signed()),
        b'C' => i64::from(value[0]),
        b's' => i64::from(i16::from_le_bytes(pair())),
        b'S' => i64::from(u16::from_le_bytes(pair())),
        b'i' => i64::from(i32::from_le_bytes(quad())),
        b'I' => i64::from(u32::from_le_bytes(quad())),
        _ => return None,
    })
}

fn long_cigar<'a>(
    cigar: Vec<(Op, u32)>,
    length: usize,
    rest: impl FnOnce() -> &'a [u8],
) -> Vec<(Op, u32)> {
    let [(Op::SoftClip, clipped), (Op::Skip, _)] = cigar.as_slice() else {
        return cigar;
    };
    if *clipped as usize != length {
        return cigar;
    }
    let rest = rest();
    let aux = rest.get(length.div_ceil(2) + length..).unwrap_or_default();
    let mut real = None;
    walk(aux, |key, kind, value| {
        if key == *b"CG"
            && kind == b'B'
            && let [b'I', _, _, _, _, words @ ..] = value
        {
            real = words
                .as_chunks::<4>()
                .0
                .iter()
                .map(|word| {
                    let value = u32::from_le_bytes(*word);
                    OPS.get((value & 0xF) as usize).map(|op| (*op, value >> 4))
                })
                .collect();
        }
    });
    real.unwrap_or(cigar)
}

fn tags(aux: &[u8], bases: &[u8], reverse: bool, clipped: bool) -> Tags {
    let mut tags = Tags::default();
    let (mut mm, mut ml, mut mn) = (None, None, None);
    walk(aux, |key, kind, value| match key {
        [b'H', b'P'] => tags.phase = integer(kind, value),
        [b'R', b'G'] if kind == b'Z' => {
            let text = value.strip_suffix(&[0]).unwrap_or(value);
            tags.group = Some(String::from_utf8_lossy(text).into_owned());
        }
        [b'S', b'A'] if kind == b'Z' => {
            let text = value.strip_suffix(&[0]).unwrap_or(value);
            tags.parts = parts(&String::from_utf8_lossy(text));
        }
        [b'X', b'S'] if kind == b'A' => tags.xs = strand(value),
        [b't', b's'] if kind == b'A' => tags.ts = strand(value),
        [b'M', b'M' | b'm'] if kind == b'Z' => mm = Some(value.strip_suffix(&[0]).unwrap_or(value)),
        [b'M', b'L' | b'l'] if kind == b'B' => {
            if let [b'C', _, _, _, _, chances @ ..] = value {
                ml = Some(chances);
            }
        }
        [b'M', b'N'] => mn = integer(kind, value),
        _ => {}
    });
    let fits = mn.map_or(!clipped, |length| {
        usize::try_from(length).ok() == Some(bases.len())
    });
    if let (Some(mm), Some(ml), true) = (mm, ml, fits) {
        tags.mods = modification::parse(&String::from_utf8_lossy(mm), ml, bases, reverse);
    }
    tags
}

fn strand(value: &[u8]) -> Option<Strand> {
    match value.first() {
        Some(b'+') => Some(Strand::Forward),
        Some(b'-') => Some(Strand::Reverse),
        _ => None,
    }
}

fn walk<'a>(mut aux: &'a [u8], mut each: impl FnMut([u8; 2], u8, &'a [u8])) {
    while let [first, second, kind, rest @ ..] = aux {
        let size = match kind {
            b'A' | b'c' | b'C' => 1,
            b's' | b'S' => 2,
            b'i' | b'I' | b'f' => 4,
            b'Z' | b'H' => rest
                .iter()
                .position(|byte| *byte == 0)
                .map_or(rest.len(), |end| end + 1),
            b'B' => {
                let [sub, a, b, c, d, ..] = rest else {
                    break;
                };
                let width = match sub {
                    b'c' | b'C' => 1,
                    b's' | b'S' => 2,
                    b'i' | b'I' | b'f' => 4,
                    _ => break,
                };
                5 + u32::from_le_bytes([*a, *b, *c, *d]) as usize * width
            }
            _ => break,
        };
        let Some(value) = rest.get(..size) else {
            break;
        };
        each([*first, *second], *kind, value);
        aux = &rest[size..];
    }
}

fn word(reader: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn count(reader: &mut impl Read) -> Result<usize, Error> {
    usize::try_from(word(reader)?.cast_signed()).map_err(|_| bad())
}

fn bad() -> Error {
    Error::Invalid(Reason::BadRecord)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::alignment::{Alignment, Op};
    use genie_core::{Region, Strand};

    use super::{Place, header, place, query, raw, record};
    use crate::{
        Error, Reason,
        bai::{Index, tests::index},
        bgzf::{
            Bgzf,
            tests::{block, file},
        },
        tabix::tests::Bins,
    };

    const CODES: &[u8; 16] = b"=ACMGRSVTWYHKDBN";

    fn int(value: usize) -> [u8; 4] {
        i32::try_from(value).unwrap().to_le_bytes()
    }

    fn head(references: &[(&str, u32)]) -> Vec<u8> {
        let text = b"@HD\tVN:1.6\tSO:coordinate\n";
        let mut bytes = b"BAM\x01".to_vec();
        bytes.extend(int(text.len()));
        bytes.extend(text);
        bytes.extend(int(references.len()));
        for (name, length) in references {
            bytes.extend(int(name.len() + 1));
            bytes.extend(name.as_bytes());
            bytes.push(0);
            bytes.extend(length.to_le_bytes());
        }
        bytes
    }

    fn body(start: i32, name: &str, flags: u16, cigar: &[(u32, u32)], bases: &str) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(0_i32.to_le_bytes());
        body.extend(start.to_le_bytes());
        body.push(u8::try_from(name.len() + 1).unwrap());
        body.push(60);
        body.extend(0_u16.to_le_bytes());
        body.extend(u16::try_from(cigar.len()).unwrap().to_le_bytes());
        body.extend(flags.to_le_bytes());
        body.extend(int(bases.len()));
        body.extend((-1_i32).to_le_bytes());
        body.extend((-1_i32).to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        body.extend(name.as_bytes());
        body.push(0);
        for (length, op) in cigar {
            body.extend((length << 4 | op).to_le_bytes());
        }
        let code = |base: &u8| CODES.iter().position(|known| known == base).unwrap() as u8;
        for pair in bases.as_bytes().chunks(2) {
            body.push(code(&pair[0]) << 4 | pair.get(1).map_or(0, code));
        }
        body
    }

    fn sized(mut body: Vec<u8>) -> Vec<u8> {
        body.extend(b"NMC\x00");
        let mut bytes = int(body.len()).to_vec();
        bytes.extend(body);
        bytes
    }

    fn read(start: i32, name: &str, flags: u16, cigar: &[(u32, u32)], bases: &str) -> Vec<u8> {
        let mut body = body(start, name, flags, cigar, bases);
        body.extend(bases.bytes().map(|_| 30));
        sized(body)
    }

    fn alignment(
        start: u64,
        name: &str,
        flags: u16,
        cigar: Vec<(Op, u32)>,
        bases: &str,
    ) -> Alignment {
        Alignment {
            name: name.to_owned(),
            start,
            flags,
            mapq: 60,
            cigar,
            bases: bases.as_bytes().to_vec(),
            qualities: vec![30; bases.len()],
            mate: None,
            tags: genie_core::alignment::Tags::default(),
        }
    }

    #[test]
    fn places_records_like_samtools() {
        let cigar = [
            (5, 4),
            (10, 0),
            (3, 2),
            (2, 1),
            (4, 3),
            (6, 7),
            (1, 8),
            (7, 5),
        ];
        let bases = "ACGTACGTACGTACGTACGTACGT";
        let mut bytes = Cursor::new(read(100, "a", 0, &cigar, bases));
        let mut buffer = Vec::new();
        assert!(raw(&mut bytes, &mut buffer).unwrap());
        let mapped = Place {
            reference: Some(0),
            start: 100,
            end: 124,
            mapped: true,
        };
        assert_eq!(place(&buffer).unwrap(), mapped);
        assert!(!raw(&mut bytes, &mut buffer).unwrap());
        let unmapped = read(100, "b", 4, &cigar, bases);
        let found = place(&unmapped[4..]).unwrap();
        assert_eq!((found.end, found.mapped), (101, false));
        let mut unplaced = read(-1, "c", 4, &[], "AC");
        unplaced[4..8].copy_from_slice(&(-1_i32).to_le_bytes());
        assert_eq!(place(&unplaced[4..]).unwrap().reference, None);
    }

    #[test]
    fn reads_the_reference_list() {
        let bytes = head(&[("chr1", 1_000), ("chr2", 500)]);
        let header = header(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(
            header.references,
            [("chr1".to_owned(), 1_000), ("chr2".to_owned(), 500)]
        );
    }

    #[test]
    fn rejects_other_files() {
        let result = header(&mut Cursor::new(b"BAI\x01\0\0\0\0".to_vec()));
        assert!(matches!(result, Err(Error::Invalid(Reason::NotBam))));
    }

    #[test]
    fn long_cigars_come_from_the_cg_tag() {
        let bases = "ACGTACGTACGTACGTA";
        let mut long = body(40, "long", 0, &[(17, 4), (15, 3)], bases);
        long.extend(bases.bytes().map(|_| 30));
        long.extend(b"CGBI");
        long.extend(3_u32.to_le_bytes());
        for (length, op) in [(10_u32, 0_u32), (2, 1), (5, 0)] {
            long.extend((length << 4 | op).to_le_bytes());
        }
        let mut reader = Cursor::new(sized(long));
        let (_, read) = record(&mut reader, &mut Vec::new()).unwrap().unwrap();
        let expected = [(Op::Match, 10), (Op::Insertion, 2), (Op::Match, 5)];
        assert_eq!(read.cigar, expected);
        let mut short = body(40, "short", 0, &[(17, 0)], bases);
        short.extend(bases.bytes().map(|_| 30));
        short.extend(b"CGBI");
        short.extend(1_u32.to_le_bytes());
        short.extend((5_u32 << 4).to_le_bytes());
        let mut reader = Cursor::new(sized(short));
        let (_, read) = record(&mut reader, &mut Vec::new()).unwrap().unwrap();
        assert_eq!(read.cigar, [(Op::Match, 17)]);
    }

    #[test]
    fn split_parts_come_from_the_sa_tag() {
        let bases = "ACGTACGTAC";
        let mut split = body(40, "split", 0, &[(6, 0), (4, 4)], bases);
        split.extend(bases.bytes().map(|_| 30));
        split.extend(b"SAZchr2,1234,+,6S4M,60,0;\0");
        let mut reader = Cursor::new(sized(split));
        let (_, read) = record(&mut reader, &mut Vec::new()).unwrap().unwrap();
        let parts: Vec<(&str, u64)> = read
            .tags
            .parts
            .iter()
            .map(|part| (part.contig.as_str(), part.start))
            .collect();
        assert_eq!(parts, [("chr2", 1233)]);
    }

    #[test]
    fn splice_strands_come_from_the_xs_and_ts_tags() {
        let bases = "ACGTACGTAC";
        let mut spliced = body(40, "xs", 0, &[(5, 0), (100, 3), (5, 0)], bases);
        spliced.extend(bases.bytes().map(|_| 30));
        spliced.extend(b"XSA-tsA+");
        let mut reader = Cursor::new(sized(spliced));
        let (_, read) = record(&mut reader, &mut Vec::new()).unwrap().unwrap();
        assert_eq!(
            (read.tags.xs, read.tags.ts),
            (Some(Strand::Reverse), Some(Strand::Forward))
        );
    }

    #[test]
    fn base_modifications_come_from_the_mm_and_ml_tags() {
        let bases = "ACGTCGACCG";
        let tagged = |flags: u16, cigar: &[(u32, u32)], extra: &[u8]| {
            let mut bytes = body(40, "mods", flags, cigar, bases);
            bytes.extend(bases.bytes().map(|_| 30));
            bytes.extend(b"MMZC+m?,0,1;\0MLBC");
            bytes.extend(2_u32.to_le_bytes());
            bytes.extend([200, 30]);
            bytes.extend(extra);
            let mut reader = Cursor::new(sized(bytes));
            let (_, read) = record(&mut reader, &mut Vec::new()).unwrap().unwrap();
            read.tags
                .mods
                .calls
                .iter()
                .map(|call| (call.at, call.chance))
                .collect::<Vec<_>>()
        };
        assert_eq!(tagged(0, &[(10, 0)], b""), [(1, 200), (7, 30)]);
        assert_eq!(tagged(0x10, &[(10, 0)], b""), [(2, 30), (9, 200)]);
        assert_eq!(tagged(0, &[(5, 5), (10, 0)], b""), []);
        let length = b"MNi\x0a\0\0\0";
        assert_eq!(tagged(0, &[(5, 5), (10, 0)], length), [(1, 200), (7, 30)]);
        assert_eq!(tagged(0, &[(10, 0)], b"MNi\x0c\0\0\0"), []);
    }

    #[test]
    fn decodes_records_until_the_end() {
        let mut bytes = read(
            99,
            "r1",
            0x10,
            &[(5, 0), (2, 1), (3, 2), (4, 0)],
            "ACGTNACGTNA",
        );
        let mut plain = body(7, "r2", 0, &[(3, 0)], "GGT");
        plain.extend([0xFF; 3]);
        bytes.extend(sized(plain));
        let mut reader = Cursor::new(bytes);
        let mut buffer = Vec::new();
        let cigar = vec![
            (Op::Match, 5),
            (Op::Insertion, 2),
            (Op::Deletion, 3),
            (Op::Match, 4),
        ];
        assert_eq!(
            record(&mut reader, &mut buffer).unwrap(),
            Some((0, alignment(99, "r1", 0x10, cigar, "ACGTNACGTNA")))
        );
        let (_, second) = record(&mut reader, &mut buffer).unwrap().unwrap();
        assert_eq!(second.bases, b"GGT");
        assert_eq!(second.qualities, []);
        assert_eq!(record(&mut reader, &mut buffer).unwrap(), None);
    }

    #[test]
    fn queries_reads_that_overlap_a_region() {
        let head = head(&[("chr1", 1_000)]);
        let mut data = head.clone();
        for start in [10, 100, 200] {
            data.extend(read(
                start,
                &format!("r{start}"),
                0,
                &[(10, 0)],
                "ACGTACGTAC",
            ));
        }
        let end = (block(&data).len() as u64) << 16;
        let bins: Bins = &[(4681, &[(head.len() as u64, end)])];
        let index = Index::read(Cursor::new(index(&[(bins, &[0])]))).unwrap();
        let mut reader = Bgzf::new(Cursor::new(file(&[&data])));
        let mut names = Vec::new();
        let region = Region::new(95, 205).unwrap();
        query(&index, &mut reader, 0, region, |read| {
            names.push(read.name);
            Ok(())
        })
        .unwrap();
        assert_eq!(names, ["r100", "r200"]);
    }
}
