use std::io::{Read, Seek, SeekFrom};

use genie_core::signal::{self, Bin, Summary};
use genie_core::{Feature, Region};
use miniz_oxide::inflate::decompress_to_vec_zlib_with_limit;

use crate::fields::{self, Field};
use crate::{Error, Reason, bed};

const MAGIC: u32 = 0x888F_FC26;
const BED_MAGIC: u32 = 0x8789_F2EB;
const NAME_FIELD: u16 = 3;
const CHROMOSOMES: u32 = 0x78CA_8C91;
const BLOCKS: u32 = 0x2468_ACE0;
const HEADER: usize = 64;
const ZOOM: usize = 24;
const RECORD: usize = 32;
const DEPTH: usize = 32;
const KEY_LIMIT: usize = 256;
const BLOCK_LIMIT: u64 = 1 << 26;
const AUTO_SQL_LIMIT: u64 = 1 << 16;
const LABELS: [&str; 2] = ["geneName2", "name2"];
const SYMBOL_FIELDS: [&str; 3] = ["geneName", "geneName2", "name2"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Zoom {
    reduction: u64,
    index: u64,
}

#[derive(Debug, Clone)]
pub struct Index {
    big: bool,
    bed: bool,
    buffer: u32,
    names: Vec<String>,
    ids: Vec<u32>,
    full: u64,
    zooms: Vec<Zoom>,
    lookup: Option<u64>,
    defined: usize,
    fields: Vec<(String, String)>,
    labels: Vec<usize>,
}

struct Item<'a> {
    chromosome: u32,
    start: u64,
    end: u64,
    rest: &'a str,
}

fn items(block: &[u8], big: bool) -> Result<Vec<Item<'_>>, Error> {
    let mut found = Vec::new();
    let mut bytes = Bytes::new(block, big);
    while bytes.at + 12 <= block.len() {
        let chromosome = bytes.u32()?;
        let start = u64::from(bytes.u32()?);
        let end = u64::from(bytes.u32()?);
        let tail = &block[bytes.at..];
        let length = tail
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(tail.len());
        let rest =
            std::str::from_utf8(&tail[..length]).map_err(|_| Error::Invalid(Reason::BadBlock))?;
        bytes.at += (length + 1).min(tail.len());
        found.push(Item {
            chromosome,
            start,
            end,
            rest,
        });
    }
    Ok(found)
}

fn touches(start: u64, end: u64, region: Region) -> bool {
    (start < region.end() && end > region.start())
        || (start == end && (start == region.end() || end == region.start()))
}

fn line(contig: &str, item: &Item<'_>) -> String {
    let mut line = format!("{contig}\t{}\t{}", item.start, item.end);
    if !item.rest.is_empty() {
        line.push('\t');
        line.push_str(item.rest);
    }
    line
}

fn feature(
    contig: &str,
    item: &Item<'_>,
    defined: usize,
    labels: &[usize],
) -> Result<Feature, Error> {
    let fields: Vec<&str> = if item.rest.is_empty() {
        Vec::new()
    } else {
        item.rest.split('\t').collect()
    };
    let standard = defined.checked_sub(3).unwrap_or(fields.len());
    let mut text = format!("{contig}\t{}\t{}", item.start, item.end);
    for field in fields.iter().take(standard) {
        text.push('\t');
        text.push_str(field);
    }
    let extra = fields.get(standard..).unwrap_or_default().join("\t");
    let feature = bed::parse(&text)
        .map_err(Error::Invalid)?
        .map(|(_, feature)| feature.with_extra(extra))
        .ok_or(Error::Invalid(Reason::BadRecord))?;
    let label = labels
        .iter()
        .filter_map(|&column| fields.get(column - 3))
        .find(|value| !value.is_empty() && **value != "none");
    Ok(match label {
        Some(label) => feature.with_name(*label),
        None => feature,
    })
}

fn auto_sql(reader: &mut (impl Read + Seek), offset: u64) -> Result<String, Error> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    reader.take(AUTO_SQL_LIMIT).read_to_end(&mut bytes)?;
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
}

fn declared(auto_sql: &str) -> Vec<(&str, &str)> {
    auto_sql
        .lines()
        .filter_map(|line| line.split_once(';'))
        .filter_map(|(declaration, about)| Some((declaration.split_whitespace().last()?, about)))
        .collect()
}

fn label_columns(fields: &[(&str, &str)]) -> Vec<usize> {
    let symbols: Vec<usize> = fields
        .iter()
        .enumerate()
        .filter(|(column, (field, about))| {
            *column > 3 && SYMBOL_FIELDS.contains(field) && names_symbol(about)
        })
        .map(|(column, _)| column)
        .collect();
    if !symbols.is_empty() {
        return symbols;
    }
    LABELS
        .iter()
        .filter_map(|wanted| fields.iter().position(|(field, _)| field == wanted))
        .filter(|&column| column > 3)
        .collect()
}

fn names_symbol(about: &str) -> bool {
    about
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case("symbol"))
}

fn depths(spans: impl Iterator<Item = Region>, region: Region) -> Vec<Summary> {
    let mut events: Vec<(u64, i64)> = Vec::new();
    for span in spans.filter_map(|span| span.intersection(region)) {
        events.push((span.start(), 1));
        events.push((span.end(), -1));
    }
    events.sort_unstable();
    let mut found = Vec::new();
    let mut depth = 0;
    let mut last = region.start();
    for (position, change) in events {
        if depth > 0 && position > last {
            found.push(Summary::single(
                Region::between(last, position),
                depth as f32,
            ));
        }
        depth += change;
        last = position;
    }
    found
}

fn name_index(
    reader: &mut (impl Read + Seek),
    extension: u64,
    big: bool,
) -> Result<Option<u64>, Error> {
    let head = chunk(reader, extension, 12)?;
    let mut bytes = Bytes::new(&head, big);
    let _size = bytes.u16()?;
    let count = bytes.u16()?;
    let mut at = bytes.u64()?;
    for _ in 0..count {
        let entry = chunk(reader, at, 16)?;
        let mut bytes = Bytes::new(&entry, big);
        let _kind = bytes.u16()?;
        let fields = usize::from(bytes.u16()?);
        let offset = bytes.u64()?;
        let list = chunk(reader, at + 16, fields * 4)?;
        let first = Bytes::new(&list, big).u16().ok();
        if fields == 1 && first == Some(NAME_FIELD) {
            return Ok(Some(offset));
        }
        at += 16 + fields as u64 * 4;
    }
    Ok(None)
}

fn same(found: &[u8], name: &[u8], fold: bool) -> bool {
    if fold {
        found.eq_ignore_ascii_case(name)
    } else {
        found == name
    }
}

fn tree_find(
    reader: &mut (impl Read + Seek),
    offset: u64,
    big: bool,
    key: &[u8],
    fold: bool,
) -> Result<Vec<(u64, u64)>, Error> {
    let head = chunk(reader, offset, 32)?;
    let mut bytes = Bytes::new(&head, big);
    let bad = || Error::Invalid(Reason::BadIndex);
    if bytes.u32()? != CHROMOSOMES {
        return Err(bad());
    }
    let _block = bytes.u32()?;
    let size = bytes.u32()? as usize;
    if size > KEY_LIMIT || bytes.u32()? != 16 {
        return Err(bad());
    }
    if key.len() > size {
        return Ok(Vec::new());
    }
    let padded = |mut bound: Vec<u8>| {
        bound.resize(size, 0);
        bound
    };
    let (low, high) = if fold {
        (key.to_ascii_uppercase(), key.to_ascii_lowercase())
    } else {
        (key.to_vec(), key.to_vec())
    };
    let (low, high) = (padded(low), padded(high));
    let mut found = Vec::new();
    let mut nodes = vec![(offset + 32, 0)];
    while let Some((at, depth)) = nodes.pop() {
        if depth > DEPTH {
            return Err(bad());
        }
        let (leaf, count, items) = node(reader, at, big, |leaf| size + if leaf { 16 } else { 8 })?;
        let mut bytes = Bytes::new(&items, big);
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let name = bytes.skip(size)?;
            let first = bytes.u64()?;
            let second = if leaf { bytes.u64()? } else { 0 };
            entries.push((name, first, second));
        }
        for (place, (name, first, second)) in entries.iter().enumerate() {
            if leaf {
                let end = name
                    .iter()
                    .rposition(|byte| *byte != 0)
                    .map_or(0, |at| at + 1);
                if same(&name[..end], key, fold) {
                    found.push((*first, *second));
                }
                continue;
            }
            let next = entries.get(place + 1).map(|(next, ..)| *next);
            if *name <= high.as_slice() && next.is_none_or(|next| low.as_slice() <= next) {
                nodes.push((*first, depth + 1));
            }
        }
    }
    Ok(found)
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

fn records(block: &[u8], big: bool, id: u32, region: Region) -> Result<Vec<Summary>, Error> {
    let mut bytes = Bytes::new(block, big);
    let mut found = Vec::new();
    for _ in 0..block.len() / RECORD {
        let chromosome = bytes.u32()?;
        let start = u64::from(bytes.u32()?);
        let end = u64::from(bytes.u32()?);
        let count = bytes.u32()?;
        let min = bytes.f32()?;
        let max = bytes.f32()?;
        let sum = f64::from(bytes.f32()?);
        let _squares = bytes.f32()?;
        let Some(span) = Region::new(start, end) else {
            continue;
        };
        if chromosome == id && span.intersection(region).is_some() {
            found.push(Summary {
                span,
                count,
                min,
                max,
                sum,
            });
        }
    }
    Ok(found)
}

impl Index {
    pub fn read(reader: &mut (impl Read + Seek)) -> Result<Self, Error> {
        let header = chunk(reader, 0, HEADER)?;
        let magic = [header[0], header[1], header[2], header[3]];
        let (little, swapped) = (u32::from_le_bytes(magic), u32::from_be_bytes(magic));
        let (big, bed) = if little == MAGIC || little == BED_MAGIC {
            (false, little == BED_MAGIC)
        } else if swapped == MAGIC || swapped == BED_MAGIC {
            (true, swapped == BED_MAGIC)
        } else {
            return Err(Error::Invalid(Reason::NotBigWig));
        };
        let mut bytes = Bytes::new(&header, big);
        let _magic = bytes.u32()?;
        let _version = bytes.u16()?;
        let levels = usize::from(bytes.u16()?);
        let tree = bytes.u64()?;
        let _data = bytes.u64()?;
        let full = bytes.u64()?;
        let _fields = bytes.u16()?;
        let defined = usize::from(bytes.u16()?);
        let schema = bytes.u64()?;
        let _total = bytes.u64()?;
        let buffer = bytes.u32()?;
        let extension = bytes.u64()?;
        let table = chunk(reader, HEADER as u64, levels * ZOOM)?;
        let mut bytes = Bytes::new(&table, big);
        let mut zooms = Vec::with_capacity(levels);
        for _ in 0..levels {
            let reduction = u64::from(bytes.u32()?);
            let _reserved = bytes.u32()?;
            let _data = bytes.u64()?;
            let index = bytes.u64()?;
            zooms.push(Zoom { reduction, index });
        }
        let (names, ids) = chromosomes(reader, tree, big)?.into_iter().unzip();
        let lookup = if bed && extension != 0 {
            name_index(reader, extension, big)?
        } else {
            None
        };
        let text = if bed && schema != 0 {
            auto_sql(reader, schema)?
        } else {
            String::new()
        };
        let declared = declared(&text);
        let fields = declared
            .iter()
            .map(|(field, about)| ((*field).to_owned(), (*about).to_owned()))
            .collect();
        let labels = label_columns(&declared);
        Ok(Self {
            big,
            bed,
            buffer,
            names,
            ids,
            full,
            zooms,
            lookup,
            defined,
            fields,
            labels,
        })
    }

    pub fn is_bed(&self) -> bool {
        self.bed
    }

    pub fn has_lookup(&self) -> bool {
        self.lookup.is_some()
    }

    pub fn fields(&self, extra: &str) -> Vec<Field> {
        if extra.is_empty() {
            return Vec::new();
        }
        let mut found = Vec::new();
        for (place, value) in extra.split('\t').enumerate() {
            let column = self.defined + place;
            let (name, about) = self
                .fields
                .get(column)
                .map_or(("", ""), |(name, about)| (name.as_str(), about.as_str()));
            if name.starts_with('_') || !fields::shown(value) {
                continue;
            }
            let label = if name.is_empty() {
                format!("Column {}", column + 1)
            } else {
                fields::label(about, name)
            };
            found.push(Field {
                label,
                value: fields::plain(fields::linked(value)),
            });
        }
        found
    }

    pub fn label_with(&mut self, field: &str) {
        let column = self.fields.iter().position(|(name, _)| name == field);
        if let Some(column) = column.filter(|&column| column > 3) {
            self.labels = vec![column];
        }
    }

    fn contig(&self, id: u32) -> Option<&str> {
        let slot = self.ids.iter().position(|known| *known == id)?;
        Some(&self.names[slot])
    }

    pub fn features(
        &self,
        reader: &mut (impl Read + Seek),
        contig: &str,
        region: Region,
    ) -> Result<Vec<Feature>, Error> {
        let Some(id) = self.id(contig) else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for (offset, size) in blocks(reader, self.full, self.big, id, region)? {
            let block = self.block(reader, offset, size)?;
            for item in items(&block, self.big)? {
                if item.chromosome == id && touches(item.start, item.end, region) {
                    found.push(feature(contig, &item, self.defined, &self.labels)?);
                }
            }
        }
        Ok(found)
    }

    pub fn find(
        &self,
        reader: &mut (impl Read + Seek),
        name: &str,
    ) -> Result<Vec<(String, Feature)>, Error> {
        let exact = self.matching(reader, name, false)?;
        if exact.is_empty() && name.bytes().any(|byte| byte.is_ascii_alphabetic()) {
            return self.matching(reader, name, true);
        }
        Ok(exact)
    }

    fn matching(
        &self,
        reader: &mut (impl Read + Seek),
        name: &str,
        fold: bool,
    ) -> Result<Vec<(String, Feature)>, Error> {
        let Some(tree) = self.lookup else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for (offset, size) in tree_find(reader, tree, self.big, name.as_bytes(), fold)? {
            let block = self.block(reader, offset, size)?;
            for item in items(&block, self.big)? {
                let first = item.rest.split('\t').next().unwrap_or_default();
                if !same(first.as_bytes(), name.as_bytes(), fold) {
                    continue;
                }
                if let Some(contig) = self.contig(item.chromosome) {
                    let feature = feature(contig, &item, self.defined, &self.labels)?;
                    found.push((contig.to_owned(), feature));
                }
            }
        }
        Ok(found)
    }

    fn zoom(&self, desired: u64) -> Option<Zoom> {
        if desired <= 1 {
            return None;
        }
        self.zooms
            .iter()
            .filter(|zoom| zoom.reduction <= desired)
            .max_by_key(|zoom| zoom.reduction)
            .copied()
    }

    pub fn summaries(
        &self,
        reader: &mut (impl Read + Seek),
        contig: &str,
        region: Region,
        desired: u64,
    ) -> Result<Vec<Summary>, Error> {
        let Some(zoom) = self.zoom(desired) else {
            if self.bed {
                let features = self.features(reader, contig, region)?;
                return Ok(depths(features.iter().map(Feature::span), region));
            }
            return self.records(reader, contig, region);
        };
        let Some(id) = self.id(contig) else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for (offset, size) in blocks(reader, zoom.index, self.big, id, region)? {
            let block = self.block(reader, offset, size)?;
            found.extend(records(&block, self.big, id, region)?);
        }
        Ok(found)
    }

    pub fn bins(
        &self,
        reader: &mut (impl Read + Seek),
        contig: &str,
        region: Region,
        count: usize,
    ) -> Result<Vec<Option<Bin>>, Error> {
        if count == 0 || region.is_empty() {
            return Ok(vec![None; count]);
        }
        let desired = region.len() / count as u64 / 2;
        let summaries = self.summaries(reader, contig, region, desired)?;
        Ok(signal::bins(&summaries, region, count))
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn lines(
        &self,
        reader: &mut (impl Read + Seek),
        contig: &str,
    ) -> Result<Vec<String>, Error> {
        let Some(id) = self.id(contig) else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for (offset, size) in blocks(reader, self.full, self.big, id, Region::between(0, 1 << 32))?
        {
            let block = self.block(reader, offset, size)?;
            for item in items(&block, self.big)? {
                if item.chromosome == id {
                    found.push(line(contig, &item));
                }
            }
        }
        Ok(found)
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
