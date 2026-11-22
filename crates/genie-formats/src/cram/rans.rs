//! The rANS 4x8 codec of CRAM 3.0 (block method 4), order 0 and order 1.

use vstd::prelude::*;

use super::bits::{LIMIT, bad};
use crate::Error;

pub(crate) fn decode(input: &[u8]) -> Result<Vec<u8>, Error> {
    let [order, _, _, _, _, a, b, c, d, data @ ..] = input else {
        return Err(bad());
    };
    let size = u32::from_le_bytes([*a, *b, *c, *d]) as usize;
    stream(*order, size, data).ok_or_else(bad)
}
verus! {

/// The `size` bytes of the data of a stream of order 0 or 1. A size past `LIMIT`, as a damaged
/// file can give, gives nothing.
fn stream(order: u8, size: usize, data: &[u8]) -> (out: Option<Vec<u8>>)
    ensures
        match out {
            Some(out) => out.len() == size && size <= LIMIT,
            None => true,
        },
{
    if size > LIMIT {
        return None;
    }
    let mut input = Input { data, at: 0 };
    match order {
        0 => order0(&mut input, size),
        1 => order1(&mut input, size),
        _ => None,
    }
}

/// The total of the frequencies of a table, 2 to the power of `SHIFT`.
const TOTAL: u32 = 4096;
const SHIFT: u32 = 12;
/// The lowest state after a renormalization: 2 to the power of 23.
const LOW: u32 = 0x80_0000;

/// The bytes of a stream and the place of the next byte.
struct Input<'a> {
    data: &'a [u8],
    at: usize,
}

impl Input<'_> {
    fn byte(&mut self) -> (byte: Option<u8>)
        requires
            old(self).at <= old(self).data.len(),
        ensures
            final(self).data == old(self).data,
            final(self).at <= final(self).data.len(),
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

    /// A state of the decoder: 4 bytes, the low byte first.
    fn state(&mut self) -> (state: Option<u32>)
        requires
            old(self).at <= old(self).data.len(),
        ensures
            final(self).data == old(self).data,
            final(self).at <= final(self).data.len(),
    {
        let first = self.byte()?;
        let second = self.byte()?;
        let third = self.byte()?;
        let fourth = self.byte()?;
        Some(u32::from(first) | u32::from(second) << 8 | u32::from(third) << 16 | u32::from(fourth)
            << 24)
    }

    /// A state with bytes after it until it is `LOW` or more.
    fn renorm(&mut self, state: u32) -> (renormed: Option<u32>)
        requires
            old(self).at <= old(self).data.len(),
        ensures
            final(self).data == old(self).data,
            final(self).at <= final(self).data.len(),
    {
        let mut state = state;
        while state < LOW
            invariant
                self.data == old(self).data,
                self.at <= self.data.len(),
            decreases self.data.len() - self.at,
        {
            let byte = self.byte()?;
            assert(state << 8 | byte as u32 >= state) by (bit_vector)
                requires
                    state < 0x80_0000,
            ;
            state = state << 8 | u32::from(byte);
        }
        Some(state)
    }
}

/// A frequency table: the frequency and the first place of each symbol, and the symbol of each
/// of the `TOTAL` places.
struct Table {
    freq: [u32; 256],
    start: [u32; 256],
    lookup: Vec<u8>,
}

impl Table {
    /// A table can decode: a symbol for each place, frequencies of `TOTAL` or less, and the
    /// first place of the symbol of each place at or before the place.
    spec fn decodes(&self) -> bool {
        &&& self.lookup.len() == TOTAL
        &&& forall|symbol: int| 0 <= symbol < 256 ==> #[trigger] self.freq[symbol] <= TOTAL
        &&& forall|place: int|
            0 <= place < TOTAL ==> self.start[#[trigger] self.lookup[place] as int] <= place
    }

    /// The symbol of a state and the next state.
    fn step(&self, state: u32) -> (found: (u8, u32))
        requires
            self.decodes(),
    {
        let low = state & (TOTAL - 1);
        assert(low < 4096) by (bit_vector)
            requires
                low == state & 4095,
        ;
        let symbol = self.lookup[low as usize];
        let index = symbol as usize;
        let high = state >> SHIFT;
        assert(high < 0x10_0000) by (bit_vector)
            requires
                high == state >> 12,
        ;
        let freq = self.freq[index];
        assert(freq * high <= 4096 * 0xF_FFFF) by (nonlinear_arith)
            requires
                freq <= 4096,
                high <= 0xF_FFFF,
        ;
        (symbol, freq * high + low - self.start[index])
    }

    /// The table of a list of symbols with their frequencies. A frequency of 0 stands for all
    /// of `TOTAL` when `zero_is_total`. A symbol that comes again gives nothing.
    fn read(input: &mut Input, zero_is_total: bool) -> (table: Option<Table>)
        requires
            old(input).at <= old(input).data.len(),
        ensures
            final(input).data == old(input).data,
            final(input).at <= final(input).data.len(),
            table is Some ==> final(input).at > old(input).at,
            match table {
                Some(table) => table.decodes(),
                None => true,
            },
    {
        let mut freq = [0u32; 256];
        let mut start = [0u32; 256];
        let mut seen = [false; 256];
        let mut lookup: Vec<u8> = Vec::with_capacity(TOTAL as usize);
        while lookup.len() < TOTAL as usize
            invariant
                lookup.len() <= TOTAL,
                forall|place: int| 0 <= place < lookup.len() ==> #[trigger] lookup[place] == 0,
            decreases TOTAL - lookup.len(),
        {
            lookup.push(0);
        }
        let ghost begin = input.at;
        let mut total: u32 = 0;
        let mut symbol = input.byte()? as usize;
        let mut run: u8 = 0;
        loop
            invariant_except_break
                symbol == 0 ==> total == 0,
            invariant
                input.data == old(input).data,
                input.at <= input.data.len(),
                input.at > begin,
                begin == old(input).at,
                symbol < 256,
                total <= TOTAL,
                lookup.len() == TOTAL,
                forall|other: int| 0 <= other < 256 ==> #[trigger] freq[other] <= TOTAL,
                forall|place: int| 0 <= place < total ==> seen[#[trigger] lookup[place] as int],
                forall|place: int| total <= place < TOTAL ==> #[trigger] lookup[place] == 0,
                forall|place: int| 0 <= place < TOTAL ==> start[#[trigger] lookup[place] as int] <= place,
            decreases input.data.len() - input.at,
        {
            if seen[symbol] {
                return None;
            }
            let mut value = u32::from(input.byte()?);
            if value >= 128 {
                value = (value & 127) << 8 | u32::from(input.byte()?);
            }
            if zero_is_total && value == 0 {
                value = TOTAL;
            }
            if value > TOTAL - total {
                return None;
            }
            let end = total + value;
            freq[symbol] = value;
            start[symbol] = total;
            seen[symbol] = true;
            let mut place = total as usize;
            while place < end as usize
                invariant
                    total <= place <= end <= TOTAL,
                    lookup.len() == TOTAL,
                    start[symbol as int] == total,
                    symbol < 256,
                    seen[symbol as int],
                    forall|other: int| place <= other < TOTAL ==> #[trigger] lookup[other] == 0,
                    forall|other: int| 0 <= other < place ==> seen[#[trigger] lookup[other] as int],
                    forall|other: int| 0 <= other < TOTAL ==> start[#[trigger] lookup[other] as int] <= other,
                decreases end - place,
            {
                lookup[place] = symbol as u8;
                place += 1;
            }
            total = end;
            let (next, more) = next_symbol(input, symbol, run)?;
            if next == 0 {
                break;
            }
            if next > 255 {
                return None;
            }
            symbol = next;
            run = more;
        }
        Some(Table { freq, start, lookup })
    }
}

/// The symbol after `symbol` in a list of symbols with runs, and the more symbols of its run: a
/// symbol that follows the one before comes with the number of more symbols in its run.
fn next_symbol(input: &mut Input, symbol: usize, run: u8) -> (next: Option<(usize, u8)>)
    requires
        old(input).at <= old(input).data.len(),
        symbol < 256,
    ensures
        final(input).data == old(input).data,
        final(input).at <= final(input).data.len(),
        final(input).at >= old(input).at,
        match next {
            Some((next, _)) => next <= 256,
            None => true,
        },
{
    if run == 0 && symbol < 255 && input.peek() == Some((symbol + 1) as u8) {
        let next = input.byte()? as usize;
        let more = input.byte()?;
        Some((next, more))
    } else if run > 0 {
        Some((symbol + 1, run - 1))
    } else {
        Some((input.byte()? as usize, 0))
    }
}

/// The bytes of an order-0 stream of `size` bytes: one table, and 4 states that take turns.
fn order0(input: &mut Input, size: usize) -> (out: Option<Vec<u8>>)
    requires
        old(input).at <= old(input).data.len(),
        size <= LIMIT,
    ensures
        match out {
            Some(out) => out.len() == size,
            None => true,
        },
{
    let table = Table::read(input, false)?;
    let first = input.state()?;
    let second = input.state()?;
    let third = input.state()?;
    let fourth = input.state()?;
    let mut states = [first, second, third, fourth];
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let whole = size - size % 4;
    while out.len() < whole
        invariant
            table.decodes(),
            input.at <= input.data.len(),
            whole == size - size % 4,
            out.len() <= whole,
            out.len() % 4 == 0,
        decreases whole - out.len(),
    {
        let ghost before = out.len();
        let mut lane: usize = 0;
        while lane < 4
            invariant
                table.decodes(),
                lane <= 4,
                out.len() == before + lane,
            decreases 4 - lane,
        {
            let (symbol, next) = table.step(states[lane]);
            out.push(symbol);
            states[lane] = next;
            lane += 1;
        }
        let mut lane: usize = 0;
        while lane < 4
            invariant
                input.at <= input.data.len(),
                lane <= 4,
            decreases 4 - lane,
        {
            states[lane] = input.renorm(states[lane])?;
            lane += 1;
        }
    }
    let mut lane: usize = 0;
    while out.len() < size
        invariant
            table.decodes(),
            input.at <= input.data.len(),
            whole == size - size % 4,
            out.len() == whole + lane,
            out.len() <= size,
        decreases size - out.len(),
    {
        let (symbol, next) = table.step(states[lane]);
        out.push(symbol);
        states[lane] = input.renorm(next)?;
        lane += 1;
    }
    Some(out)
}

/// The tables of an order-1 stream: a list of the bytes before, each with its table.
fn contexts(input: &mut Input) -> (tables: Option<Vec<Option<Table>>>)
    requires
        old(input).at <= old(input).data.len(),
    ensures
        final(input).data == old(input).data,
        final(input).at <= final(input).data.len(),
        match tables {
            Some(tables) => tables.len() == 256 && forall|known: int|
                0 <= known < 256 ==> match #[trigger] tables[known] {
                    Some(table) => table.decodes(),
                    None => true,
                },
            None => true,
        },
{
    let mut tables: Vec<Option<Table>> = Vec::new();
    while tables.len() < 256
        invariant
            tables.len() <= 256,
            forall|context: int| 0 <= context < tables.len() ==> #[trigger] tables[context] is None,
        decreases 256 - tables.len(),
    {
        tables.push(None);
    }
    let mut context = input.byte()? as usize;
    let mut run: u8 = 0;
    loop
        invariant
            input.data == old(input).data,
            input.at <= input.data.len(),
            context < 256,
            tables.len() == 256,
            forall|known: int| 0 <= known < 256 ==> match #[trigger] tables[known] {
                Some(table) => table.decodes(),
                None => true,
            },
        decreases input.data.len() - input.at,
    {
        let table = Table::read(input, true)?;
        tables.set(context, Some(table));
        let (next, more) = next_symbol(input, context, run)?;
        if next == 0 {
            break;
        }
        if next > 255 {
            return None;
        }
        context = next;
        run = more;
    }
    Some(tables)
}

/// The bytes of an order-1 stream of `size` bytes: a table for each byte before, and 4 states,
/// each for a quarter of the bytes. The last state also takes the bytes after the quarters.
fn order1(input: &mut Input, size: usize) -> (out: Option<Vec<u8>>)
    requires
        old(input).at <= old(input).data.len(),
        size <= LIMIT,
    ensures
        match out {
            Some(out) => out.len() == size,
            None => true,
        },
{
    let tables = contexts(input)?;
    let first = input.state()?;
    let second = input.state()?;
    let third = input.state()?;
    let fourth = input.state()?;
    let mut states = [first, second, third, fourth];
    let quarter = size / 4;
    let mut out: Vec<u8> = Vec::with_capacity(size);
    while out.len() < size
        invariant
            out.len() <= size,
        decreases size - out.len(),
    {
        out.push(0);
    }
    let mut contexts = [0usize; 4];
    let mut index: usize = 0;
    while index < quarter
        invariant
            input.at <= input.data.len(),
            tables.len() == 256,
            forall|known: int| 0 <= known < 256 ==> match #[trigger] tables[known] {
                Some(table) => table.decodes(),
                None => true,
            },
            forall|lane: int| 0 <= lane < 4 ==> #[trigger] contexts[lane] < 256,
            quarter == size / 4,
            out.len() == size,
            index <= quarter,
        decreases quarter - index,
    {
        let mut lane: usize = 0;
        while lane < 4
            invariant
                tables.len() == 256,
                forall|known: int| 0 <= known < 256 ==> match #[trigger] tables[known] {
                    Some(table) => table.decodes(),
                    None => true,
                },
                forall|other: int| 0 <= other < 4 ==> #[trigger] contexts[other] < 256,
                quarter == size / 4,
                out.len() == size,
                index < quarter,
                lane <= 4,
            decreases 4 - lane,
        {
            let table = tables[contexts[lane]].as_ref()?;
            let (symbol, next) = table.step(states[lane]);
            assert(lane * quarter + index < 4 * quarter) by (nonlinear_arith)
                requires
                    lane < 4,
                    index < quarter,
            ;
            out.set(lane * quarter + index, symbol);
            states[lane] = next;
            contexts[lane] = symbol as usize;
            lane += 1;
        }
        let mut lane: usize = 0;
        while lane < 4
            invariant
                input.at <= input.data.len(),
                lane <= 4,
            decreases 4 - lane,
        {
            states[lane] = input.renorm(states[lane])?;
            lane += 1;
        }
        index += 1;
    }
    let mut position = 4 * quarter;
    while position < size
        invariant
            input.at <= input.data.len(),
            tables.len() == 256,
            forall|known: int| 0 <= known < 256 ==> match #[trigger] tables[known] {
                Some(table) => table.decodes(),
                None => true,
            },
            forall|lane: int| 0 <= lane < 4 ==> #[trigger] contexts[lane] < 256,
            out.len() == size,
            position <= size,
        decreases size - position,
    {
        let table = tables[contexts[3]].as_ref()?;
        let (symbol, next) = table.step(states[3]);
        out.set(position, symbol);
        states[3] = input.renorm(next)?;
        contexts[3] = symbol as usize;
        position += 1;
    }
    Some(out)
}

} // verus!

#[cfg(test)]
mod tests {
    use super::super::bits::LIMIT;
    use super::decode;

    fn stream(order: u8, size: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = vec![order];
        bytes.extend((body.len() as u32).to_le_bytes());
        bytes.extend(size.to_le_bytes());
        bytes.extend(body);
        bytes
    }

    const STATE: [u8; 4] = [0, 0, 0x80, 0];

    #[test]
    fn one_symbol_fills_the_output() {
        let mut body = vec![b'A', 0x90, 0x00, 0];
        for _ in 0..4 {
            body.extend(STATE);
        }
        assert_eq!(decode(&stream(0, 11, &body)).unwrap(), b"AAAAAAAAAAA");
        assert_eq!(decode(&stream(0, 0, &body)).unwrap(), b"");
    }

    #[test]
    fn two_symbols_follow_the_states() {
        // A and B at half of the total each; B follows A, so it comes with a run length of 0.
        // A state picks A when its low 12 bits are below 2048, and halves to 2^23.
        let mut body = vec![b'A', 0x88, 0x00, b'B', 0, 0x88, 0x00, 0];
        for state in [0x0100_0000_u32, 0x0100_0800, 0x0100_0000, 0x0100_0800] {
            body.extend(state.to_le_bytes());
        }
        assert_eq!(decode(&stream(0, 4, &body)).unwrap(), b"ABAB");
    }

    #[test]
    fn order_one_uses_the_symbol_before() {
        // Context 0 always gives C, context C always gives C.
        let mut body = vec![0, b'C', 0x90, 0x00, 0, b'C', b'C', 0x90, 0x00, 0, 0];
        for _ in 0..4 {
            body.extend(STATE);
        }
        assert_eq!(decode(&stream(1, 9, &body)).unwrap(), b"CCCCCCCCC");
    }

    /// A frequency table that gives a symbol two times, as a damaged file can give, is an error,
    /// not an underflow: the second range of A starts at 2048, and a state of 0 reads the first.
    #[test]
    fn symbols_that_come_again_are_errors() {
        let mut body = vec![b'A', 0x88, 0x00, b'A', 0x88, 0x00, 0];
        body.extend([0; 16]);
        body.extend([0x80; 64]);
        assert!(decode(&stream(0, 4, &body)).is_err());
    }

    /// A size past the limit, as a damaged file can give, is an error: genie does not make an
    /// output of that size.
    #[test]
    fn sizes_past_the_limit_are_errors() {
        let mut body = vec![b'A', 0x90, 0x00, 0];
        for _ in 0..4 {
            body.extend(STATE);
        }
        let past = u32::try_from(LIMIT + 1).unwrap();
        assert!(decode(&stream(0, past, &body)).is_err());
        assert!(decode(&stream(1, past, &body)).is_err());
    }

    #[test]
    fn broken_streams_are_errors() {
        assert!(decode(&[]).is_err());
        assert!(decode(&stream(2, 4, &[0])).is_err());
        assert!(decode(&stream(0, 4, &[b'A', 0x90, 0x00, 0, 1, 2])).is_err());
    }
}
