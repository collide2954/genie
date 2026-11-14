//! The rANS 4x8 codec of CRAM 3.0 (block method 4), order 0 and order 1.

use super::bits::bad;
use crate::Error;

const TOTAL: u32 = 1 << SHIFT;
const SHIFT: u32 = 12;
const LOW: u32 = 1 << 23;

pub(crate) fn decode(input: &[u8]) -> Result<Vec<u8>, Error> {
    let [order, _, _, _, _, a, b, c, d, data @ ..] = input else {
        return Err(bad());
    };
    let size = u32::from_le_bytes([*a, *b, *c, *d]) as usize;
    let mut input = Input { data, at: 0 };
    match order {
        0 => order0(&mut input, size),
        1 => order1(&mut input, size),
        _ => Err(bad()),
    }
}

struct Input<'a> {
    data: &'a [u8],
    at: usize,
}

impl Input<'_> {
    fn byte(&mut self) -> Result<u8, Error> {
        let byte = *self.data.get(self.at).ok_or_else(bad)?;
        self.at += 1;
        Ok(byte)
    }

    fn peek(&self) -> Result<u8, Error> {
        self.data.get(self.at).copied().ok_or_else(bad)
    }

    fn state(&mut self) -> Result<u32, Error> {
        let mut word = [0; 4];
        for byte in &mut word {
            *byte = self.byte()?;
        }
        Ok(u32::from_le_bytes(word))
    }

    fn states(&mut self) -> Result<[u32; 4], Error> {
        Ok([self.state()?, self.state()?, self.state()?, self.state()?])
    }

    fn renorm(&mut self, state: &mut u32) -> Result<(), Error> {
        while *state < LOW {
            *state = (*state << 8) | u32::from(self.byte()?);
        }
        Ok(())
    }
}

struct Table {
    freq: [u32; 256],
    start: [u32; 256],
    lookup: Vec<u8>,
}

impl Table {
    fn read(input: &mut Input, zero_is_total: bool) -> Result<Self, Error> {
        let mut table = Self {
            freq: [0; 256],
            start: [0; 256],
            lookup: vec![0; TOTAL as usize],
        };
        let mut total = 0;
        each_symbol(input, |input, symbol| {
            let mut freq = u32::from(input.byte()?);
            if freq >= 128 {
                freq = ((freq & 127) << 8) | u32::from(input.byte()?);
            }
            if zero_is_total && freq == 0 {
                freq = TOTAL;
            }
            let end = total + freq;
            if end > TOTAL {
                return Err(bad());
            }
            table.freq[symbol] = freq;
            table.start[symbol] = total;
            table.lookup[total as usize..end as usize].fill(symbol as u8);
            total = end;
            Ok(())
        })?;
        Ok(table)
    }

    fn step(&self, state: &mut u32) -> u8 {
        let low = *state & (TOTAL - 1);
        let symbol = self.lookup[low as usize];
        let index = usize::from(symbol);
        *state = self.freq[index] * (*state >> SHIFT) + low - self.start[index];
        symbol
    }
}

/// Walks the run-length coded symbol list of a frequency table: a symbol that follows the one
/// before is written with the count of further symbols in the run, and a 0 ends the list.
fn each_symbol(
    input: &mut Input,
    mut each: impl FnMut(&mut Input, usize) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut symbol = usize::from(input.byte()?);
    let mut run = 0;
    loop {
        each(input, symbol)?;
        if run == 0 && symbol + 1 == usize::from(input.peek()?) {
            symbol = usize::from(input.byte()?);
            run = input.byte()?;
        } else if run > 0 {
            run -= 1;
            symbol += 1;
        } else {
            symbol = usize::from(input.byte()?);
        }
        if symbol == 0 || symbol > 255 {
            return if symbol == 0 { Ok(()) } else { Err(bad()) };
        }
    }
}

fn order0(input: &mut Input, size: usize) -> Result<Vec<u8>, Error> {
    let table = Table::read(input, false)?;
    let mut states = input.states()?;
    let mut out = vec![0; size];
    let whole = size & !3;
    for quad in out[..whole].as_chunks_mut::<4>().0 {
        for (byte, state) in quad.iter_mut().zip(&mut states) {
            *byte = table.step(state);
        }
        for state in &mut states {
            input.renorm(state)?;
        }
    }
    for (byte, state) in out[whole..].iter_mut().zip(&mut states) {
        *byte = table.step(state);
        input.renorm(state)?;
    }
    Ok(out)
}

fn order1(input: &mut Input, size: usize) -> Result<Vec<u8>, Error> {
    let mut tables: Vec<Option<Table>> = (0..256).map(|_| None).collect();
    each_symbol(input, |input, context| {
        tables[context] = Some(Table::read(input, true)?);
        Ok(())
    })?;
    let mut states = input.states()?;
    let mut out = vec![0; size];
    let quarter = size / 4;
    let mut contexts = [0_usize; 4];
    for index in 0..quarter {
        for (lane, state) in states.iter_mut().enumerate() {
            let table = tables[contexts[lane]].as_ref().ok_or_else(bad)?;
            let symbol = table.step(state);
            out[lane * quarter + index] = symbol;
            contexts[lane] = usize::from(symbol);
        }
        for state in &mut states {
            input.renorm(state)?;
        }
    }
    for byte in &mut out[4 * quarter..] {
        let table = tables[contexts[3]].as_ref().ok_or_else(bad)?;
        *byte = table.step(&mut states[3]);
        input.renorm(&mut states[3])?;
        contexts[3] = usize::from(*byte);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn broken_streams_are_errors() {
        assert!(decode(&[]).is_err());
        assert!(decode(&stream(2, 4, &[0])).is_err());
        assert!(decode(&stream(0, 4, &[b'A', 0x90, 0x00, 0, 1, 2])).is_err());
    }
}
