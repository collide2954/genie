//! CRAM records: the data series of a slice, mate links inside it, and the read rebuilt from
//! the reference and the read features.

use std::collections::HashMap;

use genie_core::Region;
use genie_core::alignment::{Alignment, Mate, Op, REVERSE};

use super::bits::{Bits, Bytes, bad};
use super::encoding::Streams;
use super::{Block, CORE, Compression, Cram, EXTERNAL, Fetch, Slice};
use crate::{Error, Reason, bam, md5};

const QUALITIES: i32 = 0x1;
const DETACHED: i32 = 0x2;
const DOWNSTREAM: i32 = 0x4;
const NO_SEQUENCE: i32 = 0x8;
const MATE_REVERSE: i32 = 0x1;
const MATE_UNMAPPED: i32 = 0x2;
const PAIRED: u16 = 0x1;
const UNMAPPED: u16 = 0x4;
const FLAG_MATE_UNMAPPED: u16 = 0x8;
const FLAG_MATE_REVERSE: u16 = 0x20;
const MULTIPLE: i32 = -2;

#[derive(Debug, Clone)]
enum Feature {
    Base(u8),
    Substitution(u8),
    Insertion(Vec<u8>),
    InsertedBase(u8),
    SoftClip(Vec<u8>),
    HardClip(u32),
    Padding(u32),
    Deletion(u32),
    Skip(u32),
    Bases(Vec<u8>),
    Quality,
}

#[derive(Debug, Clone)]
struct Raw {
    flags: u16,
    cf: i32,
    reference: i32,
    length: usize,
    position: i64,
    group: i32,
    name: Vec<u8>,
    mate_line: Option<usize>,
    mate_flags: i32,
    mate_reference: i32,
    mate_position: i64,
    tlen: Option<i64>,
    aux: Vec<u8>,
    features: Vec<(usize, Feature)>,
    mapq: u8,
    qualities: Vec<u8>,
    bases: Vec<u8>,
    end: i64,
}

impl Raw {
    fn mapped(&self) -> bool {
        self.flags & UNMAPPED == 0
    }
}

fn size(value: i32) -> Result<usize, Error> {
    usize::try_from(value).map_err(|_| bad())
}

fn length(value: i32) -> Result<u32, Error> {
    u32::try_from(value).map_err(|_| bad())
}

struct Series<'h, 'd> {
    header: &'h Compression,
    streams: Streams<'d>,
}

impl Series<'_, '_> {
    fn int(&mut self, key: [u8; 2]) -> Result<i32, Error> {
        let encoding = self.header.series.get(&key).ok_or_else(bad)?;
        encoding.int(&mut self.streams)
    }

    fn byte(&mut self, key: [u8; 2]) -> Result<u8, Error> {
        let encoding = self.header.series.get(&key).ok_or_else(bad)?;
        encoding.byte(&mut self.streams)
    }

    fn bytes(&mut self, key: [u8; 2]) -> Result<Vec<u8>, Error> {
        let encoding = self.header.series.get(&key).ok_or_else(bad)?;
        encoding.bytes(&mut self.streams)
    }

    fn run(&mut self, key: [u8; 2], count: usize) -> Result<Vec<u8>, Error> {
        (0..count).map(|_| self.byte(key)).collect()
    }
}

fn signed(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn feature(series: &mut Series, code: u8) -> Result<Feature, Error> {
    Ok(match code {
        b'B' => {
            let base = series.byte(*b"BA")?;
            series.byte(*b"QS")?;
            Feature::Base(base)
        }
        b'X' => Feature::Substitution(series.byte(*b"BS")?),
        b'I' => Feature::Insertion(series.bytes(*b"IN")?),
        b'i' => Feature::InsertedBase(series.byte(*b"BA")?),
        b'S' => Feature::SoftClip(series.bytes(*b"SC")?),
        b'H' => Feature::HardClip(length(series.int(*b"HC")?)?),
        b'P' => Feature::Padding(length(series.int(*b"PD")?)?),
        b'D' => Feature::Deletion(length(series.int(*b"DL")?)?),
        b'N' => Feature::Skip(length(series.int(*b"RS")?)?),
        b'b' => Feature::Bases(series.bytes(*b"BB")?),
        b'q' => {
            series.bytes(*b"QQ")?;
            Feature::Quality
        }
        b'Q' => {
            series.byte(*b"QS")?;
            Feature::Quality
        }
        _ => return Err(bad()),
    })
}

/// The reference bases that a read covers, from its features.
fn reference_span(length: usize, features: &[(usize, Feature)]) -> i64 {
    let (mut read, mut span) = (0_usize, 0_i64);
    for (at, feature) in features {
        let before = at.saturating_sub(1);
        if before > read {
            span += signed(before - read);
            read = before;
        }
        match feature {
            Feature::Base(_) | Feature::Substitution(_) => {
                read += 1;
                span += 1;
            }
            Feature::Bases(bases) => {
                read += bases.len();
                span += signed(bases.len());
            }
            Feature::Insertion(bases) | Feature::SoftClip(bases) => read += bases.len(),
            Feature::InsertedBase(_) => read += 1,
            Feature::Deletion(count) | Feature::Skip(count) => span += i64::from(*count),
            Feature::HardClip(_) | Feature::Padding(_) | Feature::Quality => {}
        }
    }
    if length > read {
        span += signed(length - read);
    }
    span
}

fn tag_value(key: [u8; 3], mut value: Vec<u8>) -> Vec<u8> {
    let mut entry = key.to_vec();
    if matches!(key[2], b'Z' | b'H') && value.last() != Some(&0) {
        value.push(0);
    }
    entry.extend(value);
    entry
}

fn raw_records(header: &Compression, slice: &Slice, blocks: &[Block]) -> Result<Vec<Raw>, Error> {
    let core = blocks
        .iter()
        .find(|block| block.content == CORE)
        .map_or(&[][..], |block| block.data.as_slice());
    let external: HashMap<i32, Bytes> = blocks
        .iter()
        .filter(|block| block.content == EXTERNAL)
        .map(|block| (block.id, Bytes::new(&block.data)))
        .collect();
    let mut series = Series {
        header,
        streams: Streams {
            core: Bits::new(core),
            external,
        },
    };
    let mut records = Vec::with_capacity(slice.records);
    let mut last = slice.start;
    for index in 0..slice.records {
        let flags = u16::try_from(series.int(*b"BF")?).map_err(|_| bad())?;
        let cf = series.int(*b"CF")?;
        let reference = if slice.reference == MULTIPLE {
            series.int(*b"RI")?
        } else {
            slice.reference
        };
        let length = size(series.int(*b"RL")?)?;
        let step = i64::from(series.int(*b"AP")?);
        let position = if header.delta {
            last += step;
            last
        } else {
            step
        };
        let group = series.int(*b"RG")?;
        let mut name = if header.names {
            series.bytes(*b"RN")?
        } else {
            Vec::new()
        };
        let mut record = Raw {
            flags,
            cf,
            reference,
            length,
            position,
            group,
            name: Vec::new(),
            mate_line: None,
            mate_flags: 0,
            mate_reference: -1,
            mate_position: 0,
            tlen: Some(0),
            aux: Vec::new(),
            features: Vec::new(),
            mapq: 0,
            qualities: Vec::new(),
            bases: Vec::new(),
            end: position,
        };
        if cf & DETACHED != 0 {
            record.mate_flags = series.int(*b"MF")?;
            if !header.names {
                name = series.bytes(*b"RN")?;
            }
            record.mate_reference = series.int(*b"NS")?;
            record.mate_position = i64::from(series.int(*b"NP")?);
            record.tlen = Some(i64::from(series.int(*b"TS")?));
        } else if cf & DOWNSTREAM != 0 {
            record.mate_line = Some(index + 1 + size(series.int(*b"NF")?)?);
            record.tlen = None;
        }
        record.name = name;
        tags_and_bases(&mut series, &mut record)?;
        records.push(record);
    }
    Ok(records)
}

/// The tags of a record, then its read features, mapping quality and qualities when it is
/// mapped, or its bases and qualities when it is not.
fn tags_and_bases(series: &mut Series, record: &mut Raw) -> Result<(), Error> {
    let header = series.header;
    let line = size(series.int(*b"TL")?)?;
    for key in header.dictionary.get(line).ok_or_else(bad)? {
        let id = (i32::from(key[0]) << 16) | (i32::from(key[1]) << 8) | i32::from(key[2]);
        let encoding = header.tags.get(&id).ok_or_else(bad)?;
        let value = encoding.bytes(&mut series.streams)?;
        record.aux.extend(tag_value(*key, value));
    }
    let (cf, length) = (record.cf, record.length);
    if record.mapped() {
        let mut at = 0;
        for _ in 0..series.int(*b"FN")? {
            let kind = series.byte(*b"FC")?;
            at += size(series.int(*b"FP")?)?;
            record.features.push((at, feature(series, kind)?));
        }
        record.mapq = u8::try_from(series.int(*b"MQ")?).unwrap_or(u8::MAX);
        if cf & QUALITIES != 0 {
            record.qualities = series.run(*b"QS", length)?;
        }
        let span = reference_span(length, &record.features);
        record.end = record.position + span.max(1) - 1;
    } else {
        if cf & NO_SEQUENCE == 0 {
            record.bases = series.run(*b"BA", length)?;
        }
        if cf & QUALITIES != 0 {
            record.qualities = series.run(*b"QS", length)?;
        }
    }
    Ok(())
}

/// Mate positions and template lengths for reads whose mate comes later in the slice, as
/// htslib makes them: the leftmost read of a pair on one reference gets +TLEN, the others -TLEN.
fn link_mates(records: &mut [Raw]) -> Result<(), Error> {
    let count = records.len();
    for index in 0..count {
        if let Some(line) = records[index].mate_line {
            let mate = records.get(line).ok_or_else(bad)?.clone();
            let this = records[index].clone();
            let (first, second) = (&mut records[index], mate);
            first.mate_position = second.position;
            first.mate_reference = second.reference;
            if second.flags & UNMAPPED != 0 {
                first.flags |= FLAG_MATE_UNMAPPED;
            }
            if second.flags & REVERSE != 0 {
                first.flags |= FLAG_MATE_REVERSE;
            }
            let other = &mut records[line];
            other.mate_position = this.position;
            other.mate_reference = this.reference;
            if this.flags & UNMAPPED != 0 {
                other.flags |= FLAG_MATE_UNMAPPED;
            }
            if this.flags & REVERSE != 0 {
                other.flags |= FLAG_MATE_REVERSE;
            }
        } else {
            let record = &mut records[index];
            if record.mate_flags & MATE_REVERSE != 0 {
                record.flags |= PAIRED | FLAG_MATE_REVERSE;
            }
            if record.mate_flags & MATE_UNMAPPED != 0 {
                record.flags |= FLAG_MATE_UNMAPPED;
            }
            if record.flags & PAIRED == 0 {
                record.mate_reference = -1;
            }
        }
        if records[index].tlen.is_some() {
            continue;
        }
        let (mut left, mut right) = (records[index].position, records[index].end);
        let mut reference = records[index].reference;
        let mut chain = vec![index];
        let mut at = index;
        loop {
            left = left.min(records[at].position);
            right = right.max(records[at].end);
            let Some(next) = records[at].mate_line else {
                records[at].mate_line = Some(index);
                break;
            };
            if next <= at || next >= count {
                return Err(bad());
            }
            at = next;
            if records[at].reference != reference {
                reference = -1;
            }
            if at == index {
                break;
            }
            chain.push(at);
        }
        let tlen = if reference == -1 { 0 } else { right - left + 1 };
        let mut signed = false;
        for member in chain {
            let leftmost = !signed && records[member].position == left;
            signed |= leftmost;
            records[member].tlen = Some(if reference == -1 {
                0
            } else if leftmost {
                tlen
            } else {
                -tlen
            });
        }
    }
    Ok(())
}

fn row(base: u8) -> usize {
    match base {
        b'A' => 0,
        b'C' => 1,
        b'G' => 2,
        b'T' => 3,
        _ => 4,
    }
}

fn push(cigar: &mut Vec<(Op, u32)>, op: Op, count: u32) {
    if count == 0 {
        return;
    }
    match cigar.last_mut() {
        Some((last, total)) if *last == op => *total += count,
        _ => cigar.push((op, count)),
    }
}

/// Bases of one contig from `start` on, upper case; positions outside give N.
struct Bases {
    start: i64,
    bases: Vec<u8>,
}

impl Bases {
    fn at(&self, position: i64) -> u8 {
        usize::try_from(position - self.start)
            .ok()
            .and_then(|index| self.bases.get(index))
            .copied()
            .unwrap_or(b'N')
    }
}

fn rebuild(raw: &Raw, reference: &Bases, table: &[[u8; 4]; 5]) -> (Vec<(Op, u32)>, Vec<u8>) {
    let mut cigar = Vec::new();
    let mut sequence = Vec::with_capacity(raw.length);
    let mut at = raw.position - 1;
    let mut read = 0;
    let matches =
        |count: usize, at: &mut i64, sequence: &mut Vec<u8>, cigar: &mut Vec<(Op, u32)>| {
            for offset in 0..count {
                sequence.push(reference.at(*at + signed(offset)));
            }
            *at += signed(count);
            push(cigar, Op::Match, count as u32);
        };
    for (position, feature) in &raw.features {
        let before = position.saturating_sub(1);
        if before > read {
            matches(before - read, &mut at, &mut sequence, &mut cigar);
            read = before;
        }
        match feature {
            Feature::Substitution(code) => {
                let base = reference.at(at);
                sequence.push(table[row(base)][usize::from(code & 3)]);
                at += 1;
                read += 1;
                push(&mut cigar, Op::Match, 1);
            }
            Feature::Base(base) => {
                sequence.push(*base);
                at += 1;
                read += 1;
                push(&mut cigar, Op::Match, 1);
            }
            Feature::Bases(bases) => {
                sequence.extend(bases);
                at += signed(bases.len());
                read += bases.len();
                push(&mut cigar, Op::Match, bases.len() as u32);
            }
            Feature::Insertion(bases) => {
                sequence.extend(bases);
                read += bases.len();
                push(&mut cigar, Op::Insertion, bases.len() as u32);
            }
            Feature::InsertedBase(base) => {
                sequence.push(*base);
                read += 1;
                push(&mut cigar, Op::Insertion, 1);
            }
            Feature::SoftClip(bases) => {
                sequence.extend(bases);
                read += bases.len();
                push(&mut cigar, Op::SoftClip, bases.len() as u32);
            }
            Feature::HardClip(count) => push(&mut cigar, Op::HardClip, *count),
            Feature::Padding(count) => push(&mut cigar, Op::Padding, *count),
            Feature::Deletion(count) => {
                at += i64::from(*count);
                push(&mut cigar, Op::Deletion, *count);
            }
            Feature::Skip(count) => {
                at += i64::from(*count);
                push(&mut cigar, Op::Skip, *count);
            }
            Feature::Quality => {}
        }
    }
    if raw.length > read {
        matches(raw.length - read, &mut at, &mut sequence, &mut cigar);
    }
    (cigar, sequence)
}

fn upper(mut bases: Vec<u8>) -> Vec<u8> {
    bases.make_ascii_uppercase();
    bases
}

/// The reference bases of each contig that the built reads need.
fn references(
    cram: &Cram,
    header: &Compression,
    slice: &Slice,
    blocks: &[Block],
    wanted: &[&Raw],
    fetch: &mut Fetch,
) -> Result<HashMap<i32, Bases>, Error> {
    let mut found = HashMap::new();
    if slice.embedded >= 0 && slice.reference >= 0 {
        let block = blocks
            .iter()
            .find(|block| block.content == EXTERNAL && block.id == slice.embedded)
            .ok_or_else(bad)?;
        let bases = Bases {
            start: slice.start - 1,
            bases: upper(block.data.clone()),
        };
        found.insert(slice.reference, bases);
        return Ok(found);
    }
    if !header.reference {
        return Ok(found);
    }
    let mut spans: HashMap<i32, (i64, i64)> = HashMap::new();
    for raw in wanted
        .iter()
        .filter(|raw| raw.mapped() && raw.reference >= 0)
    {
        let span = spans.entry(raw.reference).or_insert((i64::MAX, i64::MIN));
        span.0 = span.0.min(raw.position - 1);
        span.1 = span.1.max(raw.end);
    }
    if slice.reference >= 0
        && let Some(span) = spans.get_mut(&slice.reference)
    {
        *span = (slice.start - 1, slice.start - 1 + slice.span);
    }
    for (reference, (start, end)) in spans {
        let id = usize::try_from(reference).map_err(|_| bad())?;
        let length = cram.references.get(id).map_or(0, |(_, length)| *length);
        let start = u64::try_from(start.max(0)).unwrap_or(0).min(length);
        let end = u64::try_from(end.max(0))
            .unwrap_or(0)
            .min(length)
            .max(start);
        let bases = upper(fetch(id, Region::between(start, end)).map_err(Error::Remote)?);
        let checked = reference == slice.reference && slice.md5 != [0; 16];
        if checked && md5::digest(&bases) != slice.md5 {
            return Err(Error::Invalid(Reason::CramReference));
        }
        found.insert(
            reference,
            Bases {
                start: start.cast_signed(),
                bases,
            },
        );
    }
    Ok(found)
}

/// For a slice with several references: each run of reads of one reference, as (reference,
/// 1-based start, span), and (-1, 0, 1) for unmapped reads.
pub(crate) fn spans(
    header: &Compression,
    slice: &Slice,
    blocks: &[Block],
) -> Result<Vec<(i32, i64, i64)>, Error> {
    let records = raw_records(header, slice, blocks)?;
    let mut runs: Vec<(i32, i64, i64)> = Vec::new();
    for raw in &records {
        let (start, end) = if raw.reference < 0 {
            (0, 0)
        } else {
            (raw.position, raw.end)
        };
        match runs.last_mut() {
            Some((reference, low, high)) if *reference == raw.reference => {
                *low = (*low).min(start);
                *high = (*high).max(end);
            }
            _ => runs.push((raw.reference, start, end)),
        }
    }
    Ok(runs
        .into_iter()
        .map(|(reference, start, end)| {
            if reference < 0 {
                (-1, 0, 1)
            } else {
                (reference, start, end - start + 1)
            }
        })
        .collect())
}

/// The reads of a slice with the numbers of their references; with `only`, just the reads of
/// that reference are built, so only its bases are needed.
pub(crate) fn decode(
    cram: &Cram,
    header: &Compression,
    slice: &Slice,
    blocks: &[Block],
    only: Option<i32>,
    fetch: &mut Fetch,
) -> Result<Vec<(i32, Alignment)>, Error> {
    let mut records = raw_records(header, slice, blocks)?;
    link_mates(&mut records)?;
    let wanted: Vec<&Raw> = records
        .iter()
        .filter(|raw| only.is_none_or(|id| raw.reference == id))
        .collect();
    let bases = references(cram, header, slice, blocks, &wanted, fetch)?;
    let empty = Bases {
        start: 0,
        bases: Vec::new(),
    };
    let mut reads = Vec::with_capacity(wanted.len());
    for raw in wanted {
        let (cigar, mut sequence) = if raw.mapped() {
            rebuild(
                raw,
                bases.get(&raw.reference).unwrap_or(&empty),
                &header.substitutions,
            )
        } else {
            (Vec::new(), raw.bases.clone())
        };
        if raw.cf & NO_SEQUENCE != 0 {
            sequence.clear();
        }
        let mut aux = raw.aux.clone();
        if let Some(group) = usize::try_from(raw.group)
            .ok()
            .and_then(|index| cram.groups.get(index))
        {
            aux.extend(*b"RGZ");
            aux.extend(group.as_bytes());
            aux.push(0);
        }
        let clipped = cigar.iter().any(|(op, _)| *op == Op::HardClip);
        let tags = bam::tags(&aux, &sequence, raw.flags & REVERSE != 0, clipped);
        let mate = (raw.mate_reference >= 0 && raw.mate_position >= 1).then(|| Mate {
            contig: raw.mate_reference as usize,
            start: (raw.mate_position - 1) as u64,
            insert: raw.tlen.unwrap_or(0),
        });
        let alignment = Alignment {
            name: String::from_utf8_lossy(&raw.name).into_owned(),
            start: u64::try_from(raw.position - 1).unwrap_or(0),
            flags: raw.flags,
            mapq: raw.mapq,
            cigar,
            bases: sequence,
            qualities: if raw.cf & QUALITIES != 0 && raw.qualities.first() != Some(&0xFF) {
                raw.qualities.clone()
            } else {
                Vec::new()
            },
            mate,
            tags,
        };
        reads.push((raw.reference, alignment));
    }
    Ok(reads)
}
