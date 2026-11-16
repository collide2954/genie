//! BCF, the binary form of VCF that bcftools writes: a BGZF file with the text of a VCF header
//! and records of typed values. A record names its contig, its filters and its fields by their
//! places in dictionaries that the header gives.

use std::io::{ErrorKind, Read, Seek};

use genie_core::Region;
use genie_core::variant::{Call, Variant};

use crate::annotations::{self, Layout};
use crate::bgzf::Bgzf;
use crate::features::Entry;
use crate::tabix::Index;
use crate::table::Table;
use crate::{Error, Reason, sv, vcf};

const MAGIC: &[u8; 3] = b"BCF";
const TEXT_LIMIT: usize = 1 << 28;
const RECORD_LIMIT: usize = 1 << 28;
const DICTIONARY_LIMIT: usize = 1 << 20;
/// The bytes of the fixed fields at the start of a record.
const FIXED: usize = 24;
const PASS: &str = "PASS";
const MISSING_FLOAT: u32 = 0x7F80_0001;
const END_FLOAT: u32 = 0x7F80_0002;
const INT8: u8 = 1;
const INT16: u8 = 2;
const INT32: u8 = 3;
const FLOAT: u8 = 5;
const CHAR: u8 = 7;

/// The header of a BCF file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    /// The contigs, in the order of their dictionary.
    pub contigs: Vec<String>,
    /// The dictionary of strings: PASS, then the IDs of the FILTER, INFO and FORMAT lines.
    strings: Vec<String>,
    /// The samples and the INFO lines, as in a VCF header.
    pub vcf: vcf::Header,
}

/// Reads the header at the start of the file, after which the records start.
pub fn header(reader: &mut impl Read) -> Result<Header, Error> {
    let mut magic = [0; 5];
    reader
        .read_exact(&mut magic)
        .map_err(|_| Error::Invalid(Reason::NotBcf))?;
    if &magic[..3] != MAGIC || magic[3] != 2 {
        return Err(Error::Invalid(Reason::NotBcf));
    }
    let mut size = [0; 4];
    reader.read_exact(&mut size)?;
    let size = u32::from_le_bytes(size) as usize;
    if size > TEXT_LIMIT {
        return Err(Error::Invalid(Reason::NotBcf));
    }
    let mut text = vec![0; size];
    reader.read_exact(&mut text)?;
    let text = String::from_utf8_lossy(&text);
    let text = text.trim_end_matches('\0');
    let mut strings = vec![PASS.to_owned()];
    let mut contigs = Vec::new();
    for line in text.lines() {
        let words = ["##FILTER=<", "##INFO=<", "##FORMAT=<"]
            .iter()
            .find_map(|prefix| line.strip_prefix(prefix));
        let (list, body) = match (words, line.strip_prefix("##contig=<")) {
            (Some(body), _) => (&mut strings, body),
            (None, Some(body)) => (&mut contigs, body),
            (None, None) => continue,
        };
        if let Some(id) = attribute(body, "ID") {
            let place = attribute(body, "IDX").and_then(|text| text.parse().ok());
            insert(list, id, place);
        }
    }
    Ok(Header {
        contigs,
        strings,
        vcf: vcf::header(text.as_bytes())?,
    })
}

/// The value of an attribute of a header line, as `DP` for `ID` in `ID=DP,Number=1,…>`.
fn attribute<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let mut rest = body;
    loop {
        let (name, after) = rest.split_once('=')?;
        let name = name.trim_start_matches(',');
        if let Some(quoted) = after.strip_prefix('"') {
            let mut escaped = false;
            let end = quoted.char_indices().find_map(|(at, character)| {
                let close = !escaped && character == '"';
                escaped = !escaped && character == '\\';
                close.then_some(at)
            })?;
            if name == key {
                return Some(&quoted[..end]);
            }
            rest = &quoted[end + 1..];
        } else {
            let end = after.find([',', '>']).unwrap_or(after.len());
            if name == key {
                return Some(&after[..end]);
            }
            rest = &after[end..];
        }
    }
}

/// Puts an ID in a dictionary: at its IDX, or else at the end if the dictionary does not have
/// it yet.
fn insert(list: &mut Vec<String>, id: &str, place: Option<usize>) {
    match place {
        Some(place) if place < DICTIONARY_LIMIT => {
            if list.len() <= place {
                list.resize(place + 1, String::new());
            }
            id.clone_into(&mut list[place]);
        }
        _ => {
            if !list.iter().any(|known| known == id) {
                list.push(id.to_owned());
            }
        }
    }
}

/// Reads the next record into `bytes`. Gives the length of its shared part, or nothing at the
/// end of the file.
fn next(reader: &mut impl Read, bytes: &mut Vec<u8>) -> Result<Option<usize>, Error> {
    let mut lengths = [0; 8];
    match reader.read_exact(&mut lengths) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let shared = u32::from_le_bytes([lengths[0], lengths[1], lengths[2], lengths[3]]) as usize;
    let individual = u32::from_le_bytes([lengths[4], lengths[5], lengths[6], lengths[7]]) as usize;
    if shared < FIXED || shared + individual > RECORD_LIMIT {
        return Err(bad());
    }
    bytes.resize(shared + individual, 0);
    reader.read_exact(bytes)?;
    Ok(Some(shared))
}

/// The contig, the start and the end of a record, from its fixed fields.
fn place(bytes: &[u8]) -> Result<(usize, u64, u64), Error> {
    let mut values = Values { data: bytes, at: 0 };
    let contig = usize::try_from(values.i32()?).map_err(|_| bad())?;
    let start = u64::try_from(values.i32()?).unwrap_or(0);
    let length = u64::try_from(values.i32()?).unwrap_or(1).max(1);
    Ok((contig, start, start + length))
}

/// The contig of a record and its variant. The annotations of the `layouts` leave the INFO and
/// go into the variant, as for VCF.
fn record(
    bytes: &[u8],
    shared: usize,
    header: &Header,
    layouts: &[Layout],
) -> Result<(usize, Variant), Error> {
    let (contig, start, end) = place(bytes)?;
    let mut values = Values {
        data: &bytes[..shared],
        at: 12,
    };
    let quality = values.u32()?;
    let quality = (quality != MISSING_FLOAT)
        .then(|| f32::from_bits(quality))
        .filter(|quality| quality.is_finite());
    let counts = values.u32()?;
    let (alleles, infos) = (counts >> 16, counts & 0xFFFF);
    let layout = values.u32()?;
    let (formats, samples) = (layout >> 24, layout & 0x00FF_FFFF);
    let id = values.text_value()?;
    let mut texts = Vec::new();
    for _ in 0..alleles {
        texts.push(values.text_value()?);
    }
    let (kind, count) = values.kind()?;
    let filters: Vec<&str> = values
        .ints(kind, count)?
        .into_iter()
        .flatten()
        .filter_map(|place| header.string(place))
        .collect();
    let mut info = Vec::new();
    for _ in 0..infos {
        let key = values.int()?.and_then(|place| header.string(place));
        let value = values.info_value()?;
        if let Some(key) = key {
            info.push(value.map_or_else(|| key.to_owned(), |value| format!("{key}={value}")));
        }
    }
    let info = if info.is_empty() {
        ".".to_owned()
    } else {
        info.join(";")
    };
    let calls = genotypes(&bytes[shared..], (formats, samples), header)?;
    let mut texts = texts.into_iter();
    let reference = texts.next().unwrap_or_default();
    let mut alternates: Vec<String> = texts.collect();
    if alternates.is_empty() {
        alternates.push(".".to_owned());
    }
    let span = Region::between(start, end);
    let structure = sv::structure(&reference, &alternates[0], &info, span);
    let (annotations, kept) = annotations::split(&info, layouts);
    let variant = Variant {
        id: if id == "." { String::new() } else { id },
        span,
        reference,
        alternates,
        quality,
        filter: if filters.is_empty() {
            ".".to_owned()
        } else {
            filters.join(";")
        },
        info: vcf::cut(&kept),
        annotations,
        structure,
        calls,
    };
    Ok((contig, variant))
}

/// The calls of the samples, from their GT field.
fn genotypes(
    bytes: &[u8],
    (formats, samples): (u32, u32),
    header: &Header,
) -> Result<Vec<Call>, Error> {
    let samples = samples as usize;
    let mut values = Values { data: bytes, at: 0 };
    for _ in 0..formats {
        let key = values.int()?.and_then(|place| header.string(place));
        let (kind, count) = values.kind()?;
        if key == Some("GT") {
            let mut calls = Vec::with_capacity(samples);
            for _ in 0..samples {
                let alleles: Vec<String> = values
                    .ints(kind, count)?
                    .into_iter()
                    .map(|allele| match allele {
                        Some(value) if value >> 1 > 0 => ((value >> 1) - 1).to_string(),
                        _ => ".".to_owned(),
                    })
                    .collect();
                calls.push(Call::parse(&alleles.join("/")));
            }
            return Ok(calls);
        }
        values.take(width(kind)? * count * samples)?;
    }
    Ok(vec![Call::Missing; samples])
}

impl Header {
    fn string(&self, place: i32) -> Option<&str> {
        let place = usize::try_from(place).ok()?;
        self.strings
            .get(place)
            .map(String::as_str)
            .filter(|text| !text.is_empty())
    }
}

/// The variants of a region, with the CSI index of the file.
pub fn query<R: Read + Seek>(
    index: &Index,
    reader: &mut Bgzf<R>,
    header: &Header,
    (contig, region): (&str, Region),
    layouts: &[Layout],
) -> Result<Vec<Variant>, Error> {
    let Some(wanted) = header.contigs.iter().position(|name| name == contig) else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    let mut bytes = Vec::new();
    for chunk in index.chunks(contig, region) {
        if reader.virtual_offset() != chunk.start {
            reader.seek(chunk.start)?;
        }
        while reader.virtual_offset() < chunk.end {
            let Some(shared) = next(reader, &mut bytes)? else {
                break;
            };
            let (place, start, end) = place(&bytes)?;
            if place != wanted || start >= region.end() {
                return Ok(found);
            }
            if end > region.start() {
                found.push(record(&bytes, shared, header, layouts)?.1);
            }
        }
    }
    Ok(found)
}

/// All variants of a file, for a file without an index.
pub fn read_table(mut reader: impl Read) -> Result<(Header, Table<Variant>), Error> {
    let header = header(&mut reader)?;
    let layouts = annotations::layouts(&header.vcf.infos);
    let mut rows = Vec::new();
    let mut bytes = Vec::new();
    while let Some(shared) = next(&mut reader, &mut bytes)? {
        let (contig, variant) = record(&bytes, shared, &header, &layouts)?;
        let name = header.contigs.get(contig).ok_or_else(bad)?.clone();
        rows.push((name, variant.span, variant));
    }
    Ok((header, Table::new(rows)))
}

/// The IDs of the variants of a file, for the locus box.
pub fn names(mut reader: impl Read) -> Result<Vec<Entry>, Error> {
    let header = header(&mut reader)?;
    let mut entries = Vec::new();
    let mut bytes = Vec::new();
    while let Some(shared) = next(&mut reader, &mut bytes)? {
        let (contig, start, end) = place(&bytes)?;
        let mut values = Values {
            data: &bytes[..shared],
            at: FIXED,
        };
        let ids = values.text_value()?;
        let Some(contig) = header.contigs.get(contig) else {
            continue;
        };
        for name in ids.split(';').filter(|name| !matches!(*name, "" | ".")) {
            entries.push(Entry {
                name: name.to_owned(),
                contig: contig.clone(),
                span: Region::between(start, end),
            });
        }
    }
    Ok(entries)
}

fn bad() -> Error {
    Error::Invalid(Reason::BadBcf)
}

/// The bytes of a value of a type.
fn width(kind: u8) -> Result<usize, Error> {
    match kind {
        0 => Ok(0),
        INT8 | CHAR => Ok(1),
        INT16 => Ok(2),
        INT32 | FLOAT => Ok(4),
        _ => Err(bad()),
    }
}

/// A reader of the typed values of a record.
struct Values<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Values<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let bytes = self
            .data
            .get(self.at..self.at.checked_add(count).ok_or_else(bad)?)
            .ok_or_else(bad)?;
        self.at += count;
        Ok(bytes)
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()?.cast_signed())
    }

    /// The type and the number of the values that follow.
    fn kind(&mut self) -> Result<(u8, usize), Error> {
        let byte = self.take(1)?[0];
        let (kind, count) = (byte & 0x0F, usize::from(byte >> 4));
        if count < 15 {
            return Ok((kind, count));
        }
        let count = self
            .int()?
            .and_then(|count| usize::try_from(count).ok())
            .ok_or_else(bad)?;
        Ok((kind, count))
    }

    /// A typed integer, as a key or a count.
    fn int(&mut self) -> Result<Option<i32>, Error> {
        let (kind, count) = self.kind()?;
        Ok(self.ints(kind, count)?.first().copied().flatten())
    }

    /// Integers of a type, with nothing for a missing value. A vector stops at its end marker.
    fn ints(&mut self, kind: u8, count: usize) -> Result<Vec<Option<i32>>, Error> {
        let size = width(kind)?;
        let bytes = self.take(size * count)?;
        let (missing, end) = match kind {
            0 => return Ok(Vec::new()),
            INT8 => (i32::from(i8::MIN), i32::from(i8::MIN) + 1),
            INT16 => (i32::from(i16::MIN), i32::from(i16::MIN) + 1),
            INT32 => (i32::MIN, i32::MIN + 1),
            _ => return Err(bad()),
        };
        let mut found = Vec::with_capacity(count);
        for value in bytes.chunks(size) {
            let value = match value {
                [one] => i32::from(i8::from_le_bytes([*one])),
                [low, high] => i32::from(i16::from_le_bytes([*low, *high])),
                [a, b, c, d] => i32::from_le_bytes([*a, *b, *c, *d]),
                _ => return Err(bad()),
            };
            if value == end {
                break;
            }
            found.push((value != missing).then_some(value));
        }
        Ok(found)
    }

    fn floats(&mut self, count: usize) -> Result<Vec<Option<f32>>, Error> {
        let bytes = self.take(4 * count)?;
        let mut found = Vec::with_capacity(count);
        for word in bytes.chunks(4) {
            let bits = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
            if bits == END_FLOAT {
                break;
            }
            found.push((bits != MISSING_FLOAT).then(|| f32::from_bits(bits)));
        }
        Ok(found)
    }

    fn text(&mut self, count: usize) -> Result<String, Error> {
        let bytes = self.take(count)?;
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
    }

    /// A typed string, as the ID or an allele.
    fn text_value(&mut self) -> Result<String, Error> {
        let (kind, count) = self.kind()?;
        if kind == CHAR {
            return self.text(count);
        }
        self.take(width(kind)? * count)?;
        Ok(String::new())
    }

    /// The value of an INFO field as VCF text, or nothing for a flag.
    fn info_value(&mut self) -> Result<Option<String>, Error> {
        let (kind, count) = self.kind()?;
        if kind == 0 || count == 0 {
            self.take(width(kind)? * count)?;
            return Ok(None);
        }
        let joined = |items: Vec<String>| items.join(",");
        Ok(Some(match kind {
            CHAR => self.text(count)?,
            FLOAT => joined(
                self.floats(count)?
                    .into_iter()
                    .map(|value| value.map_or_else(|| ".".to_owned(), |value| value.to_string()))
                    .collect(),
            ),
            _ => joined(
                self.ints(kind, count)?
                    .into_iter()
                    .map(|value| value.map_or_else(|| ".".to_owned(), |value| value.to_string()))
                    .collect(),
            ),
        }))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::{Cursor, Write};

    use genie_core::Region;
    use genie_core::variant::{Call, Sv};

    use super::{header, names, query, read_table};
    use crate::annotations;
    use crate::bgzf::{self, Bgzf};
    use crate::binning::{Builder, Scheme, put};
    use crate::tabix::Index;
    use crate::{Error, Reason};

    const HEADER: &str = "##fileformat=VCFv4.2\n\
        ##FILTER=<ID=PASS,Description=\"All filters passed\",IDX=0>\n\
        ##INFO=<ID=DP,Number=1,Type=Integer,Description=\"Read depth\",IDX=1>\n\
        ##INFO=<ID=AF,Number=A,Type=Float,Description=\"Allele frequency\",IDX=2>\n\
        ##INFO=<ID=DB,Number=0,Type=Flag,Description=\"dbSNP, \\\"build\\\" 151\",IDX=3>\n\
        ##INFO=<ID=CSQ,Number=.,Type=String,Description=\"Consequence annotations from Ensembl \
        VEP. Format: Allele|Consequence|IMPACT|SYMBOL\",IDX=4>\n\
        ##INFO=<ID=SVTYPE,Number=1,Type=String,Description=\"Type of the variant\",IDX=5>\n\
        ##INFO=<ID=END,Number=1,Type=Integer,Description=\"End\",IDX=6>\n\
        ##FILTER=<ID=q10,Description=\"Quality below 10\",IDX=7>\n\
        ##FORMAT=<ID=GT,Number=1,Type=String,Description=\"Genotype\",IDX=8>\n\
        ##FORMAT=<ID=DP,Number=1,Type=Integer,Description=\"Read depth\",IDX=1>\n\
        ##contig=<ID=chr1,length=10000,IDX=0>\n\
        ##contig=<ID=chr2,length=5000,IDX=1>\n\
        #CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\tFORMAT\tA\tB\n";

    enum Value {
        Flag,
        Ints(Vec<i32>),
        Floats(Vec<f32>),
        Text(String),
    }

    struct Record {
        contig: i32,
        position: i32,
        length: i32,
        quality: Option<f32>,
        id: &'static str,
        alleles: &'static [&'static str],
        filters: &'static [i32],
        infos: Vec<(i32, Value)>,
        /// The two alleles of each sample: (allele + 1) << 1, 0 for a missing allele, and -127
        /// to end a haploid call.
        calls: [[i8; 2]; 2],
    }

    fn descriptor(out: &mut Vec<u8>, kind: u8, count: usize) {
        if count < 15 {
            out.push((u8::try_from(count).unwrap() << 4) | kind);
        } else {
            out.push(0xF0 | kind);
            out.push(0x13);
            out.extend(i32::try_from(count).unwrap().to_le_bytes());
        }
    }

    fn int(out: &mut Vec<u8>, value: i32) {
        out.push(0x11);
        out.push(i8::try_from(value).unwrap().to_le_bytes()[0]);
    }

    fn text(out: &mut Vec<u8>, value: &str) {
        descriptor(out, 7, value.len());
        out.extend(value.as_bytes());
    }

    fn shared(record: &Record) -> Vec<u8> {
        let mut out = Vec::new();
        for value in [record.contig, record.position, record.length] {
            out.extend(value.to_le_bytes());
        }
        let quality = record.quality.map_or(0x7F80_0001, f32::to_bits);
        out.extend(quality.to_le_bytes());
        let counts = (u32::try_from(record.alleles.len()).unwrap() << 16)
            | u32::try_from(record.infos.len()).unwrap();
        out.extend(counts.to_le_bytes());
        out.extend((1_u32 << 24 | 2).to_le_bytes());
        text(&mut out, record.id);
        for allele in record.alleles {
            text(&mut out, allele);
        }
        descriptor(&mut out, 1, record.filters.len());
        for filter in record.filters {
            out.push(i8::try_from(*filter).unwrap().to_le_bytes()[0]);
        }
        for (key, value) in &record.infos {
            int(&mut out, *key);
            match value {
                Value::Flag => out.push(0x00),
                Value::Ints(values) if values.iter().all(|value| i8::try_from(*value).is_ok()) => {
                    descriptor(&mut out, 1, values.len());
                    for value in values {
                        out.push(i8::try_from(*value).unwrap().to_le_bytes()[0]);
                    }
                }
                Value::Ints(values) => {
                    descriptor(&mut out, 2, values.len());
                    for value in values {
                        out.extend(i16::try_from(*value).unwrap().to_le_bytes());
                    }
                }
                Value::Floats(values) => {
                    descriptor(&mut out, 5, values.len());
                    for value in values {
                        out.extend(value.to_le_bytes());
                    }
                }
                Value::Text(value) => text(&mut out, value),
            }
        }
        out
    }

    fn individual(record: &Record) -> Vec<u8> {
        let mut out = Vec::new();
        int(&mut out, 8);
        descriptor(&mut out, 1, 2);
        for call in record.calls {
            out.extend(call.map(|allele| allele.to_le_bytes()[0]));
        }
        out
    }

    fn records() -> Vec<Record> {
        let csq = "CT|stop_gained|HIGH|GENEA,CT|intron_variant|MODIFIER|GENEB";
        vec![
            Record {
                contig: 0,
                position: 99,
                length: 1,
                quality: Some(50.5),
                id: "rs1",
                alleles: &["A", "G", "T"],
                filters: &[0],
                infos: vec![
                    (1, Value::Ints(vec![10])),
                    (2, Value::Floats(vec![0.5, 0.25])),
                    (3, Value::Flag),
                ],
                calls: [[2, 4], [4, 4]],
            },
            Record {
                contig: 0,
                position: 199,
                length: 1,
                quality: None,
                id: ".",
                alleles: &["C", "CT"],
                filters: &[7],
                infos: vec![
                    (1, Value::Ints(vec![300])),
                    (4, Value::Text(csq.to_owned())),
                ],
                calls: [[0, 0], [2, 3]],
            },
            Record {
                contig: 0,
                position: 499,
                length: 400,
                quality: Some(30.0),
                id: "sv1",
                alleles: &["N", "<DEL>"],
                filters: &[],
                infos: vec![
                    (5, Value::Text("DEL".to_owned())),
                    (6, Value::Ints(vec![900])),
                ],
                calls: [[4, -127], [2, 4]],
            },
            Record {
                contig: 1,
                position: 49,
                length: 1,
                quality: Some(9.0),
                id: "rs4",
                alleles: &["G", "A"],
                filters: &[0],
                infos: Vec::new(),
                calls: [[2, 2], [2, 4]],
            },
        ]
    }

    /// A BCF file of the test records, in two BGZF blocks, and its CSI index.
    pub(crate) fn file() -> (Vec<u8>, Vec<u8>) {
        let mut writer = bgzf::Writer::new(Vec::new());
        writer.write_all(b"BCF\x02\x02").unwrap();
        let text = format!("{HEADER}\0");
        writer
            .write_all(&u32::try_from(text.len()).unwrap().to_le_bytes())
            .unwrap();
        writer.write_all(text.as_bytes()).unwrap();
        writer.end_block().unwrap();
        let start = writer.virtual_offset();
        let scheme = Scheme::reaching(14, 1, 10_256);
        let mut builder = Builder::new(scheme, true, start);
        for (index, record) in records().iter().enumerate() {
            if index == 2 {
                writer.end_block().unwrap();
            }
            let (shared, individual) = (shared(record), individual(record));
            writer
                .write_all(&u32::try_from(shared.len()).unwrap().to_le_bytes())
                .unwrap();
            writer
                .write_all(&u32::try_from(individual.len()).unwrap().to_le_bytes())
                .unwrap();
            writer.write_all(&shared).unwrap();
            writer.write_all(&individual).unwrap();
            let place = (
                usize::try_from(record.contig).unwrap(),
                u64::try_from(record.position).unwrap(),
                u64::try_from(record.position + record.length).unwrap(),
            );
            builder
                .push(Some(place), writer.virtual_offset(), true)
                .unwrap();
        }
        let end = writer.virtual_offset();
        let data = writer.finish().unwrap();
        let (references, unplaced) = builder.finish(end);
        let mut bytes = b"CSI\x01".to_vec();
        bytes.extend(scheme.shift().to_le_bytes());
        bytes.extend(scheme.depth().to_le_bytes());
        put(&mut bytes, 0);
        put(&mut bytes, references.len());
        for bins in &references {
            bins.write(&mut bytes);
        }
        bytes.extend(unplaced.to_le_bytes());
        let mut index = bgzf::Writer::new(Vec::new());
        index.write_all(&bytes).unwrap();
        (data, index.finish().unwrap())
    }

    #[test]
    fn headers_give_the_dictionaries_and_the_samples() {
        let (data, _) = file();
        let found = header(&mut Bgzf::new(Cursor::new(data))).unwrap();
        assert_eq!(found.contigs, ["chr1", "chr2"]);
        assert_eq!(
            (found.string(1), found.string(7), found.string(8)),
            (Some("DP"), Some("q10"), Some("GT"))
        );
        assert_eq!(found.vcf.samples, ["A", "B"]);
        assert!(
            found
                .vcf
                .infos
                .contains(&("DB".to_owned(), "dbSNP, \"build\" 151".to_owned()))
        );
    }

    #[test]
    fn regions_give_their_variants_with_the_index() {
        let (data, index) = file();
        let mut reader = Bgzf::new(Cursor::new(data));
        let found = header(&mut reader).unwrap();
        let index = Index::read(Cursor::new(index))
            .unwrap()
            .named(&found.contigs)
            .unwrap();
        let layouts = annotations::layouts(&found.vcf.infos);
        let near = query(
            &index,
            &mut reader,
            &found,
            ("chr1", Region::between(150, 600)),
            &layouts,
        )
        .unwrap();
        assert_eq!(near.len(), 2);
        let insertion = &near[0];
        assert_eq!(
            (
                insertion.id.as_str(),
                insertion.quality,
                insertion.filter.as_str()
            ),
            ("", None, "q10")
        );
        assert_eq!(
            (
                insertion.reference.as_str(),
                insertion.alternates.as_slice()
            ),
            ("C", &["CT".to_owned()][..])
        );
        assert_eq!(insertion.info, "DP=300");
        assert_eq!(insertion.annotations.first[0].gene, "GENEA");
        assert_eq!(insertion.calls, [Call::Missing, Call::Reference]);
        let deletion = &near[1];
        assert_eq!(deletion.span, Region::between(499, 899));
        assert_eq!(
            deletion.structure.as_ref().map(|found| found.kind),
            Some(Sv::Deletion)
        );
        assert_eq!(
            (deletion.filter.as_str(), deletion.calls.as_slice()),
            (".", &[Call::Alternate, Call::Het][..])
        );
        let other = query(
            &index,
            &mut reader,
            &found,
            ("chr2", Region::between(0, 5_000)),
            &layouts,
        )
        .unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].calls, [Call::Reference, Call::Het]);
        let first = query(
            &index,
            &mut reader,
            &found,
            ("chr1", Region::between(0, 150)),
            &layouts,
        )
        .unwrap();
        assert_eq!(
            (
                first[0].id.as_str(),
                first[0].info.as_str(),
                first[0].quality
            ),
            ("rs1", "DP=10;AF=0.5,0.25;DB", Some(50.5))
        );
        assert_eq!(first[0].alternates, ["G", "T"]);
        assert_eq!(first[0].calls, [Call::Het, Call::Alternate]);
    }

    #[test]
    fn files_without_an_index_read_whole_and_give_their_names() {
        let (data, _) = file();
        let (found, table) = read_table(Bgzf::new(Cursor::new(data.clone()))).unwrap();
        assert_eq!(found.vcf.samples, ["A", "B"]);
        assert_eq!(table.query("chr1", Region::between(0, 10_000)).len(), 3);
        assert_eq!(table.query("chr2", Region::between(0, 5_000))[0].id, "rs4");
        let entries = names(Bgzf::new(Cursor::new(data))).unwrap();
        let found: Vec<(&str, &str, Region)> = entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.contig.as_str(), entry.span))
            .collect();
        assert_eq!(
            found,
            [
                ("rs1", "chr1", Region::between(99, 100)),
                ("sv1", "chr1", Region::between(499, 899)),
                ("rs4", "chr2", Region::between(49, 50)),
            ]
        );
    }

    #[test]
    fn the_fixtures_are_the_test_file_and_its_index() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
        let (data, index) = file();
        if std::env::var_os("GENIE_WRITE_FIXTURES").is_some() {
            std::fs::write(folder.join("calls.bcf"), &data).unwrap();
            std::fs::write(folder.join("calls.bcf.csi"), &index).unwrap();
        }
        assert_eq!(std::fs::read(folder.join("calls.bcf")).unwrap(), data);
        assert_eq!(std::fs::read(folder.join("calls.bcf.csi")).unwrap(), index);
    }

    #[test]
    fn other_files_are_not_bcf() {
        let (mut data, _) = file();
        let mut wrong = Bgzf::new(Cursor::new(b"##fileformat=VCFv4.2\n".to_vec()));
        assert!(matches!(
            header(&mut wrong),
            Err(Error::Invalid(Reason::NotBcf))
        ));
        data.truncate(30);
        assert!(read_table(Bgzf::new(Cursor::new(data))).is_err());
    }
}
