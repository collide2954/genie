//! Hi-C contact maps in the `.hic` format of Juicer, versions 6 to 9: the contacts between the
//! bins of a chromosome, at the resolutions and with the normalizations of the file.

use std::collections::{BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

use miniz_oxide::inflate::decompress_to_vec_zlib_with_limit;

use crate::{Error, Reason};

const MAGIC: [u8; 4] = *b"HIC\0";
const OLDEST: i32 = 6;
const NEWEST: i32 = 9;
const TEXT_LIMIT: u64 = 1 << 20;
const COUNT_LIMIT: u64 = 1 << 24;
const VALUES_LIMIT: u64 = 1 << 28;
const BLOCK_LIMIT: usize = 1 << 26;
const BUFFER: usize = 1 << 16;
/// The unit of the resolutions in bases.
const BP: &str = "BP";
/// The normalization that gives the counts as they are.
pub const NONE: &str = "NONE";

/// A contact between two bins of a chromosome, with `x <= y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    pub x: u32,
    pub y: u32,
    pub count: f32,
}

/// Where the values of a normalization vector are.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Vector {
    kind: String,
    chromosome: i32,
    unit: String,
    bin: i64,
    at: u64,
}

/// The blocks of the contacts of a chromosome with itself at one resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub bin: u32,
    block_bins: u64,
    block_columns: u64,
    blocks: HashMap<u64, (u64, u64)>,
    /// Version 9 indexes the blocks by their place along the diagonal and their distance from it.
    diagonal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hic {
    version: i32,
    /// The chromosomes and their lengths. Juicer puts "All" first.
    pub chromosomes: Vec<(String, u64)>,
    /// The bin sizes in bases, as the file lists them.
    pub resolutions: Vec<u32>,
    master: HashMap<String, u64>,
    /// Where the expected values start in the footer.
    sections: u64,
    vectors: Vec<Vector>,
}

impl Hic {
    pub fn read(reader: &mut (impl Read + Seek)) -> Result<Self, Error> {
        let mut input = Input::new(reader);
        if input.bytes::<4>()? != MAGIC {
            return Err(Error::Invalid(Reason::NotHic));
        }
        let version = input.i32()?;
        if !(OLDEST..=NEWEST).contains(&version) {
            return Err(Error::Invalid(Reason::HicVersion));
        }
        let wide = version > 8;
        let master_at = input.offset()?;
        input.text()?;
        let index_at = if wide {
            let at = input.offset()?;
            input.i64()?;
            Some(at)
        } else {
            None
        };
        for _ in 0..input.count()? {
            input.text()?;
            input.text()?;
        }
        let mut chromosomes = Vec::new();
        for _ in 0..input.count()? {
            let name = input.text()?;
            let length = if wide {
                input.offset()?
            } else {
                u64::try_from(input.i32()?).map_err(|_| invalid())?
            };
            chromosomes.push((name, length));
        }
        let mut resolutions = Vec::new();
        for _ in 0..input.count()? {
            resolutions.push(u32::try_from(input.i32()?).map_err(|_| invalid())?);
        }
        input.seek(master_at)?;
        input.wide_count(wide)?;
        let mut master = HashMap::new();
        for _ in 0..input.count()? {
            let key = input.text()?;
            let at = input.offset()?;
            input.i32()?;
            master.insert(key, at);
        }
        let sections = input.position()?;
        let vectors_at = match index_at {
            Some(at) if at > 0 => Some(at),
            Some(_) => None,
            None => Some(walk(&mut input, wide, None)?.1),
        };
        let mut vectors = Vec::new();
        if let Some(at) = vectors_at {
            input.seek(at)?;
            for _ in 0..input.count()? {
                let kind = input.text()?;
                let chromosome = input.i32()?;
                let unit = input.text()?;
                let bin = i64::from(input.i32()?);
                let at = input.offset()?;
                input.wide_count(wide)?;
                vectors.push(Vector {
                    kind,
                    chromosome,
                    unit,
                    bin,
                    at,
                });
            }
        }
        Ok(Self {
            version,
            chromosomes,
            resolutions,
            master,
            sections,
            vectors,
        })
    }

    /// The normalizations of the file: NONE first, then the others in the order of the file.
    pub fn normalizations(&self) -> Vec<String> {
        let mut found = vec![NONE.to_owned()];
        for vector in &self.vectors {
            if !found.contains(&vector.kind) {
                found.push(vector.kind.clone());
            }
        }
        found
    }

    /// The blocks of a chromosome (its place in `chromosomes`) at a bin size, if the file has
    /// contacts of the chromosome at that size.
    pub fn matrix(
        &self,
        reader: &mut (impl Read + Seek),
        chromosome: usize,
        bin: u32,
    ) -> Result<Option<Matrix>, Error> {
        let Some(&at) = self.master.get(&format!("{chromosome}_{chromosome}")) else {
            return Ok(None);
        };
        let mut input = Input::new(reader);
        input.seek(at)?;
        input.i32()?;
        input.i32()?;
        for _ in 0..input.count()? {
            let unit = input.text()?;
            input.i32()?;
            for _ in 0..4 {
                input.f32()?;
            }
            let size = input.i32()?;
            let block_bins = u64::try_from(input.i32()?).map_err(|_| invalid())?;
            let block_columns = u64::try_from(input.i32()?).map_err(|_| invalid())?;
            let count = input.count()?;
            if unit == BP && i64::from(size) == i64::from(bin) {
                if block_bins == 0 {
                    return Err(invalid());
                }
                let mut blocks = HashMap::new();
                for _ in 0..count {
                    let number = u64::try_from(input.i32()?).map_err(|_| invalid())?;
                    let at = input.offset()?;
                    let size = u64::try_from(input.i32()?).map_err(|_| invalid())?;
                    blocks.insert(number, (at, size));
                }
                return Ok(Some(Matrix {
                    bin,
                    block_bins,
                    block_columns,
                    blocks,
                    diagonal: self.version > 8,
                }));
            }
            input.skip(count * 16)?;
        }
        Ok(None)
    }

    /// The contacts between the bins from `first` to `last`, of bins at most `reach` bins
    /// apart.
    pub fn contacts(
        &self,
        reader: &mut (impl Read + Seek),
        matrix: &Matrix,
        (first, last): (u32, u32),
        reach: u32,
    ) -> Result<Vec<Contact>, Error> {
        let mut found = Vec::new();
        for number in matrix.numbers(u64::from(first), u64::from(last), u64::from(reach)) {
            let Some(&(at, size)) = matrix.blocks.get(&number) else {
                continue;
            };
            if size == 0 {
                continue;
            }
            reader.seek(SeekFrom::Start(at))?;
            let mut packed = vec![0; usize::try_from(size).map_err(|_| invalid())?];
            reader.read_exact(&mut packed)?;
            let block = decompress_to_vec_zlib_with_limit(&packed, BLOCK_LIMIT)
                .map_err(|_| Error::Invalid(Reason::BadBlock))?;
            records(&block, self.version, |x, y, count| {
                let (x, y) = (x.min(y), x.max(y));
                if x >= first && y <= last && y - x <= reach && count.is_finite() {
                    found.push(Contact { x, y, count });
                }
            })?;
        }
        Ok(found)
    }

    fn vector(&self, kind: &str, chromosome: usize, bin: u32) -> Option<&Vector> {
        self.vectors.iter().find(|vector| {
            vector.kind == kind
                && usize::try_from(vector.chromosome).is_ok_and(|found| found == chromosome)
                && vector.unit == BP
                && vector.bin == i64::from(bin)
        })
    }

    /// Whether the file has the normalization vector of a chromosome at a bin size.
    pub fn has_norm(&self, kind: &str, chromosome: usize, bin: u32) -> bool {
        self.vector(kind, chromosome, bin).is_some()
    }

    /// The normalization vector of a chromosome at a bin size: one value for each bin.
    pub fn norm(
        &self,
        reader: &mut (impl Read + Seek),
        kind: &str,
        chromosome: usize,
        bin: u32,
    ) -> Result<Option<Vec<f64>>, Error> {
        let Some(vector) = self.vector(kind, chromosome, bin) else {
            return Ok(None);
        };
        let mut input = Input::new(reader);
        input.seek(vector.at)?;
        let wide = self.version > 8;
        let count = input.wide_count(wide)?;
        input.values(count, wide).map(Some)
    }

    /// The expected value of each distance in bins on a chromosome, for a normalization at a bin
    /// size.
    pub fn expected(
        &self,
        reader: &mut (impl Read + Seek),
        kind: &str,
        chromosome: usize,
        bin: u32,
    ) -> Result<Option<Vec<f64>>, Error> {
        let mut input = Input::new(reader);
        input.seek(self.sections)?;
        let chromosome = i32::try_from(chromosome).map_err(|_| invalid())?;
        Ok(walk(&mut input, self.version > 8, Some((kind, bin, chromosome)))?.0)
    }
}

/// Reads the expected values of the footer from where they start. Gives the values of `wanted`
/// (a normalization, a bin size and a chromosome) divided by the factor of the chromosome, and
/// the position after the expected values.
fn walk<R: Read + Seek>(
    input: &mut Input<R>,
    wide: bool,
    wanted: Option<(&str, u32, i32)>,
) -> Result<(Option<Vec<f64>>, u64), Error> {
    let mut found = None;
    for normalized in [false, true] {
        for _ in 0..input.count()? {
            let kind = if normalized {
                input.text()?
            } else {
                NONE.to_owned()
            };
            let unit = input.text()?;
            let bin = input.i32()?;
            let count = input.wide_count(wide)?;
            let hit = wanted.filter(|(name, size, _)| {
                *name == kind && unit == BP && i64::from(bin) == i64::from(*size)
            });
            let mut values = if hit.is_some() {
                Some(input.values(count, wide)?)
            } else {
                input.skip(count * if wide { 4 } else { 8 })?;
                None
            };
            for _ in 0..input.count()? {
                let chromosome = input.i32()?;
                let factor = if wide {
                    f64::from(input.f32()?)
                } else {
                    input.f64()?
                };
                if let (Some(values), Some((.., wanted))) = (&mut values, hit)
                    && chromosome == wanted
                    && factor.is_normal()
                {
                    for value in values.iter_mut() {
                        *value /= factor;
                    }
                }
            }
            if values.is_some() {
                found = values;
            }
        }
    }
    Ok((found, input.position()?))
}

impl Matrix {
    /// The numbers of the blocks that can hold contacts between the bins from `first` to
    /// `last` that are at most `reach` bins apart.
    fn numbers(&self, first: u64, last: u64, reach: u64) -> BTreeSet<u64> {
        let size = self.block_bins;
        let mut numbers = BTreeSet::new();
        if self.diagonal {
            let deepest = |distance: u64| {
                (1.0 + distance as f64 / std::f64::consts::SQRT_2 / size as f64).log2() as u64
            };
            let further = deepest(reach.min(last.saturating_sub(first))) + 1;
            for depth in 0..=further {
                for along in first / size..=last / size + 1 {
                    numbers.insert(depth * self.block_columns + along);
                }
            }
        } else {
            let (low, high) = (first / size, (last + 1) / size);
            let apart = reach / size + 1;
            for row in low..=high {
                for column in low..=high {
                    if row.abs_diff(column) <= apart {
                        numbers.insert(row * self.block_columns + column);
                    }
                }
            }
        }
        numbers
    }
}

/// Calls `found` with the two bins and the count of each contact of a block.
fn records(block: &[u8], version: i32, mut found: impl FnMut(u32, u32, f32)) -> Result<(), Error> {
    let mut bytes = Bytes { data: block, at: 0 };
    let total = bytes.i32()?;
    let mut call = |x: i32, y: i32, count: f32| {
        if let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) {
            found(x, y, count);
        }
    };
    if version < 7 {
        for _ in 0..total.max(0) {
            let (x, y) = (bytes.i32()?, bytes.i32()?);
            call(x, y, bytes.f32()?);
        }
        return Ok(());
    }
    let x_offset = bytes.i32()?;
    let y_offset = bytes.i32()?;
    let short_counts = bytes.u8()? == 0;
    let (short_x, short_y) = if version > 8 {
        (bytes.u8()? == 0, bytes.u8()? == 0)
    } else {
        (true, true)
    };
    let number = |bytes: &mut Bytes<'_>, short: bool| -> Result<i32, Error> {
        if short {
            Ok(i32::from(bytes.i16()?))
        } else {
            bytes.i32()
        }
    };
    match bytes.u8()? {
        1 => {
            for _ in 0..number(&mut bytes, short_y)?.max(0) {
                let y = y_offset.saturating_add(number(&mut bytes, short_y)?);
                for _ in 0..number(&mut bytes, short_x)?.max(0) {
                    let x = x_offset.saturating_add(number(&mut bytes, short_x)?);
                    let count = if short_counts {
                        f32::from(bytes.i16()?)
                    } else {
                        bytes.f32()?
                    };
                    call(x, y, count);
                }
            }
        }
        2 => {
            let points = bytes.i32()?;
            let width = i32::from(bytes.i16()?);
            if width <= 0 {
                return Err(Error::Invalid(Reason::BadBlock));
            }
            for index in 0..points.max(0) {
                let (row, column) = (index / width, index % width);
                let (x, y) = (
                    x_offset.saturating_add(column),
                    y_offset.saturating_add(row),
                );
                if short_counts {
                    let count = bytes.i16()?;
                    if count != i16::MIN {
                        call(x, y, f32::from(count));
                    }
                } else {
                    let count = bytes.f32()?;
                    if !count.is_nan() {
                        call(x, y, count);
                    }
                }
            }
        }
        _ => return Err(Error::Invalid(Reason::BadBlock)),
    }
    Ok(())
}

fn invalid() -> Error {
    Error::Invalid(Reason::BadIndex)
}

/// A reader of the little-endian fields of the file.
struct Input<R> {
    reader: BufReader<R>,
}

impl<R: Read + Seek> Input<R> {
    fn new(reader: R) -> Self {
        Self {
            reader: BufReader::with_capacity(BUFFER, reader),
        }
    }

    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut bytes = [0; N];
        self.reader.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.bytes()?))
    }

    fn i64(&mut self) -> Result<i64, Error> {
        Ok(i64::from_le_bytes(self.bytes()?))
    }

    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.bytes()?))
    }

    fn f64(&mut self) -> Result<f64, Error> {
        Ok(f64::from_le_bytes(self.bytes()?))
    }

    fn offset(&mut self) -> Result<u64, Error> {
        u64::try_from(self.i64()?).map_err(|_| Error::Invalid(Reason::BadOffset))
    }

    fn count(&mut self) -> Result<u64, Error> {
        u64::try_from(self.i32()?)
            .ok()
            .filter(|count| *count <= COUNT_LIMIT)
            .ok_or_else(invalid)
    }

    /// A count of values: 8 bytes from version 9, else 4.
    fn wide_count(&mut self, wide: bool) -> Result<u64, Error> {
        let count = if wide {
            self.i64()?
        } else {
            i64::from(self.i32()?)
        };
        u64::try_from(count)
            .ok()
            .filter(|count| *count <= VALUES_LIMIT)
            .ok_or_else(invalid)
    }

    /// Values of 4 bytes from version 9, else of 8 bytes.
    fn values(&mut self, count: u64, wide: bool) -> Result<Vec<f64>, Error> {
        let width = if wide { 4 } else { 8 };
        let mut bytes = vec![0; usize::try_from(count * width).map_err(|_| invalid())?];
        self.reader.read_exact(&mut bytes)?;
        Ok(if wide {
            let (words, _) = bytes.as_chunks::<4>();
            words
                .iter()
                .map(|word| f64::from(f32::from_le_bytes(*word)))
                .collect()
        } else {
            let (words, _) = bytes.as_chunks::<8>();
            words.iter().map(|word| f64::from_le_bytes(*word)).collect()
        })
    }

    fn text(&mut self) -> Result<String, Error> {
        let mut bytes = Vec::new();
        (&mut self.reader)
            .take(TEXT_LIMIT)
            .read_until(0, &mut bytes)?;
        if bytes.pop() != Some(0) {
            return Err(Error::Invalid(Reason::NotHic));
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn seek(&mut self, at: u64) -> Result<(), Error> {
        self.reader.seek(SeekFrom::Start(at))?;
        Ok(())
    }

    fn skip(&mut self, count: u64) -> Result<(), Error> {
        let count = i64::try_from(count).map_err(|_| invalid())?;
        self.reader.seek_relative(count)?;
        Ok(())
    }

    fn position(&mut self) -> Result<u64, Error> {
        Ok(self.reader.stream_position()?)
    }
}

/// A reader of the little-endian fields of a block.
struct Bytes<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let bytes = self
            .data
            .get(self.at..self.at + N)
            .ok_or(Error::Invalid(Reason::BadBlock))?;
        self.at += N;
        let mut found = [0; N];
        found.copy_from_slice(bytes);
        Ok(found)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take::<1>()?[0])
    }

    fn i16(&mut self) -> Result<i16, Error> {
        Ok(i16::from_le_bytes(self.take()?))
    }

    fn i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.take()?))
    }

    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.take()?))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;
    use std::io::Cursor;

    use miniz_oxide::deflate::compress_to_vec_zlib;

    use super::{Contact, Hic, NONE};
    use crate::{Error, Reason};

    /// The contacts of chr1 at 100 bp bins in the test files.
    pub(crate) fn fine() -> Vec<(u32, u32, f32)> {
        let mut found = Vec::new();
        for x in (0..100).step_by(7) {
            for distance in [0, 3, 12, 40] {
                if x + distance < 100 {
                    found.push((x, x + distance, (x + distance) as f32 + 1.0));
                }
            }
        }
        found
    }

    fn coarse() -> Vec<(u32, u32, f32)> {
        let mut sums: BTreeMap<(u32, u32), f32> = BTreeMap::new();
        for (x, y, count) in fine() {
            *sums.entry((x / 10, y / 10)).or_default() += count;
        }
        sums.into_iter()
            .map(|((x, y), count)| (x, y, count))
            .collect()
    }

    struct Out {
        bytes: Vec<u8>,
        wide: bool,
    }

    impl Out {
        fn i16(&mut self, value: impl TryInto<i16>) {
            let value: i16 = value.try_into().ok().unwrap();
            self.bytes.extend(value.to_le_bytes());
        }
        fn i32(&mut self, value: impl TryInto<i32>) {
            let value: i32 = value.try_into().ok().unwrap();
            self.bytes.extend(value.to_le_bytes());
        }
        fn i64(&mut self, value: impl TryInto<i64>) {
            let value: i64 = value.try_into().ok().unwrap();
            self.bytes.extend(value.to_le_bytes());
        }
        fn text(&mut self, value: &str) {
            self.bytes.extend(value.as_bytes());
            self.bytes.push(0);
        }
        fn count(&mut self, value: usize) {
            if self.wide {
                self.i64(value);
            } else {
                self.i32(value);
            }
        }
        fn values(&mut self, values: &[f64]) {
            self.count(values.len());
            for value in values {
                self.factor(*value);
            }
        }
        fn factor(&mut self, value: f64) {
            if self.wide {
                self.bytes.extend((value as f32).to_le_bytes());
            } else {
                self.bytes.extend(value.to_le_bytes());
            }
        }
        fn at(&self) -> i64 {
            i64::try_from(self.bytes.len()).unwrap()
        }
        fn patch(&mut self, slot: usize, value: i64) {
            self.bytes[slot..slot + 8].copy_from_slice(&value.to_le_bytes());
        }
    }

    const BLOCK_BINS: u32 = 10;
    const BLOCK_COLUMNS: u32 = 12;

    fn number(version: i32, x: u32, y: u32) -> u32 {
        if version > 8 {
            let along = x.midpoint(y) / BLOCK_BINS;
            let depth = (1.0 + f64::from(y - x) / std::f64::consts::SQRT_2 / f64::from(BLOCK_BINS))
                .log2() as u32;
            depth * BLOCK_COLUMNS + along
        } else {
            (y / BLOCK_BINS) * BLOCK_COLUMNS + x / BLOCK_BINS
        }
    }

    /// A block in one of four styles: a list with float counts and short positions, a list with
    /// short counts (and wide positions in version 9), and dense blocks of floats or shorts.
    fn block(version: i32, style: usize, contacts: &[(u32, u32, f32)]) -> Vec<u8> {
        let mut out = Out {
            bytes: Vec::new(),
            wide: false,
        };
        let (x_offset, y_offset) = (
            contacts.iter().map(|c| c.0).min().unwrap(),
            contacts.iter().map(|c| c.1).min().unwrap(),
        );
        out.i32(contacts.len());
        out.i32(x_offset);
        out.i32(y_offset);
        let short_counts = style % 2 == 1;
        out.bytes.push(u8::from(!short_counts));
        let wide_positions = version > 8 && style == 1;
        if version > 8 {
            out.bytes
                .extend([u8::from(wide_positions), u8::from(wide_positions)]);
        }
        let count = |out: &mut Out, value: f32| {
            if short_counts {
                out.bytes.extend((value as i16).to_le_bytes());
            } else {
                out.bytes.extend(value.to_le_bytes());
            }
        };
        let position = |out: &mut Out, value: u32| {
            if wide_positions {
                out.i32(value);
            } else {
                out.i16(value);
            }
        };
        if style < 2 {
            out.bytes.push(1);
            let mut rows: BTreeMap<u32, Vec<(u32, f32)>> = BTreeMap::new();
            for (x, y, value) in contacts {
                rows.entry(*y).or_default().push((*x, *value));
            }
            position(&mut out, u32::try_from(rows.len()).unwrap());
            for (y, row) in rows {
                position(&mut out, y - y_offset);
                position(&mut out, u32::try_from(row.len()).unwrap());
                for (x, value) in row {
                    position(&mut out, x - x_offset);
                    count(&mut out, value);
                }
            }
        } else {
            out.bytes.push(2);
            let width = contacts.iter().map(|c| c.0).max().unwrap() - x_offset + 1;
            let height = contacts.iter().map(|c| c.1).max().unwrap() - y_offset + 1;
            out.i32(width * height);
            out.i16(width);
            for row in 0..height {
                for column in 0..width {
                    let found = contacts
                        .iter()
                        .find(|c| c.0 == x_offset + column && c.1 == y_offset + row);
                    match (found, short_counts) {
                        (Some(c), _) => count(&mut out, c.2),
                        (None, true) => out.bytes.extend(i16::MIN.to_le_bytes()),
                        (None, false) => out.bytes.extend(f32::NAN.to_le_bytes()),
                    }
                }
            }
        }
        compress_to_vec_zlib(&out.bytes, 6)
    }

    /// The header of a test file. Gives the places of the footer position and the place of the
    /// position of the normalization index (version 9).
    fn header(out: &mut Out, version: i32) -> (usize, usize) {
        out.i32(version);
        let master_slot = out.bytes.len();
        out.i64(0);
        out.text("test");
        let index_slot = out.bytes.len();
        if out.wide {
            out.i64(0);
            out.i64(0);
        }
        out.i32(1);
        out.text("software");
        out.text("genie test");
        out.i32(3);
        for (name, length) in [("All", 15), ("chr1", 10_000), ("chr2", 5_000)] {
            out.text(name);
            if out.wide {
                out.i64(length);
            } else {
                out.i32(length);
            }
        }
        out.i32(2);
        out.i32(1_000);
        out.i32(100);
        out.i32(0);
        (master_slot, index_slot)
    }

    /// The blocks and the matrix of chr1 at 1 kb and 100 bp. Gives the position of the matrix.
    fn zooms(out: &mut Out, version: i32) -> i64 {
        let mut zooms = Vec::new();
        for (bin, contacts) in [(1_000, coarse()), (100, fine())] {
            let mut grouped: BTreeMap<u32, Vec<(u32, u32, f32)>> = BTreeMap::new();
            for contact in contacts {
                grouped
                    .entry(number(version, contact.0, contact.1))
                    .or_default()
                    .push(contact);
            }
            let mut blocks = Vec::new();
            for (style, (number, group)) in grouped.into_iter().enumerate() {
                let packed = block(version, style % 4, &group);
                blocks.push((number, out.at(), packed.len()));
                out.bytes.extend(packed);
            }
            zooms.push((bin, blocks));
        }
        let matrix = out.at();
        out.i32(1);
        out.i32(1);
        out.i32(zooms.len());
        for (bin, blocks) in &zooms {
            out.text("BP");
            out.i32(0);
            for _ in 0..4 {
                out.bytes.extend(1.0_f32.to_le_bytes());
            }
            out.i32(*bin);
            out.i32(BLOCK_BINS);
            out.i32(BLOCK_COLUMNS);
            out.i32(blocks.len());
            for (number, at, size) in blocks {
                out.i32(*number);
                out.i64(*at);
                out.i32(*size);
            }
        }
        matrix
    }

    /// The footer: the master index, the expected values and the normalization index.
    fn footer(out: &mut Out, matrix: i64, (kr_at, scale_at): (i64, i64), index_slot: usize) {
        out.count(0);
        out.i32(1);
        out.text("1_1");
        out.i64(matrix);
        out.i32(0);
        out.i32(2);
        out.text("BP");
        out.i32(1_000);
        out.values(&[80.0, 40.0]);
        out.i32(0);
        out.text("BP");
        out.i32(100);
        out.values(&[8.0, 4.0, 2.0, 1.0]);
        out.i32(2);
        out.i32(1);
        out.factor(2.0);
        out.i32(2);
        out.factor(4.0);
        out.i32(1);
        out.text("KR");
        out.text("BP");
        out.i32(100);
        out.values(&[1.0, 0.5]);
        out.i32(1);
        out.i32(1);
        out.factor(1.0);
        let index = out.at();
        out.i32(2);
        let (width, head) = if out.wide { (4, 8) } else { (8, 4) };
        for (kind, bin, at, length) in [("KR", 100, kr_at, 100), ("SCALE", 1_000, scale_at, 10)] {
            out.text(kind);
            out.i32(1);
            out.text("BP");
            out.i32(bin);
            out.i64(at);
            out.count(length * width + head);
        }
        if out.wide {
            out.patch(index_slot, index);
            let length = out.at() - index;
            out.patch(index_slot + 8, length);
        }
    }

    /// A Hi-C file of a version with chr1 (10 kb) and chr2 (5 kb), contacts on chr1 at 1 kb and
    /// 100 bp, a KR vector of chr1 at 100 bp, a SCALE vector of chr1 at 1 kb, and expected values.
    pub(crate) fn file(version: i32) -> Vec<u8> {
        let mut out = Out {
            bytes: b"HIC\0".to_vec(),
            wide: version > 8,
        };
        let (master_slot, index_slot) = header(&mut out, version);
        let matrix = zooms(&mut out, version);
        let kr: Vec<f64> = (0..100).map(|bin| 1.0 + f64::from(bin) / 128.0).collect();
        let kr_at = out.at();
        out.values(&kr);
        let scale_at = out.at();
        out.values(&[2.0; 10]);
        let master = out.at();
        out.patch(master_slot, master);
        footer(&mut out, matrix, (kr_at, scale_at), index_slot);
        out.bytes
    }

    fn sorted(mut contacts: Vec<Contact>) -> Vec<(u32, u32, f32)> {
        contacts.sort_by_key(|contact| (contact.x, contact.y));
        contacts
            .into_iter()
            .map(|contact| (contact.x, contact.y, contact.count))
            .collect()
    }

    #[test]
    fn headers_list_the_chromosomes_resolutions_and_normalizations() {
        for version in [8, 9] {
            let hic = Hic::read(&mut Cursor::new(file(version))).unwrap();
            let names: Vec<&str> = hic
                .chromosomes
                .iter()
                .map(|(name, _)| name.as_str())
                .collect();
            assert_eq!(names, ["All", "chr1", "chr2"], "{version}");
            assert_eq!(hic.chromosomes[1].1, 10_000);
            assert_eq!(hic.resolutions, [1_000, 100]);
            assert_eq!(hic.normalizations(), [NONE, "KR", "SCALE"]);
        }
    }

    #[test]
    fn contacts_come_from_the_blocks_of_a_region() {
        for version in [8, 9] {
            let bytes = file(version);
            let mut reader = Cursor::new(bytes);
            let hic = Hic::read(&mut reader).unwrap();
            let matrix = hic.matrix(&mut reader, 1, 100).unwrap().unwrap();
            let all = hic.contacts(&mut reader, &matrix, (0, 99), 99).unwrap();
            assert_eq!(sorted(all), fine(), "{version}");
            let part = hic.contacts(&mut reader, &matrix, (20, 60), 12).unwrap();
            let wanted: Vec<(u32, u32, f32)> = fine()
                .into_iter()
                .filter(|(x, y, _)| *x >= 20 && *y <= 60 && y - x <= 12)
                .collect();
            assert_eq!(sorted(part), wanted, "{version}");
            let coarse_matrix = hic.matrix(&mut reader, 1, 1_000).unwrap().unwrap();
            let summed = hic
                .contacts(&mut reader, &coarse_matrix, (0, 9), 9)
                .unwrap();
            assert_eq!(sorted(summed), coarse(), "{version}");
            assert_eq!(hic.matrix(&mut reader, 2, 100).unwrap(), None);
            assert_eq!(hic.matrix(&mut reader, 1, 5).unwrap(), None);
        }
    }

    #[test]
    fn normalization_vectors_and_expected_values_come_on_request() {
        for version in [8, 9] {
            let mut reader = Cursor::new(file(version));
            let hic = Hic::read(&mut reader).unwrap();
            let kr = hic.norm(&mut reader, "KR", 1, 100).unwrap().unwrap();
            assert_eq!(
                (kr.len(), kr[0], kr[99]),
                (100, 1.0, 1.773_437_5),
                "{version}"
            );
            let scale = hic.norm(&mut reader, "SCALE", 1, 1_000).unwrap().unwrap();
            assert_eq!(scale, [2.0; 10]);
            assert_eq!(hic.norm(&mut reader, "KR", 2, 100).unwrap(), None);
            assert!(hic.has_norm("KR", 1, 100) && !hic.has_norm("SCALE", 1, 100));
            let plain = hic.expected(&mut reader, NONE, 1, 100).unwrap().unwrap();
            assert_eq!(plain, [4.0, 2.0, 1.0, 0.5], "{version}");
            let other = hic.expected(&mut reader, NONE, 2, 100).unwrap().unwrap();
            assert_eq!(other, [2.0, 1.0, 0.5, 0.25]);
            let balanced = hic.expected(&mut reader, "KR", 1, 100).unwrap().unwrap();
            assert_eq!(balanced, [1.0, 0.5]);
            assert_eq!(hic.expected(&mut reader, "VC", 1, 100).unwrap(), None);
        }
    }

    #[test]
    fn the_fixture_is_the_test_file_of_version_9() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/contacts.hic");
        if std::env::var_os("GENIE_WRITE_FIXTURES").is_some() {
            std::fs::write(&path, file(9)).unwrap();
        }
        assert_eq!(std::fs::read(&path).unwrap(), file(9));
    }

    #[test]
    fn other_files_and_versions_say_so() {
        let mut wrong = file(8);
        wrong[0] = b'X';
        assert!(matches!(
            Hic::read(&mut Cursor::new(wrong)),
            Err(Error::Invalid(Reason::NotHic))
        ));
        let mut old = file(8);
        old[4] = 5;
        assert!(matches!(
            Hic::read(&mut Cursor::new(old)),
            Err(Error::Invalid(Reason::HicVersion))
        ));
        let mut short = file(9);
        short.truncate(40);
        assert!(Hic::read(&mut Cursor::new(short)).is_err());
    }
}
