//! The encodings of CRAM data series: values from the core bit stream or from external blocks.

use std::collections::HashMap;

#[cfg(verus_keep_ghost)]
use vstd::arithmetic::power2::{
    lemma_pow2_strictly_increases, lemma_pow2_unfold, lemma2_to64, pow2,
};
#[cfg(verus_keep_ghost)]
use vstd::bits::{lemma_u32_pow2_no_overflow, lemma_u32_shl_is_mul};
use vstd::prelude::*;

use super::bits::{Bits, Bytes, bad};
#[cfg(verus_keep_ghost)]
use super::bits::{spec_bit, spec_bits};
use crate::Error;

verus! {

/// The canonical Huffman codes of a data series: the bit length, the code and the symbol of
/// each code, the shorter codes first.
#[derive(Debug, Clone, PartialEq, Eq)]
#[verifier::allow(autoderive_clone_without_spec)]
pub(crate) struct Huffman {
    codes: Vec<(u32, u32, i32)>,
}

impl Huffman {
    /// Each code has 31 bits or fewer, and no code is shorter than the code before it.
    pub(crate) closed spec fn valid(&self) -> bool {
        &&& forall|index: int|
            0 <= index < self.codes.len() ==> #[trigger] self.codes[index].0 <= 31
        &&& forall|index: int|
            0 < index < self.codes.len() ==> #[trigger] self.codes[index].0 >= self.codes[index
                - 1].0
    }

    /// The symbol of the first code whose bits come next in the stream, and the place after
    /// its bits. Nothing when no code fits, or when the stream stops before the bits.
    fn decode(&self, bits: &mut Bits) -> (symbol: Option<i32>)
        requires
            self.valid(),
            old(bits).valid(),
        ensures
            final(bits).valid(),
            final(bits).bytes() == old(bits).bytes(),
            match symbol {
                Some(symbol) => exists|index: int|
                    #![trigger self.codes[index]]
                    0 <= index < self.codes.len() && self.codes[index].2 == symbol
                        && self.codes[index].1 == spec_bits(
                        old(bits).bytes(),
                        old(bits).place(),
                        self.codes[index].0 as nat,
                    ) && final(bits).place() == old(bits).place() + self.codes[index].0
                        && forall|other: int|
                        0 <= other < index ==> #[trigger] self.codes[other].1 != spec_bits(
                            old(bits).bytes(),
                            old(bits).place(),
                            self.codes[other].0 as nat,
                        ),
                None => true,
            },
    {
        let ghost start = bits.place();
        let ghost bytes = bits.bytes();
        let mut code: u32 = 0;
        let mut length: u32 = 0;
        let mut index: usize = 0;
        proof {
            lemma2_to64();
        }
        while index < self.codes.len()
            invariant
                self.valid(),
                bits.valid(),
                bits.bytes() == bytes,
                bytes == old(bits).bytes(),
                start == old(bits).place(),
                bits.place() == start + length,
                index <= self.codes.len(),
                length == if index == 0 {
                    0
                } else {
                    self.codes[index - 1].0
                },
                length <= 31,
                code == spec_bits(bytes, start, length as nat),
                code < pow2(length as nat),
                forall|other: int|
                    0 <= other < index ==> #[trigger] self.codes[other].1 != spec_bits(
                        bytes,
                        start,
                        self.codes[other].0 as nat,
                    ),
            decreases self.codes.len() - index,
        {
            let (size, value, symbol) = self.codes[index];
            while length < size
                invariant
                    bits.valid(),
                    bits.bytes() == bytes,
                    bytes == old(bits).bytes(),
                    bits.place() == start + length,
                    length <= size,
                    size <= 31,
                    code == spec_bits(bytes, start, length as nat),
                    code < pow2(length as nat),
                decreases size - length,
            {
                let bit = bits.bit()?;
                proof {
                    lemma2_to64();
                    lemma_pow2_unfold((length + 1) as nat);
                    lemma_pow2_strictly_increases(length as nat, 31);
                }
                code = code * 2 + bit;
                length += 1;
            }
            if code == value {
                assert(self.codes[index as int].1 == spec_bits(
                    bytes,
                    start,
                    self.codes[index as int].0 as nat,
                ));
                return Some(symbol);
            }
            index += 1;
        }
        None
    }
}

/// Canonical codes for symbols with their bit lengths, the shorter codes first, as CRAM gives
/// them: within a length the codes count up, and a longer length starts at the next code of
/// the length before, with 0 bits after it. Lengths of more than 31 bits, lengths out of
/// order, or more codes than 32 bits can count, give nothing.
fn canonical(pairs: &[(u32, i32)]) -> (huffman: Option<Huffman>)
    ensures
        match huffman {
            Some(huffman) => huffman.valid() && huffman.codes.len() == pairs.len() && forall|
                index: int,
            |
                0 <= index < pairs.len() ==> #[trigger] huffman.codes[index].0 == pairs[index].0
                    && huffman.codes[index].2 == pairs[index].1,
            None => true,
        },
{
    let mut codes: Vec<(u32, u32, i32)> = Vec::with_capacity(pairs.len());
    let mut code: u32 = 0;
    let mut last: u32 = 0;
    let mut index: usize = 0;
    while index < pairs.len()
        invariant
            index <= pairs.len(),
            codes.len() == index,
            last <= 31,
            index > 0 ==> codes[index - 1].0 == last,
            forall|other: int| 0 <= other < index ==> #[trigger] codes[other].0 <= 31,
            forall|other: int|
                0 < other < index ==> #[trigger] codes[other].0 >= codes[other - 1].0,
            forall|other: int|
                0 <= other < index ==> #[trigger] codes[other].0 == pairs[other].0
                    && codes[other].2 == pairs[other].1,
        decreases pairs.len() - index,
    {
        let (length, symbol) = pairs[index];
        if length > 31 || length < last {
            return None;
        }
        if index > 0 {
            code = code.checked_add(1)?;
        }
        if length > last {
            code <<= length - last;
            last = length;
        }
        codes.push((length, code, symbol));
        index += 1;
    }
    Some(Huffman { codes })
}

/// The number of a gamma code: `zeros` 0 bits, then a 1 bit and `zeros` more bits, as the
/// number 2 to the power of `zeros` plus the number of the bits after the 1 bit. More than 31
/// zeros, or a stream that stops before the bits, give nothing.
fn gamma(bits: &mut Bits) -> (value: Option<u32>)
    requires
        old(bits).valid(),
    ensures
        final(bits).valid(),
        final(bits).bytes() == old(bits).bytes(),
        match value {
            Some(value) => exists|zeros: nat|
                #![trigger spec_bits(old(bits).bytes(), old(bits).place(), zeros)]
                zeros <= 31 && spec_bits(old(bits).bytes(), old(bits).place(), zeros) == 0
                    && spec_bit(old(bits).bytes(), old(bits).place() + zeros) == 1 && value
                    == pow2(zeros) + spec_bits(
                    old(bits).bytes(),
                    old(bits).place() + zeros + 1,
                    zeros,
                ) && final(bits).place() == old(bits).place() + 2 * zeros + 1,
            None => true,
        },
{
    let ghost start = bits.place();
    let ghost bytes = bits.bytes();
    let mut zeros: u32 = 0;
    let mut bit = bits.bit()?;
    while bit == 0
        invariant
            bits.valid(),
            bits.bytes() == bytes,
            bytes == old(bits).bytes(),
            start == old(bits).place(),
            zeros <= 31,
            bits.place() == start + zeros + 1,
            spec_bits(bytes, start, zeros as nat) == 0,
            bit == spec_bit(bytes, start + zeros),
            bit <= 1,
        decreases 31 - zeros,
    {
        if zeros == 31 {
            return None;
        }
        zeros += 1;
        bit = bits.bit()?;
    }
    let rest = bits.bits(zeros)?;
    proof {
        lemma_u32_pow2_no_overflow(zeros as nat);
        lemma_u32_shl_is_mul(1, zeros);
    }
    let high = 1u32 << zeros;
    assert(high | rest == high + rest) by (bit_vector)
        requires
            rest < high,
            high == 1u32 << zeros,
            zeros < 32,
    ;
    assert(spec_bits(bytes, start, zeros as nat) == 0);
    Some(high | rest)
}

/// The number of a subexponential code with parameter `k`: `ones` 1 bits and a 0 bit, then
/// with no ones the number of the next `k` bits, and with ones the number 2 to the power of
/// `k + ones - 1` plus the number of the next `k + ones - 1` bits. More than 31 ones, numbers
/// of more than 32 bits, or a stream that stops before the bits, give nothing.
fn subexp(bits: &mut Bits, k: u32) -> (value: Option<u32>)
    requires
        old(bits).valid(),
    ensures
        final(bits).valid(),
        final(bits).bytes() == old(bits).bytes(),
        match value {
            Some(value) => exists|ones: nat|
                #![trigger spec_bits(old(bits).bytes(), old(bits).place(), ones)]
                ones <= 31 && spec_bits(old(bits).bytes(), old(bits).place(), ones) == pow2(ones)
                    - 1 && spec_bit(old(bits).bytes(), old(bits).place() + ones) == 0 && if ones
                    == 0 {
                    value == spec_bits(old(bits).bytes(), old(bits).place() + 1, k as nat)
                        && final(bits).place() == old(bits).place() + 1 + k
                } else {
                    k + ones - 1 < 32 && value == pow2((k + ones - 1) as nat) + spec_bits(
                        old(bits).bytes(),
                        old(bits).place() + ones + 1,
                        (k + ones - 1) as nat,
                    ) && final(bits).place() == old(bits).place() + 2 * ones + k
                },
            None => true,
        },
{
    let ghost start = bits.place();
    let ghost bytes = bits.bytes();
    let mut ones: u32 = 0;
    let mut bit = bits.bit()?;
    proof {
        lemma2_to64();
    }
    while bit == 1
        invariant
            bits.valid(),
            bits.bytes() == bytes,
            bytes == old(bits).bytes(),
            start == old(bits).place(),
            ones <= 31,
            bits.place() == start + ones + 1,
            spec_bits(bytes, start, ones as nat) == pow2(ones as nat) - 1,
            bit == spec_bit(bytes, start + ones),
            bit <= 1,
        decreases 31 - ones,
    {
        if ones == 31 {
            return None;
        }
        proof {
            lemma_pow2_unfold((ones + 1) as nat);
        }
        ones += 1;
        bit = bits.bit()?;
    }
    if ones == 0 {
        let value = bits.bits(k)?;
        assert(spec_bits(bytes, start, 0) == pow2(0) - 1);
        return Some(value);
    }
    let width = k.checked_add(ones - 1)?;
    if width >= 32 {
        return None;
    }
    let rest = bits.bits(width)?;
    proof {
        lemma_u32_pow2_no_overflow(width as nat);
        lemma_u32_shl_is_mul(1, width);
    }
    let high = 1u32 << width;
    assert(high | rest == high + rest) by (bit_vector)
        requires
            rest < high,
            high == 1u32 << width,
            width < 32,
    ;
    assert(spec_bits(bytes, start, ones as nat) == pow2(ones as nat) - 1);
    Some(high | rest)
}

} // verus!

impl Huffman {
    /// Canonical codes: sorted by bit length, then by symbol, counting up within each length.
    pub fn new(symbols: &[i32], lengths: &[u32]) -> Result<Self, Error> {
        if symbols.len() != lengths.len() || symbols.is_empty() {
            return Err(bad());
        }
        let mut pairs: Vec<(u32, i32)> = lengths
            .iter()
            .copied()
            .zip(symbols.iter().copied())
            .collect();
        pairs.sort_unstable();
        canonical(&pairs).ok_or_else(bad)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Encoding {
    Null,
    External(i32),
    Huffman(Huffman),
    ByteArrayLen(Box<Encoding>, Box<Encoding>),
    ByteArrayStop(u8, i32),
    Beta { offset: i32, bits: u32 },
    Subexp { offset: i32, k: u32 },
    Gamma { offset: i32 },
}

/// The core bit stream and the external blocks of a slice, each with its own read position.
pub(crate) struct Streams<'a> {
    pub core: Bits<'a>,
    pub external: HashMap<i32, Bytes<'a>>,
}

impl<'a> Streams<'a> {
    fn block(&mut self, id: i32) -> Result<&mut Bytes<'a>, Error> {
        self.external.get_mut(&id).ok_or_else(bad)
    }
}

fn count(bytes: &mut Bytes) -> Result<u32, Error> {
    u32::try_from(bytes.itf8()?).map_err(|_| bad())
}

impl Encoding {
    pub fn read(bytes: &mut Bytes) -> Result<Self, Error> {
        let codec = bytes.itf8()?;
        let size = bytes.size()?;
        let mut params = Bytes::new(bytes.take(size)?);
        Ok(match codec {
            0 => Self::Null,
            1 => Self::External(params.itf8()?),
            3 => {
                let symbols = (0..params.size()?)
                    .map(|_| params.itf8())
                    .collect::<Result<Vec<_>, _>>()?;
                let lengths = (0..params.size()?)
                    .map(|_| count(&mut params))
                    .collect::<Result<Vec<_>, _>>()?;
                Self::Huffman(Huffman::new(&symbols, &lengths)?)
            }
            4 => {
                let length = Self::read(&mut params)?;
                let values = Self::read(&mut params)?;
                Self::ByteArrayLen(Box::new(length), Box::new(values))
            }
            5 => Self::ByteArrayStop(params.byte()?, params.itf8()?),
            6 => Self::Beta {
                offset: params.itf8()?,
                bits: count(&mut params)?,
            },
            7 => Self::Subexp {
                offset: params.itf8()?,
                k: count(&mut params)?,
            },
            9 => Self::Gamma {
                offset: params.itf8()?,
            },
            _ => return Err(bad()),
        })
    }

    /// The value of the next code: the number of a bit coding minus its offset, with the bits
    /// of the number as a signed number and wrapping arithmetic, as htslib gives it.
    pub fn int(&self, streams: &mut Streams) -> Result<i32, Error> {
        let core = &mut streams.core;
        let (value, offset) = match self {
            Self::External(id) => return streams.block(*id)?.itf8(),
            Self::Huffman(huffman) => return huffman.decode(core).ok_or_else(bad),
            Self::Beta { offset, bits } => (core.bits(*bits), *offset),
            Self::Gamma { offset } => (gamma(core), *offset),
            Self::Subexp { offset, k } => (subexp(core, *k), *offset),
            Self::Null | Self::ByteArrayLen(..) | Self::ByteArrayStop(..) => return Err(bad()),
        };
        Ok(value.ok_or_else(bad)?.cast_signed().wrapping_sub(offset))
    }

    pub fn byte(&self, streams: &mut Streams) -> Result<u8, Error> {
        match self {
            Self::External(id) => streams.block(*id)?.byte(),
            _ => Ok(self.int(streams)? as u8),
        }
    }

    pub fn bytes(&self, streams: &mut Streams) -> Result<Vec<u8>, Error> {
        match self {
            Self::ByteArrayLen(length, values) => {
                let length = usize::try_from(length.int(streams)?).map_err(|_| bad())?;
                match values.as_ref() {
                    Self::External(id) => Ok(streams.block(*id)?.take(length)?.to_vec()),
                    other => (0..length).map(|_| other.byte(streams)).collect(),
                }
            }
            Self::ByteArrayStop(stop, id) => {
                let block = streams.block(*id)?;
                let end = block
                    .rest()
                    .iter()
                    .position(|byte| byte == stop)
                    .ok_or_else(bad)?;
                let value = block.take(end)?.to_vec();
                block.byte()?;
                Ok(value)
            }
            _ => Err(bad()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::bits::{Bits, Bytes};
    use super::{Encoding, Huffman, Streams};

    fn streams<'a>(core: &'a [u8], external: &[(i32, &'a [u8])]) -> Streams<'a> {
        Streams {
            core: Bits::new(core),
            external: external
                .iter()
                .map(|(id, data)| (*id, Bytes::new(data)))
                .collect::<HashMap<_, _>>(),
        }
    }

    fn encoding(codec: u8, params: &[u8]) -> Encoding {
        let mut bytes = vec![codec, params.len() as u8];
        bytes.extend(params);
        Encoding::read(&mut Bytes::new(&bytes)).unwrap()
    }

    #[test]
    fn canonical_huffman_codes_count_up_by_length() {
        // Symbols 7 (1 bit), 3 and 5 (2 bits each): codes 0, 10 and 11.
        let huffman = Huffman::new(&[5, 7, 3], &[2, 1, 2]).unwrap();
        let mut both = streams(&[0b0101_1000], &[]);
        let decode = Encoding::Huffman(huffman);
        let values: Vec<i32> = (0..4).map(|_| decode.int(&mut both).unwrap()).collect();
        assert_eq!(values, [7, 3, 5, 7]);
        let one = Encoding::Huffman(Huffman::new(&[42], &[0]).unwrap());
        let mut nothing = streams(&[], &[]);
        assert_eq!(one.int(&mut nothing).unwrap(), 42);
        assert_eq!(one.int(&mut nothing).unwrap(), 42);
    }

    #[test]
    fn encodings_read_their_parameters() {
        assert_eq!(encoding(1, &[0x05]), Encoding::External(5));
        assert_eq!(
            encoding(6, &[0x02, 0x04]),
            Encoding::Beta { offset: 2, bits: 4 }
        );
        assert_eq!(encoding(9, &[0x01]), Encoding::Gamma { offset: 1 });
        assert_eq!(
            encoding(7, &[0x00, 0x02]),
            Encoding::Subexp { offset: 0, k: 2 }
        );
        assert_eq!(encoding(5, &[0x09, 0x03]), Encoding::ByteArrayStop(9, 3));
        assert_eq!(
            encoding(4, &[1, 1, 0x02, 1, 1, 0x03]),
            Encoding::ByteArrayLen(
                Box::new(Encoding::External(2)),
                Box::new(Encoding::External(3))
            )
        );
        assert_eq!(
            encoding(3, &[0x02, 0x41, 0x42, 0x02, 0x01, 0x01]),
            Encoding::Huffman(Huffman::new(&[0x41, 0x42], &[1, 1]).unwrap())
        );
        assert_eq!(encoding(0, &[]), Encoding::Null);
        let mut bytes = Bytes::new(&[2, 0]);
        assert!(Encoding::read(&mut bytes).is_err());
    }

    #[test]
    fn bit_encodings_subtract_their_offset() {
        // Beta 4 bits offset 2: 1010 - 2 = 8. Gamma offset 1: 00101 = 5 - 1 = 4.
        // Subexp k 1: 0 then 1 bit (1) = 1; 110 then 2 bits (01) = 4 | 1 = 5.
        let mut core = streams(&[0b1010_0010, 0b1011_1001], &[]);
        let beta = Encoding::Beta { offset: 2, bits: 4 };
        let gamma = Encoding::Gamma { offset: 1 };
        let subexp = Encoding::Subexp { offset: 0, k: 1 };
        assert_eq!(beta.int(&mut core).unwrap(), 8);
        assert_eq!(gamma.int(&mut core).unwrap(), 4);
        assert_eq!(subexp.int(&mut core).unwrap(), 1);
        assert_eq!(subexp.int(&mut core).unwrap(), 5);
    }

    /// Codes of 32 bits or more, as a damaged header can give, are errors, not overflows.
    #[test]
    fn wide_bit_encodings_are_errors() {
        let data = [0b1000_0000, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let mut core = streams(&data, &[]);
        let subexp = Encoding::Subexp { offset: 0, k: 40 };
        assert!(subexp.int(&mut core).is_err());
        let mut core = streams(&data, &[]);
        let huge = Encoding::Subexp {
            offset: 0,
            k: u32::MAX,
        };
        assert!(huge.int(&mut core).is_err());
        let mut core = streams(&data, &[]);
        let beta = Encoding::Beta {
            offset: 0,
            bits: 40,
        };
        assert!(beta.int(&mut core).is_err());
    }

    /// Gamma and Subexp codes with 31 zeros or ones are the longest: 32 are errors.
    #[test]
    fn gamma_and_subexp_codes_stop_at_31_bits() {
        // 31 zeros, the 1 bit, then 31 ones: 2^31 plus 2^31 - 1, which is -1 as a signed number.
        let gamma = Encoding::Gamma { offset: 0 };
        let mut core = streams(&[0, 0, 0, 0x01, 0xFF, 0xFF, 0xFF, 0xFE], &[]);
        assert_eq!(gamma.int(&mut core).unwrap(), -1);
        let mut core = streams(&[0, 0, 0, 0, 0x80], &[]);
        assert!(gamma.int(&mut core).is_err());
        // 31 ones, the 0 bit, then 31 zeros: 2^31 with k = 1, the lowest signed number.
        let subexp = Encoding::Subexp { offset: 0, k: 1 };
        let mut core = streams(&[0xFF, 0xFF, 0xFF, 0xFE, 0, 0, 0, 0], &[]);
        assert_eq!(subexp.int(&mut core).unwrap(), i32::MIN);
        let mut core = streams(&[0xFF, 0xFF, 0xFF, 0xFF, 0], &[]);
        assert!(subexp.int(&mut core).is_err());
    }

    #[test]
    fn external_values_and_byte_arrays_come_from_their_blocks() {
        let mut both = streams(
            &[],
            &[(1, &[0x81, 0x00, 7]), (2, b"ACGT\0TT\0"), (3, b"xyz")],
        );
        let external = Encoding::External(1);
        assert_eq!(external.int(&mut both).unwrap(), 256);
        assert_eq!(external.byte(&mut both).unwrap(), 7);
        let stop = Encoding::ByteArrayStop(0, 2);
        assert_eq!(stop.bytes(&mut both).unwrap(), b"ACGT");
        assert_eq!(stop.bytes(&mut both).unwrap(), b"TT");
        assert!(stop.bytes(&mut both).is_err());
        let mut lengths = streams(&[], &[(4, &[3, 1]), (3, b"xyz")]);
        let array = Encoding::ByteArrayLen(
            Box::new(Encoding::External(4)),
            Box::new(Encoding::External(3)),
        );
        assert_eq!(array.bytes(&mut lengths).unwrap(), b"xyz");
        assert!(array.bytes(&mut lengths).is_err());
        assert!(Encoding::External(9).int(&mut lengths).is_err());
    }
}
