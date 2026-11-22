//! The rANS Nx16 codec of CRAM 3.1 (block method 5): order 0 or 1, 4 or 32 states, and the
//! STRIPE, NOSZ, CAT, RLE and PACK transforms.

#[cfg(verus_keep_ghost)]
use vstd::arithmetic::div_mod::{lemma_fundamental_div_mod, lemma_mod_bound};
#[cfg(verus_keep_ghost)]
use vstd::arithmetic::mul::{
    lemma_mul_inequality, lemma_mul_is_commutative, lemma_mul_is_distributive_sub_other_way,
};
#[cfg(verus_keep_ghost)]
use vstd::arithmetic::power2::pow2;
#[cfg(verus_keep_ghost)]
use vstd::bits::{
    lemma_u32_low_bits_mask_is_mod, lemma_u32_pow2_no_overflow, lemma_u32_shl_is_mul,
    lemma_u32_shr_is_div,
};
use vstd::prelude::*;
use vstd::slice::{slice_subrange, slice_to_vec};

use super::bits::bad;
use crate::Error;

pub(crate) fn decode(input: &[u8], size: Option<usize>) -> Result<Vec<u8>, Error> {
    let mut input = Input { data: input, at: 0 };
    decode_from(&mut input, size, 0).ok_or_else(bad)
}
verus! {

const ORDER: u8 = 0x01;
const X32: u8 = 0x04;
const STRIPE: u8 = 0x08;
const NOSZ: u8 = 0x10;
const CAT: u8 = 0x20;
const RLE: u8 = 0x40;
const PACK: u8 = 0x80;
/// The lowest state after a renormalization: 2 to the power of 15.
const LOW: u32 = 0x8000;
/// The total of the frequencies of an order-0 table: 2 to the power of `SHIFT`.
const TOTAL: u32 = 0x1000;
const SHIFT: u32 = 12;
/// The largest size of an output: 2 to the power of 30 bytes.
const LIMIT: usize = 0x4000_0000;
/// The most levels of stripes in stripes.
const DEPTH: usize = 4;

/// The bytes of a stream and the place of the next byte.
struct Input<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Input<'a> {
    /// The input has the bytes of `before`, and its place is the place of `before` or later.
    spec fn after(self, before: Input<'a>) -> bool {
        &&& self.data == before.data
        &&& before.at <= self.at <= self.data.len()
    }

    fn byte(&mut self) -> (byte: Option<u8>)
        requires
            old(self).at <= old(self).data.len(),
        ensures
            final(self).after(*old(self)),
            byte is Some ==> final(self).at == old(self).at + 1,
            byte is None ==> final(self).at == old(self).at,
    {
        if self.at < self.data.len() {
            let byte = self.data[self.at];
            self.at += 1;
            Some(byte)
        } else {
            None
        }
    }

    fn peek(&self) -> (byte: Option<u8>)
        requires
            self.at <= self.data.len(),
    {
        if self.at < self.data.len() {
            Some(self.data[self.at])
        } else {
            None
        }
    }

    /// The next `count` bytes.
    fn take(&mut self, count: usize) -> (part: Option<&'a [u8]>)
        requires
            old(self).at <= old(self).data.len(),
        ensures
            final(self).after(*old(self)),
            match part {
                Some(part) => part.len() == count,
                None => true,
            },
    {
        let end = self.at.checked_add(count)?;
        if end > self.data.len() {
            return None;
        }
        let part = slice_subrange(self.data, self.at, end);
        self.at = end;
        Some(part)
    }

    /// Seven bits for each byte, the high groups first; a byte with its top bit set has more
    /// bytes after it. A value of more than 32 bits gives nothing.
    fn uint7(&mut self) -> (value: Option<usize>)
        requires
            old(self).at <= old(self).data.len(),
        ensures
            final(self).after(*old(self)),
            match value {
                Some(value) => value <= u32::MAX,
                None => true,
            },
    {
        let mut value: u64 = 0;
        let mut count: usize = 0;
        while count < 5
            invariant
                self.after(*old(self)),
                count <= 5,
            decreases 5 - count,
        {
            let byte = self.byte()?;
            value = value << 7 | u64::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                if value > 0xFFFF_FFFF {
                    return None;
                }
                return Some(value as usize);
            }
            count += 1;
        }
        None
    }

    /// A state: 4 bytes, the low byte first.
    fn word(&mut self) -> (word: Option<u32>)
        requires
            old(self).at <= old(self).data.len(),
        ensures
            final(self).after(*old(self)),
    {
        let first = self.byte()?;
        let second = self.byte()?;
        let third = self.byte()?;
        let fourth = self.byte()?;
        Some(u32::from(first) | u32::from(second) << 8 | u32::from(third) << 16 | u32::from(fourth)
            << 24)
    }

    /// A state below `LOW` takes the next 2 bytes, the low byte first, as its new low bits.
    fn renorm(&mut self, state: u32) -> (renormed: Option<u32>)
        requires
            old(self).at <= old(self).data.len(),
        ensures
            final(self).after(*old(self)),
    {
        if state < LOW {
            let low = self.byte()?;
            let high = self.byte()?;
            Some(state << 16 | u32::from(low) | u32::from(high) << 8)
        } else {
            Some(state)
        }
    }
}

/// A frequency table: the frequency and the first place of each symbol, and the symbol of each
/// of the 2 to the power of `shift` places.
struct Table {
    freq: [u32; 256],
    start: [u32; 256],
    lookup: Vec<u8>,
    shift: u32,
}

impl Table {
    /// A table can decode: 2 to the power of `shift` places with a symbol each, frequencies of
    /// that number or less, and the first place of the symbol of each place at or before the
    /// place.
    spec fn decodes(&self) -> bool {
        &&& 1 <= self.shift <= 15
        &&& self.lookup.len() == pow2(self.shift as nat)
        &&& forall|symbol: int|
            0 <= symbol < 256 ==> #[trigger] self.freq[symbol] <= pow2(self.shift as nat)
        &&& forall|place: int|
            0 <= place < self.lookup.len() ==> self.start[#[trigger] self.lookup[place] as int]
                <= place
    }

    /// The place of a state: its low `shift` bits.
    fn place(&self, state: u32) -> (place: u32)
        requires
            self.decodes(),
        ensures
            place == state as nat % pow2(self.shift as nat),
            place < self.lookup.len(),
    {
        proof {
            lemma_u32_pow2_no_overflow(self.shift as nat);
            lemma_u32_shl_is_mul(1, self.shift);
            lemma_u32_low_bits_mask_is_mod(state, self.shift as nat);
            lemma_mod_bound(state as int, pow2(self.shift as nat) as int);
        }
        state & ((1u32 << self.shift) - 1)
    }

    /// The symbol of a state, for the last symbol of the state.
    fn peek(&self, state: u32) -> (symbol: u8)
        requires
            self.decodes(),
    {
        let place = self.place(state);
        self.lookup[place as usize]
    }

    /// The symbol of a state and the next state.
    fn step(&self, state: u32) -> (found: (u8, u32))
        requires
            self.decodes(),
    {
        let low = self.place(state);
        let symbol = self.lookup[low as usize];
        let index = symbol as usize;
        let high = state >> self.shift;
        let freq = self.freq[index];
        proof {
            let size = pow2(self.shift as nat) as int;
            lemma_u32_pow2_no_overflow(self.shift as nat);
            lemma_u32_shr_is_div(state, self.shift);
            lemma_fundamental_div_mod(state as int, size);
            assert(state as int == size * (high as int) + low as int);
            assert(freq as int <= size);
            lemma_mul_inequality(freq as int, size, high as int);
        }
        (symbol, freq * high + low - self.start[index])
    }

    /// The table of the frequencies of the 256 symbols, with 2 to the power of `shift` places.
    /// Frequencies with a sum of more than the places give nothing.
    fn new(freq: &[u32; 256], shift: u32) -> (table: Option<Table>)
        requires
            1 <= shift <= 15,
        ensures
            match table {
                Some(table) => table.decodes() && table.shift == shift,
                None => true,
            },
    {
        proof {
            lemma_u32_pow2_no_overflow(shift as nat);
            lemma_u32_shl_is_mul(1, shift);
        }
        let size = (1u32 << shift) as usize;
        let mut lookup: Vec<u8> = Vec::with_capacity(size);
        while lookup.len() < size
            invariant
                lookup.len() <= size,
                forall|place: int| 0 <= place < lookup.len() ==> #[trigger] lookup[place] == 0,
            decreases size - lookup.len(),
        {
            lookup.push(0);
        }
        let mut start = [0u32; 256];
        let mut total: usize = 0;
        let mut symbol: usize = 0;
        while symbol < 256
            invariant
                size == pow2(shift as nat),
                size < u32::MAX,
                lookup.len() == size,
                symbol <= 256,
                total <= size,
                symbol == 0 ==> total == 0,
                start[0] == 0,
                forall|other: int| 0 <= other < symbol ==> #[trigger] freq[other] <= size,
                forall|place: int| 0 <= place < total ==> #[trigger] lookup[place] < symbol,
                forall|place: int| total <= place < size ==> #[trigger] lookup[place] == 0,
                forall|place: int|
                    0 <= place < size ==> start[#[trigger] lookup[place] as int] <= place,
            decreases 256 - symbol,
        {
            let value = freq[symbol] as usize;
            if value > size - total {
                return None;
            }
            let end = total + value;
            start[symbol] = total as u32;
            let mut place = total;
            while place < end
                invariant
                    total <= place <= end <= size,
                    lookup.len() == size,
                    symbol < 256,
                    start[symbol as int] == total,
                    start[0] == 0,
                    forall|other: int| 0 <= other < total ==> #[trigger] lookup[other] < symbol,
                    forall|other: int|
                        total <= other < place ==> #[trigger] lookup[other] == symbol,
                    forall|other: int| place <= other < size ==> #[trigger] lookup[other] == 0,
                    forall|other: int|
                        0 <= other < size ==> start[#[trigger] lookup[other] as int] <= other,
                decreases end - place,
            {
                lookup.set(place, symbol as u8);
                place += 1;
            }
            total = end;
            symbol += 1;
        }
        Some(Table { freq: *freq, start, lookup, shift })
    }
}

/// Each table of a list of 256 tables can decode.
spec fn decodable(tables: &Vec<Option<Table>>) -> bool {
    &&& tables.len() == 256
    &&& forall|context: int|
        0 <= context < 256 ==> match #[trigger] tables[context] {
            Some(table) => table.decodes(),
            None => true,
        }
}

/// Frequencies with a sum of a power of two below `wanted` scale up to a sum of `wanted`.
fn normalise(freq: &[u32; 256], total: u32, wanted: u32) -> (scaled: Option<[u32; 256]>)
    requires
        wanted <= 0x8000,
{
    let mut scaled = *freq;
    if total == 0 || total == wanted {
        return Some(scaled);
    }
    let mut shift: u32 = 0;
    let mut sum = total;
    while sum < wanted && shift < 16
        invariant
            shift <= 16,
            wanted <= 0x8000,
        decreases 16 - shift,
    {
        sum *= 2;
        shift += 1;
    }
    if sum != wanted {
        return None;
    }
    let mut symbol: usize = 0;
    while symbol < 256
        invariant
            shift <= 16,
        decreases 256 - symbol,
    {
        scaled[symbol] <<= shift;
        symbol += 1;
    }
    Some(scaled)
}

/// The symbols of a frequency table: a list with runs that a 0 ends. A symbol that follows the
/// one before comes with the number of more symbols in its run.
fn alphabet(input: &mut Input) -> (present: Option<[bool; 256]>)
    requires
        old(input).at <= old(input).data.len(),
    ensures
        final(input).after(*old(input)),
{
    let mut present = [false; 256];
    let mut symbol = input.byte()? as usize;
    let mut run: u8 = 0;
    loop
        invariant
            input.after(*old(input)),
            symbol < 256,
        decreases input.data.len() - input.at, run,
    {
        present[symbol] = true;
        if run == 0 {
            let next = input.peek()?;
            if symbol + 1 == next as usize {
                symbol = input.byte()? as usize;
                run = input.byte()?;
            } else {
                symbol = input.byte()? as usize;
            }
        } else {
            run -= 1;
            symbol += 1;
        }
        if symbol == 0 {
            return Some(present);
        }
        if symbol > 255 {
            return None;
        }
    }
}

/// The states of `lanes` lanes.
fn states(input: &mut Input, lanes: usize) -> (states: Option<Vec<u32>>)
    requires
        old(input).at <= old(input).data.len(),
    ensures
        final(input).after(*old(input)),
        match states {
            Some(states) => states.len() == lanes,
            None => true,
        },
{
    let mut states: Vec<u32> = Vec::with_capacity(lanes);
    while states.len() < lanes
        invariant
            input.after(*old(input)),
            states.len() <= lanes,
        decreases lanes - states.len(),
    {
        let state = input.word()?;
        states.push(state);
    }
    Some(states)
}

/// The bytes of an order-0 stream of `size` bytes: one table, and `lanes` states that take
/// turns.
fn order0(input: &mut Input, size: usize, lanes: usize) -> (out: Option<Vec<u8>>)
    requires
        old(input).at <= old(input).data.len(),
        size <= LIMIT,
        1 <= lanes <= 32,
    ensures
        final(input).after(*old(input)),
        match out {
            Some(out) => out.len() == size,
            None => true,
        },
{
    let present = alphabet(input)?;
    let mut freq = [0u32; 256];
    let mut total: u32 = 0;
    let mut symbol: usize = 0;
    while symbol < 256
        invariant
            input.after(*old(input)),
        decreases 256 - symbol,
    {
        if present[symbol] {
            let value = input.uint7()? as u32;
            freq[symbol] = value;
            total = total.checked_add(value)?;
        }
        symbol += 1;
    }
    let freq = normalise(&freq, total, TOTAL)?;
    let table = Table::new(&freq, SHIFT)?;
    let mut states = states(input, lanes)?;
    let mut out: Vec<u8> = Vec::with_capacity(size);
    while size - out.len() >= lanes
        invariant
            input.after(*old(input)),
            table.decodes(),
            states.len() == lanes,
            1 <= lanes,
            out.len() <= size,
        decreases size - out.len(),
    {
        let ghost before = out.len();
        let mut lane: usize = 0;
        while lane < lanes
            invariant
                table.decodes(),
                states.len() == lanes,
                lane <= lanes,
                out.len() == before + lane,
            decreases lanes - lane,
        {
            let (symbol, next) = table.step(states[lane]);
            out.push(symbol);
            states.set(lane, next);
            lane += 1;
        }
        let mut lane: usize = 0;
        while lane < lanes
            invariant
                input.after(*old(input)),
                states.len() == lanes,
                lane <= lanes,
            decreases lanes - lane,
        {
            let next = input.renorm(states[lane])?;
            states.set(lane, next);
            lane += 1;
        }
    }
    let whole = out.len();
    while out.len() < size
        invariant
            table.decodes(),
            states.len() == lanes,
            whole + lanes > size,
            whole <= out.len() <= size,
        decreases size - out.len(),
    {
        let lane = out.len() - whole;
        out.push(table.peek(states[lane]));
    }
    Some(out)
}

/// One table for each symbol before: rows of frequencies where a zero comes with the count of
/// more zeros after it.
fn tables(input: &mut Input, shift: u32) -> (tables: Option<Vec<Option<Table>>>)
    requires
        old(input).at <= old(input).data.len(),
        1 <= shift <= 15,
    ensures
        final(input).after(*old(input)),
        match tables {
            Some(tables) => decodable(&tables),
            None => true,
        },
{
    let present = alphabet(input)?;
    let mut tables: Vec<Option<Table>> = Vec::new();
    while tables.len() < 256
        invariant
            tables.len() <= 256,
            forall|context: int| 0 <= context < tables.len() ==> #[trigger] tables[context] is None,
        decreases 256 - tables.len(),
    {
        tables.push(None);
    }
    assert((1u32 << shift) <= 0x8000) by (bit_vector)
        requires
            shift <= 15,
    ;
    let mut context: usize = 0;
    while context < 256
        invariant
            input.after(*old(input)),
            1 <= shift <= 15,
            (1u32 << shift) <= 0x8000,
            decodable(&tables),
        decreases 256 - context,
    {
        if present[context] {
            let mut freq = [0u32; 256];
            let mut total: u32 = 0;
            let mut run: u8 = 0;
            let mut symbol: usize = 0;
            while symbol < 256
                invariant
                    input.after(*old(input)),
                decreases 256 - symbol,
            {
                if present[symbol] {
                    if run > 0 {
                        run -= 1;
                    } else {
                        let value = input.uint7()? as u32;
                        freq[symbol] = value;
                        if value == 0 {
                            run = input.byte()?;
                        }
                    }
                    total = total.checked_add(freq[symbol])?;
                }
                symbol += 1;
            }
            let freq = normalise(&freq, total, 1u32 << shift)?;
            let table = Table::new(&freq, shift)?;
            tables.set(context, Some(table));
        }
        context += 1;
    }
    Some(tables)
}

/// The tables of an order-1 stream: stored, or in an order-0 stream when the low bit of `head`
/// is set.
fn order1_tables(input: &mut Input, head: u8, shift: u32) -> (tables: Option<
    Vec<Option<Table>>,
>)
    requires
        old(input).at <= old(input).data.len(),
        1 <= shift <= 15,
    ensures
        final(input).after(*old(input)),
        match tables {
            Some(tables) => decodable(&tables),
            None => true,
        },
{
    if head & 1 == 0 {
        return tables(input, shift);
    }
    let unpacked = input.uint7()?;
    let packed = input.uint7()?;
    if unpacked > LIMIT {
        return None;
    }
    let part = input.take(packed)?;
    let mut part_input = Input { data: part, at: 0 };
    let raw = order0(&mut part_input, unpacked, 4)?;
    let mut raw_input = Input { data: raw.as_slice(), at: 0 };
    tables(&mut raw_input, shift)
}

/// A list of `count` zeros: the first context of each state.
fn zeros(count: usize) -> (zeros: Vec<usize>)
    ensures
        zeros.len() == count,
        forall|at: int| 0 <= at < count ==> #[trigger] zeros[at] == 0,
{
    let mut zeros: Vec<usize> = Vec::with_capacity(count);
    while zeros.len() < count
        invariant
            zeros.len() <= count,
            forall|at: int| 0 <= at < zeros.len() ==> #[trigger] zeros[at] == 0,
        decreases count - zeros.len(),
    {
        zeros.push(0);
    }
    zeros
}

/// The bytes of an order-1 stream of `size` bytes: a table for each byte before, and `lanes`
/// states, each for a part of the bytes. The last state also takes the bytes after the parts.
fn order1(input: &mut Input, size: usize, lanes: usize) -> (out: Option<Vec<u8>>)
    requires
        old(input).at <= old(input).data.len(),
        size <= LIMIT,
        1 <= lanes <= 32,
    ensures
        final(input).after(*old(input)),
        match out {
            Some(out) => out.len() == size,
            None => true,
        },
{
    let head = input.byte()?;
    let shift = u32::from(head >> 4);
    if shift == 0 || shift > 15 {
        return None;
    }
    let tables = order1_tables(input, head, shift)?;
    let mut states = states(input, lanes)?;
    let segment = size / lanes;
    proof {
        lemma_fundamental_div_mod(size as int, lanes as int);
        lemma_mod_bound(size as int, lanes as int);
    }
    assert(lanes * segment <= size) by (nonlinear_arith)
        requires
            size == lanes * (size / lanes) + size % lanes,
            size % lanes >= 0,
            segment == size / lanes,
    ;
    let mut contexts = zeros(lanes);
    let mut out: Vec<u8> = vec![0u8; size];
    let mut index: usize = 0;
    while index < segment
        invariant
            input.after(*old(input)),
            decodable(&tables),
            states.len() == lanes,
            contexts.len() == lanes,
            forall|lane: int| 0 <= lane < lanes ==> #[trigger] contexts[lane] < 256,
            lanes * segment <= size,
            out.len() == size,
            index <= segment,
        decreases segment - index,
    {
        let mut lane: usize = 0;
        while lane < lanes
            invariant
                input.after(*old(input)),
                decodable(&tables),
                states.len() == lanes,
                contexts.len() == lanes,
                forall|other: int| 0 <= other < lanes ==> #[trigger] contexts[other] < 256,
                lanes * segment <= size,
                out.len() == size,
                index < segment,
                lane <= lanes,
            decreases lanes - lane,
        {
            let table = tables[contexts[lane]].as_ref()?;
            let (symbol, next) = table.step(states[lane]);
            assert(lane * segment + index < lanes * segment) by (nonlinear_arith)
                requires
                    lane < lanes,
                    index < segment,
            ;
            out.set(lane * segment + index, symbol);
            states.set(lane, next);
            contexts.set(lane, symbol as usize);
            lane += 1;
        }
        let mut lane: usize = 0;
        while lane < lanes
            invariant
                input.after(*old(input)),
                states.len() == lanes,
                lane <= lanes,
            decreases lanes - lane,
        {
            let next = input.renorm(states[lane])?;
            states.set(lane, next);
            lane += 1;
        }
        index += 1;
    }
    let last = lanes - 1;
    let mut position = lanes * segment;
    while position < size
        invariant
            input.after(*old(input)),
            decodable(&tables),
            states.len() == lanes,
            contexts.len() == lanes,
            last < lanes,
            forall|lane: int| 0 <= lane < lanes ==> #[trigger] contexts[lane] < 256,
            out.len() == size,
            position <= size,
        decreases size - position,
    {
        let table = tables[contexts[last]].as_ref()?;
        let (symbol, next) = table.step(states[last]);
        out.set(position, symbol);
        contexts.set(last, symbol as usize);
        let next = input.renorm(next)?;
        states.set(last, next);
        position += 1;
    }
    Some(out)
}

/// Literals with run symbols: each run symbol takes a run length (one less than its copies)
/// from the meta data, after the list of run symbols (a count, 0 for all 256, then symbols).
fn unrun(literals: &[u8], meta: &[u8], size: usize) -> (out: Option<Vec<u8>>)
    requires
        size <= LIMIT,
    ensures
        match out {
            Some(out) => out.len() <= size,
            None => true,
        },
{
    let mut meta = Input { data: meta, at: 0 };
    let first = meta.byte()?;
    let count: usize = if first == 0 {
        256
    } else {
        first as usize
    };
    let mut runs = [false; 256];
    let mut index: usize = 0;
    while index < count
        invariant
            meta.at <= meta.data.len(),
            count <= 256,
        decreases count - index,
    {
        let symbol = meta.byte()?;
        runs[symbol as usize] = true;
        index += 1;
    }
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut at: usize = 0;
    while at < literals.len()
        invariant
            meta.at <= meta.data.len(),
            out.len() <= size,
        decreases literals.len() - at,
    {
        let byte = literals[at];
        let more = if runs[byte as usize] {
            meta.uint7()?
        } else {
            0
        };
        if more >= size - out.len() {
            return None;
        }
        let end = out.len() + more + 1;
        while out.len() < end
            invariant
                out.len() <= end,
                end <= size,
            decreases end - out.len(),
        {
            out.push(byte);
        }
        at += 1;
    }
    Some(out)
}

/// Bytes that hold 8, 4 or 2 symbol numbers (1, 2 or 4 bits each, low bits first), or no data
/// when there is one symbol.
fn unpack(data: &[u8], map: &[u8], size: usize) -> (out: Option<Vec<u8>>)
    requires
        size <= LIMIT,
    ensures
        match out {
            Some(out) => out.len() == size,
            None => true,
        },
{
    let symbols = map.len();
    if symbols == 1 {
        return Some(vec![map[0]; size]);
    }
    if symbols == 0 || symbols > 16 {
        return None;
    }
    let (bits, per, mask): (u8, u8, u8) = if symbols == 2 {
        (1, 8, 0x1)
    } else if symbols <= 4 {
        (2, 4, 0x3)
    } else {
        (4, 2, 0xF)
    };
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut at: usize = 0;
    while at < data.len() && out.len() < size
        invariant
            out.len() <= size,
            bits == 1 && per == 8 || bits == 2 && per == 4 || bits == 4 && per == 2,
        decreases data.len() - at,
    {
        let byte = data[at];
        let mut slot: u8 = 0;
        while slot < per && out.len() < size
            invariant
                out.len() <= size,
                slot <= per,
                bits == 1 && per == 8 || bits == 2 && per == 4 || bits == 4 && per == 2,
            decreases per - slot,
        {
            let value = byte >> (slot * bits) & mask;
            if value as usize >= map.len() {
                return None;
            }
            out.push(map[value as usize]);
            slot += 1;
        }
        at += 1;
    }
    if out.len() != size {
        return None;
    }
    Some(out)
}

/// Byte `index` of part `part` of `count` parts goes to a place in an output of `size` bytes.
proof fn lemma_stripe_place(size: int, count: int, part: int, index: int)
    requires
        0 <= size,
        0 <= part < count,
        0 <= index < size / count + if size % count > part {
            1int
        } else {
            0int
        },
    ensures
        index * count + part < size,
{
    lemma_fundamental_div_mod(size, count);
    lemma_mod_bound(size, count);
    let q = size / count;
    lemma_mul_is_commutative(count, q);
    if size % count > part {
        lemma_mul_inequality(index, q, count);
    } else {
        lemma_mul_inequality(index, q - 1, count);
        lemma_mul_is_distributive_sub_other_way(count, q, 1);
    }
}

/// The bytes of a stream of `count` parts: part `part` has the bytes at `part`, `part` plus
/// `count`, and so on. Each part is a stream of its own.
fn stripe(input: &mut Input, size: usize, depth: usize) -> (out: Option<Vec<u8>>)
    requires
        old(input).at <= old(input).data.len(),
        size <= LIMIT,
        depth < DEPTH,
    ensures
        final(input).after(*old(input)),
        match out {
            Some(out) => out.len() == size,
            None => true,
        },
    decreases DEPTH - depth, 0int,
{
    let count = input.byte()? as usize;
    if count == 0 {
        return None;
    }
    let mut lengths: Vec<usize> = Vec::with_capacity(count);
    while lengths.len() < count
        invariant
            input.after(*old(input)),
            lengths.len() <= count,
        decreases count - lengths.len(),
    {
        let length = input.uint7()?;
        lengths.push(length);
    }
    let mut out: Vec<u8> = vec![0u8; size];
    let mut part: usize = 0;
    while part < count
        invariant
            input.after(*old(input)),
            out.len() == size,
            lengths.len() == count,
            1 <= count,
            part <= count,
            size <= LIMIT,
            depth < DEPTH,
        decreases count - part,
    {
        let mut wanted = size / count;
        if size % count > part {
            wanted += 1;
        }
        let data = input.take(lengths[part])?;
        let mut part_input = Input { data, at: 0 };
        let bytes = decode_from(&mut part_input, Some(wanted), depth + 1)?;
        if bytes.len() != wanted {
            return None;
        }
        let mut index: usize = 0;
        while index < wanted
            invariant
                out.len() == size,
                bytes.len() == wanted,
                wanted == size / count + if size % count > part {
                    1int
                } else {
                    0int
                },
                part < count,
                index <= wanted,
            decreases wanted - index,
        {
            proof {
                lemma_stripe_place(size as int, count as int, part as int, index as int);
            }
            out.set(index * count + part, bytes[index]);
            index += 1;
        }
        part += 1;
    }
    Some(out)
}

/// The bytes of a stream: its flags, its size when the flags do not give NOSZ, and the data
/// of the order 0 or order 1 codec or of CAT, after the PACK, RLE or STRIPE transforms. A
/// stripe at `DEPTH` levels of stripes gives nothing.
fn decode_from(input: &mut Input, size: Option<usize>, depth: usize) -> (out: Option<Vec<u8>>)
    requires
        old(input).at <= old(input).data.len(),
        depth <= DEPTH,
    ensures
        final(input).after(*old(input)),
        match out {
            Some(out) => out.len() <= LIMIT,
            None => true,
        },
    decreases DEPTH - depth, 1int,
{
    let flags = input.byte()?;
    let size = if flags & NOSZ == 0 {
        input.uint7()?
    } else {
        size?
    };
    if size > LIMIT {
        return None;
    }
    if flags & STRIPE != 0 {
        if depth >= DEPTH {
            return None;
        }
        return stripe(input, size, depth);
    }
    let mut length = size;
    let mut packed: Option<(Vec<u8>, usize)> = None;
    if flags & PACK != 0 {
        let count = input.byte()? as usize;
        let map = slice_to_vec(input.take(count)?);
        packed = Some((map, length));
        length = input.uint7()?;
        if length > LIMIT {
            return None;
        }
    }
    let mut runs: Option<(Vec<u8>, usize)> = None;
    if flags & RLE != 0 {
        let meta_size = input.uint7()?;
        let literals = input.uint7()?;
        if meta_size / 2 > LIMIT {
            return None;
        }
        let meta = if meta_size & 1 != 0 {
            slice_to_vec(input.take(meta_size / 2)?)
        } else {
            let compressed = input.uint7()?;
            let part = input.take(compressed)?;
            let mut part_input = Input { data: part, at: 0 };
            order0(&mut part_input, meta_size / 2, 4)?
        };
        runs = Some((meta, length));
        length = literals;
    }
    if length > LIMIT {
        return None;
    }
    let lanes: usize = if flags & X32 != 0 {
        32
    } else {
        4
    };
    let mut data = if flags & CAT != 0 {
        slice_to_vec(input.take(length)?)
    } else if flags & ORDER != 0 {
        order1(input, length, lanes)?
    } else {
        order0(input, length, lanes)?
    };
    if let Some((meta, length)) = runs {
        data = unrun(data.as_slice(), meta.as_slice(), length)?;
    }
    if let Some((map, length)) = packed {
        data = unpack(data.as_slice(), map.as_slice(), length)?;
    }
    if data.len() != size {
        return None;
    }
    Some(data)
}

} // verus!

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

    /// Seven bits for each byte, the high groups first, and the top bit set on each byte but
    /// the last.
    fn uint7(value: usize) -> Vec<u8> {
        let mut bytes = vec![(value & 0x7F) as u8];
        let mut rest = value >> 7;
        while rest > 0 {
            bytes.push((rest & 0x7F) as u8 | 0x80);
            rest >>= 7;
        }
        bytes.reverse();
        bytes
    }

    /// A stripe holds stripes to 4 levels or fewer. More levels, as a damaged file can give,
    /// are an error, not an overflow of the stack.
    #[test]
    fn stripes_in_stripes_stop_at_four_levels() {
        fn nested(levels: usize) -> Vec<u8> {
            let inner = [0x20, 1, b'A'];
            let mut length = inner.len();
            let mut headers = Vec::new();
            for _ in 0..levels {
                let mut header = vec![0x08, 1, 1];
                header.extend(uint7(length));
                length += header.len();
                headers.push(header);
            }
            let mut block: Vec<u8> = headers.into_iter().rev().flatten().collect();
            block.extend(inner);
            block
        }
        assert_eq!(decode(&nested(4), None).unwrap(), b"A");
        assert!(decode(&nested(5), None).is_err());
        assert!(decode(&nested(100_000), None).is_err());
    }

    /// Sizes past the limit of 2 to the power of 30 bytes, as a damaged file can give, are
    /// errors: genie does not make an output of that size.
    #[test]
    fn sizes_past_the_limit_are_errors() {
        let past = (1 << 30) + 1;
        // PACK and RLE: the size after RLE comes after the symbols of PACK.
        let mut block = vec![0xE0, 4, 1, b'T'];
        block.extend(uint7(past));
        let meta = [1, b'A', 1];
        block.extend(uint7(meta.len() * 2 + 1));
        block.push(1);
        block.extend(meta);
        block.push(b'A');
        assert!(decode(&block, None).is_err());
        // An order-0 stream with one symbol, which each state gives again and again.
        let mut stream = vec![b'Q', 0, 1];
        stream.extend(states(4, 1 << 15));
        // RLE with compressed meta data.
        let mut block = vec![0x60, 4];
        block.extend(uint7(past * 2));
        block.push(1);
        block.extend(uint7(stream.len()));
        block.extend(&stream);
        block.push(b'A');
        assert!(decode(&block, None).is_err());
        // Order 1 with compressed tables.
        let mut block = vec![0x01, 4, 0xC1];
        block.extend(uint7(past));
        block.extend(uint7(stream.len()));
        block.extend(&stream);
        block.extend(states(4, 1 << 15));
        assert!(decode(&block, None).is_err());
    }
}
