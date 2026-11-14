//! The rANS Nx16 codec of CRAM 3.1 (block method 5): order 0 or 1, 4 or 32 states, and the
//! STRIPE, NOSZ, CAT, RLE and PACK transforms.

use super::bits::bad;
use crate::Error;

const ORDER: u8 = 0x01;
const X32: u8 = 0x04;
const STRIPE: u8 = 0x08;
const NOSZ: u8 = 0x10;
const CAT: u8 = 0x20;
const RLE: u8 = 0x40;
const PACK: u8 = 0x80;
const LOW: u32 = 1 << 15;
const SHIFT: u32 = 12;
const LIMIT: usize = 1 << 30;

struct Input<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Input<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    fn byte(&mut self) -> Result<u8, Error> {
        let byte = *self.data.get(self.at).ok_or_else(bad)?;
        self.at += 1;
        Ok(byte)
    }

    fn peek(&self) -> Result<u8, Error> {
        self.data.get(self.at).copied().ok_or_else(bad)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let end = self.at.checked_add(count).ok_or_else(bad)?;
        let part = self.data.get(self.at..end).ok_or_else(bad)?;
        self.at = end;
        Ok(part)
    }

    /// Seven bits per byte, high groups first; a set top bit means that more bytes follow.
    fn uint7(&mut self) -> Result<usize, Error> {
        let mut value: u64 = 0;
        for _ in 0..5 {
            let byte = self.byte()?;
            value = (value << 7) | u64::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                return usize::try_from(value).map_err(|_| bad());
            }
        }
        Err(bad())
    }

    fn word(&mut self) -> Result<u32, Error> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn renorm(&mut self, state: &mut u32) -> Result<(), Error> {
        if *state < LOW {
            let bytes = self.take(2)?;
            *state = (*state << 16) | u32::from(u16::from_le_bytes([bytes[0], bytes[1]]));
        }
        Ok(())
    }
}

pub(crate) fn decode(input: &[u8], size: Option<usize>) -> Result<Vec<u8>, Error> {
    decode_from(&mut Input::new(input), size)
}

fn decode_from(input: &mut Input, size: Option<usize>) -> Result<Vec<u8>, Error> {
    let flags = input.byte()?;
    let size = if flags & NOSZ == 0 {
        input.uint7()?
    } else {
        size.ok_or_else(bad)?
    };
    if size > LIMIT {
        return Err(bad());
    }
    if flags & STRIPE != 0 {
        return stripe(input, size);
    }
    let mut length = size;
    let mut packed = None;
    if flags & PACK != 0 {
        let count = usize::from(input.byte()?);
        let map = input.take(count)?.to_vec();
        packed = Some((map, length));
        length = input.uint7()?;
    }
    let mut runs = None;
    if flags & RLE != 0 {
        let meta_size = input.uint7()?;
        let literals = input.uint7()?;
        let meta = if meta_size & 1 != 0 {
            input.take(meta_size / 2)?.to_vec()
        } else {
            let compressed = input.uint7()?;
            order0(&mut Input::new(input.take(compressed)?), meta_size / 2, 4)?
        };
        runs = Some((meta, length));
        length = literals;
    }
    if length > LIMIT {
        return Err(bad());
    }
    let lanes = if flags & X32 != 0 { 32 } else { 4 };
    let mut data = if flags & CAT != 0 {
        input.take(length)?.to_vec()
    } else if flags & ORDER != 0 {
        order1(input, length, lanes)?
    } else {
        order0(input, length, lanes)?
    };
    if let Some((meta, length)) = runs {
        data = unrun(&data, &meta, length)?;
    }
    if let Some((map, length)) = packed {
        data = unpack(&data, &map, length)?;
    }
    if data.len() != size {
        return Err(bad());
    }
    Ok(data)
}

fn stripe(input: &mut Input, size: usize) -> Result<Vec<u8>, Error> {
    let count = usize::from(input.byte()?);
    if count == 0 {
        return Err(bad());
    }
    let lengths = (0..count)
        .map(|_| input.uint7())
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = vec![0; size];
    for (part, length) in lengths.into_iter().enumerate() {
        let wanted = size / count + usize::from(size % count > part);
        let bytes = decode_from(&mut Input::new(input.take(length)?), Some(wanted))?;
        if bytes.len() != wanted {
            return Err(bad());
        }
        for (index, byte) in bytes.into_iter().enumerate() {
            out[index * count + part] = byte;
        }
    }
    Ok(out)
}

/// The symbols of a frequency table: a run-length coded list that a 0 ends.
fn alphabet(input: &mut Input) -> Result<[bool; 256], Error> {
    let mut present = [false; 256];
    let mut symbol = usize::from(input.byte()?);
    let mut run = 0;
    loop {
        present[symbol] = true;
        if run == 0 && symbol + 1 == usize::from(input.peek()?) {
            symbol = usize::from(input.byte()?);
            run = input.byte()?;
        } else if run > 0 {
            run -= 1;
            symbol += 1;
        } else {
            symbol = usize::from(input.byte()?);
        }
        match symbol {
            0 => return Ok(present),
            256.. => return Err(bad()),
            _ => {}
        }
    }
}

/// Frequencies that sum to a power of two are scaled up to the total of the codec.
fn normalise(freq: &mut [u32; 256], total: u32, wanted: u32) -> Result<(), Error> {
    if total == 0 || total == wanted {
        return Ok(());
    }
    let mut shift = 0;
    while (total << shift) < wanted {
        shift += 1;
    }
    if total << shift != wanted {
        return Err(bad());
    }
    for value in freq.iter_mut() {
        *value <<= shift;
    }
    Ok(())
}

struct Table {
    freq: [u32; 256],
    start: [u32; 256],
    lookup: Vec<u8>,
    shift: u32,
}

impl Table {
    fn new(freq: &[u32; 256], shift: u32) -> Result<Self, Error> {
        let size = 1_usize << shift;
        let mut table = Self {
            freq: *freq,
            start: [0; 256],
            lookup: vec![0; size],
            shift,
        };
        let mut total = 0_usize;
        for (symbol, value) in freq.iter().enumerate() {
            let end = total + *value as usize;
            if end > size {
                return Err(bad());
            }
            table.start[symbol] = total as u32;
            table.lookup[total..end].fill(symbol as u8);
            total = end;
        }
        Ok(table)
    }

    fn peek(&self, state: u32) -> u8 {
        self.lookup[(state & ((1 << self.shift) - 1)) as usize]
    }

    fn step(&self, state: &mut u32) -> u8 {
        let low = *state & ((1 << self.shift) - 1);
        let symbol = self.lookup[low as usize];
        let index = usize::from(symbol);
        *state = self.freq[index] * (*state >> self.shift) + low - self.start[index];
        symbol
    }
}

fn states(input: &mut Input, lanes: usize) -> Result<Vec<u32>, Error> {
    (0..lanes).map(|_| input.word()).collect()
}

fn order0(input: &mut Input, size: usize, lanes: usize) -> Result<Vec<u8>, Error> {
    let present = alphabet(input)?;
    let mut freq = [0_u32; 256];
    let mut total = 0_u32;
    for (symbol, value) in freq.iter_mut().enumerate() {
        if present[symbol] {
            *value = u32::try_from(input.uint7()?).map_err(|_| bad())?;
            total = total.checked_add(*value).ok_or_else(bad)?;
        }
    }
    normalise(&mut freq, total, 1 << SHIFT)?;
    let table = Table::new(&freq, SHIFT)?;
    let mut states = states(input, lanes)?;
    let mut out = vec![0; size];
    let whole = size - size % lanes;
    for group in out[..whole].chunks_exact_mut(lanes) {
        for (byte, state) in group.iter_mut().zip(states.iter_mut()) {
            *byte = table.step(state);
        }
        for state in &mut states {
            input.renorm(state)?;
        }
    }
    for (byte, state) in out[whole..].iter_mut().zip(&states) {
        *byte = table.peek(*state);
    }
    Ok(out)
}

/// One table for each symbol before: rows of frequencies where a zero comes with the count of
/// more zeros after it.
fn tables(input: &mut Input, shift: u32) -> Result<Vec<Option<Table>>, Error> {
    let present = alphabet(input)?;
    let mut tables: Vec<Option<Table>> = (0..256).map(|_| None).collect();
    for (context, slot) in tables.iter_mut().enumerate() {
        if !present[context] {
            continue;
        }
        let mut freq = [0_u32; 256];
        let (mut total, mut run) = (0_u32, 0_u8);
        for (symbol, value) in freq.iter_mut().enumerate() {
            if !present[symbol] {
                continue;
            }
            if run > 0 {
                run -= 1;
            } else {
                *value = u32::try_from(input.uint7()?).map_err(|_| bad())?;
                if *value == 0 {
                    run = input.byte()?;
                }
            }
            total = total.checked_add(*value).ok_or_else(bad)?;
        }
        normalise(&mut freq, total, 1 << shift)?;
        *slot = Some(Table::new(&freq, shift)?);
    }
    Ok(tables)
}

fn order1(input: &mut Input, size: usize, lanes: usize) -> Result<Vec<u8>, Error> {
    let head = input.byte()?;
    let shift = u32::from(head >> 4);
    if !(1..=15).contains(&shift) {
        return Err(bad());
    }
    let tables = if head & 1 == 0 {
        tables(input, shift)?
    } else {
        let unpacked = input.uint7()?;
        let packed = input.uint7()?;
        let raw = order0(&mut Input::new(input.take(packed)?), unpacked, 4)?;
        tables(&mut Input::new(&raw), shift)?
    };
    let mut states = states(input, lanes)?;
    let segment = size / lanes;
    let mut contexts = vec![0_usize; lanes];
    let mut out = vec![0; size];
    for index in 0..segment {
        for (lane, state) in states.iter_mut().enumerate() {
            let table = tables[contexts[lane]].as_ref().ok_or_else(bad)?;
            let symbol = table.step(state);
            out[lane * segment + index] = symbol;
            contexts[lane] = usize::from(symbol);
        }
        for state in &mut states {
            input.renorm(state)?;
        }
    }
    let last = lanes - 1;
    for byte in &mut out[lanes * segment..] {
        let table = tables[contexts[last]].as_ref().ok_or_else(bad)?;
        *byte = table.step(&mut states[last]);
        contexts[last] = usize::from(*byte);
        input.renorm(&mut states[last])?;
    }
    Ok(out)
}

/// Literals with run symbols: each run symbol takes a run length (one less than its copies)
/// from the meta data, after the list of run symbols (a count, 0 for all 256, then symbols).
fn unrun(literals: &[u8], meta: &[u8], size: usize) -> Result<Vec<u8>, Error> {
    let mut meta = Input::new(meta);
    let count = match meta.byte()? {
        0 => 256,
        count => usize::from(count),
    };
    let mut runs = [false; 256];
    for _ in 0..count {
        runs[usize::from(meta.byte()?)] = true;
    }
    let mut out = Vec::with_capacity(size);
    for &byte in literals {
        let copies = if runs[usize::from(byte)] {
            meta.uint7()? + 1
        } else {
            1
        };
        if out.len() + copies > size {
            return Err(bad());
        }
        out.resize(out.len() + copies, byte);
    }
    Ok(out)
}

/// Bytes that hold 8, 4 or 2 symbol numbers (1, 2 or 4 bits each, low bits first), or no data
/// when there is one symbol.
fn unpack(data: &[u8], map: &[u8], size: usize) -> Result<Vec<u8>, Error> {
    let bits = match map.len() {
        1 => return Ok(vec![map[0]; size]),
        2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => return Err(bad()),
    };
    let mask = (1_u8 << bits) - 1;
    let mut out = Vec::with_capacity(size);
    'bytes: for byte in data {
        for slot in 0..8 / bits {
            if out.len() == size {
                break 'bytes;
            }
            let value = usize::from((byte >> (slot * bits)) & mask);
            out.push(*map.get(value).ok_or_else(bad)?);
        }
    }
    if out.len() != size {
        return Err(bad());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::decode;

    fn states(count: usize, state: u32) -> Vec<u8> {
        (0..count).flat_map(|_| state.to_le_bytes()).collect()
    }

    #[test]
    fn cat_blocks_are_stored_bytes() {
        let mut block = vec![0x20, 5];
        block.extend(b"hello");
        assert_eq!(decode(&block, None).unwrap(), b"hello");
        let mut quiet = vec![0x30];
        quiet.extend(b"abc");
        assert_eq!(decode(&quiet, Some(3)).unwrap(), b"abc");
    }

    #[test]
    fn order_zero_uses_four_or_thirty_two_states() {
        // One symbol at the whole total: every state gives Q and keeps its value.
        let mut block = vec![0x00, 9, b'Q', 0, 1];
        block.extend(states(4, 1 << 15));
        assert_eq!(decode(&block, None).unwrap(), b"QQQQQQQQQ");
        let mut wide = vec![0x04, 40, b'Q', 0, 1];
        wide.extend(states(32, 1 << 15));
        assert_eq!(decode(&wide, None).unwrap(), vec![b'Q'; 40]);
    }

    #[test]
    fn sizes_are_seven_bit_groups() {
        let mut block = vec![0x00, 0x81, 0x00, b'Z', 0, 1];
        block.extend(states(4, 1 << 15));
        assert_eq!(decode(&block, None).unwrap(), vec![b'Z'; 128]);
    }

    #[test]
    fn pack_spreads_small_alphabets_over_bits() {
        // Two symbols A and C, one bit each, low bits first: 0b1010_0101 gives CACAACAC.
        let mut block = vec![0xA0, 8, 2, b'A', b'C', 1, 0b1010_0101];
        assert_eq!(decode(&block, None).unwrap(), b"CACAACAC");
        // One symbol: no data at all.
        block = vec![0xA0, 4, 1, b'T', 0];
        assert_eq!(decode(&block, None).unwrap(), b"TTTT");
    }

    #[test]
    fn rle_repeats_marked_symbols() {
        // Literals "AxB", A is a run symbol with 2 more copies, B with none; meta is stored.
        let mut block = vec![0x60, 5];
        let meta = [2, b'A', b'B', 2, 0];
        block.push((meta.len() as u8) * 2 + 1);
        block.push(3);
        block.extend(meta);
        block.extend(b"AxB");
        assert_eq!(decode(&block, None).unwrap(), b"AAAxB");
    }

    #[test]
    fn stripes_interleave_their_parts() {
        // Two parts "ACE" and "BD" give "ABCDE".
        let first = [0x20, 3, b'A', b'C', b'E'];
        let second = [0x20, 2, b'B', b'D'];
        let mut block = vec![0x08, 5, 2, first.len() as u8, second.len() as u8];
        block.extend(first);
        block.extend(second);
        assert_eq!(decode(&block, None).unwrap(), b"ABCDE");
    }

    #[test]
    fn order_one_uses_the_symbol_before() {
        // Order 1 with a stored table: shift 12; symbols 0 and C; contexts 0 and C each always
        // give C. A zero frequency comes with the count of more zeros after it.
        let mut block = vec![0x01, 9, 0xC0, 0, b'C', 0, 0, 0, 1, 0, 0, 1];
        block.extend(states(4, 1 << 15));
        assert_eq!(decode(&block, None).unwrap(), b"CCCCCCCCC");
    }

    #[test]
    fn broken_blocks_are_errors() {
        assert!(decode(&[], None).is_err());
        assert!(decode(&[0x20, 9, b'a'], None).is_err());
        assert!(decode(&[0x00, 4, b'Q', 0, 1, 0, 0], None).is_err());
    }
}
