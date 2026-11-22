//! The encodings of CRAM data series: values from the core bit stream or from external blocks.

use std::collections::HashMap;

use super::bits::{Bits, Bytes, bad};
use crate::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Huffman {
    codes: Vec<(u32, u32, i32)>,
}

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
        let mut codes = Vec::with_capacity(pairs.len());
        let (mut code, mut last) = (0_u32, 0_u32);
        for (index, (length, symbol)) in pairs.into_iter().enumerate() {
            if length > 31 {
                return Err(bad());
            }
            if index > 0 {
                code += 1;
            }
            if length > last {
                code <<= length - last;
                last = length;
            }
            codes.push((length, code, symbol));
        }
        Ok(Self { codes })
    }

    fn decode(&self, bits: &mut Bits) -> Result<i32, Error> {
        let (mut code, mut length) = (0_u32, 0_u32);
        for &(size, value, symbol) in &self.codes {
            while length < size {
                code = (code << 1) | bits.bit()?;
                length += 1;
            }
            if code == value {
                return Ok(symbol);
            }
        }
        Err(bad())
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

    pub fn int(&self, streams: &mut Streams) -> Result<i32, Error> {
        let core = &mut streams.core;
        let value = match self {
            Self::External(id) => return streams.block(*id)?.itf8(),
            Self::Huffman(huffman) => return huffman.decode(core),
            Self::Beta { offset, bits } => core.bits(*bits)?.cast_signed().wrapping_sub(*offset),
            Self::Gamma { offset } => {
                let mut zeros = 0;
                while core.bit()? == 0 {
                    zeros += 1;
                    if zeros > 31 {
                        return Err(bad());
                    }
                }
                let value = (1 << zeros) | core.bits(zeros)?;
                value.cast_signed().wrapping_sub(*offset)
            }
            Self::Subexp { offset, k } => {
                let mut ones = 0;
                while core.bit()? == 1 {
                    ones += 1;
                    if ones > 31 {
                        return Err(bad());
                    }
                }
                let value = if ones == 0 {
                    core.bits(*k)?
                } else {
                    let width = k.checked_add(ones - 1).filter(|width| *width < 32);
                    let width = width.ok_or_else(bad)?;
                    (1 << width) | core.bits(width)?
                };
                value.cast_signed().wrapping_sub(*offset)
            }
            Self::Null | Self::ByteArrayLen(..) | Self::ByteArrayStop(..) => return Err(bad()),
        };
        Ok(value)
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
