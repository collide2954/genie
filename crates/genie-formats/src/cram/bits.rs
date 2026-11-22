//! Byte and bit readers for CRAM: ITF8 and LTF8 numbers, and the MSB-first core bit stream.

use vstd::prelude::*;

use crate::{Error, Reason};

pub(crate) fn bad() -> Error {
    Error::Invalid(Reason::BadCram)
}

verus! {

/// The bytes of an ITF8 number from its first byte: 1 to 5, by the 1 bits at its high end.
pub open spec fn spec_itf8_length(first: u8) -> int {
    if first < 0x80 {
        1
    } else if first < 0xC0 {
        2
    } else if first < 0xE0 {
        3
    } else if first < 0xF0 {
        4
    } else {
        5
    }
}

/// The value of the ITF8 number of bytes that start with its bytes, as CRAM 3.1 gives it: the
/// low bits of the first byte, then the next bytes, high bits first. A number of 5 bytes takes
/// only the low 4 bits of its last byte.
pub open spec fn spec_itf8(bytes: Seq<u8>) -> int
    recommends
        bytes.len() >= spec_itf8_length(bytes[0]),
{
    let length = spec_itf8_length(bytes[0]);
    if length == 1 {
        bytes[0] as int
    } else if length == 2 {
        (bytes[0] % 64) * 256 + bytes[1]
    } else if length == 3 {
        (bytes[0] % 32) * 65536 + bytes[1] * 256 + bytes[2]
    } else if length == 4 {
        (bytes[0] % 16) * 16777216 + bytes[1] * 65536 + bytes[2] * 256 + bytes[3]
    } else {
        (bytes[0] % 16) * 268435456 + bytes[1] * 1048576 + bytes[2] * 4096 + bytes[3] * 16 + bytes[4]
            % 16
    }
}

/// The ITF8 number at the start of bytes as 32 bits, and its number of bytes, or nothing when
/// the bytes stop before its end.
#[allow(clippy::len_zero)] // Verus has no specification of is_empty for slices.
pub fn read_itf8(bytes: &[u8]) -> (found: Option<(u32, usize)>)
    ensures
        match found {
            Some((value, length)) => bytes.len() >= 1 && length == spec_itf8_length(bytes[0])
                && length <= bytes.len() && value == spec_itf8(bytes@),
            None => bytes.len() == 0 || bytes.len() < spec_itf8_length(bytes[0]),
        },
{
    if bytes.len() == 0 {
        return None;
    }
    let first = bytes[0];
    if first < 0x80 {
        return Some((u32::from(first), 1));
    }
    if first < 0xC0 {
        if bytes.len() < 2 {
            return None;
        }
        let second = bytes[1];
        let value = u32::from(first & 0x3F) << 8 | u32::from(second);
        assert(value == (first % 64) * 256 + second) by (bit_vector)
            requires
                value == ((first & 0x3F) as u32) << 8 | second as u32,
        ;
        return Some((value, 2));
    }
    if first < 0xE0 {
        if bytes.len() < 3 {
            return None;
        }
        let (second, third) = (bytes[1], bytes[2]);
        let value = u32::from(first & 0x1F) << 16 | u32::from(second) << 8 | u32::from(third);
        assert(value == (first % 32) * 65536 + second * 256 + third) by (bit_vector)
            requires
                value == ((first & 0x1F) as u32) << 16 | (second as u32) << 8 | third as u32,
        ;
        return Some((value, 3));
    }
    if first < 0xF0 {
        if bytes.len() < 4 {
            return None;
        }
        let (second, third, fourth) = (bytes[1], bytes[2], bytes[3]);
        let value = u32::from(first & 0x0F) << 24 | u32::from(second) << 16 | u32::from(third) << 8
            | u32::from(fourth);
        assert(value == (first % 16) * 16777216 + second * 65536 + third * 256 + fourth)
            by (bit_vector)
            requires
                value == ((first & 0x0F) as u32) << 24 | (second as u32) << 16 | (third as u32)
                    << 8 | fourth as u32,
        ;
        return Some((value, 4));
    }
    if bytes.len() < 5 {
        return None;
    }
    let (second, third, fourth, fifth) = (bytes[1], bytes[2], bytes[3], bytes[4]);
    let value = u32::from(first & 0x0F) << 28 | u32::from(second) << 20 | u32::from(third) << 12
        | u32::from(fourth) << 4 | u32::from(fifth & 0x0F);
    assert(value == (first % 16) * 268435456 + second * 1048576 + third * 4096 + fourth * 16
        + fifth % 16) by (bit_vector)
        requires
            value == ((first & 0x0F) as u32) << 28 | (second as u32) << 20 | (third as u32) << 12
                | (fourth as u32) << 4 | (fifth & 0x0F) as u32,
    ;
    Some((value, 5))
}

/// The number of bytes in big-endian order: the high byte first.
pub open spec fn spec_big(bytes: Seq<u8>) -> int
    decreases bytes.len(),
{
    if bytes.len() == 0 {
        0
    } else {
        spec_big(bytes.drop_last()) * 256 + bytes.last()
    }
}

/// The bytes of an LTF8 number after its first byte: 0 to 8, by the 1 bits at the high end of
/// its first byte.
pub open spec fn spec_ltf8_extra(first: u8) -> int {
    if first < 0x80 {
        0
    } else if first < 0xC0 {
        1
    } else if first < 0xE0 {
        2
    } else if first < 0xF0 {
        3
    } else if first < 0xF8 {
        4
    } else if first < 0xFC {
        5
    } else if first < 0xFE {
        6
    } else if first < 0xFF {
        7
    } else {
        8
    }
}

/// The value bits of the first byte of an LTF8 number: the bits after its 1 bits and their 0
/// bit, none for 7 or 8 more bytes.
pub open spec fn spec_ltf8_low(first: u8) -> int {
    let extra = spec_ltf8_extra(first);
    if extra >= 7 {
        0
    } else {
        first as int % vstd::arithmetic::power2::pow2((7 - extra) as nat) as int
    }
}

/// The LTF8 number at the start of bytes as 64 bits, and its number of bytes, or nothing when
/// the bytes stop before its end. The value is the value bits of the first byte, then the next
/// bytes, high bits first.
#[allow(clippy::len_zero)] // Verus has no specification of is_empty for slices.
pub fn read_ltf8(bytes: &[u8]) -> (found: Option<(u64, usize)>)
    ensures
        match found {
            Some((value, length)) => bytes.len() >= 1 && length == 1 + spec_ltf8_extra(bytes[0])
                && length <= bytes.len() && value == spec_ltf8_low(bytes[0]) * vstd::arithmetic::power::pow(
                256,
                (length - 1) as nat,
            ) + spec_big(bytes@.subrange(1, length as int)),
            None => bytes.len() == 0 || bytes.len() < 1 + spec_ltf8_extra(bytes[0]),
        },
{
    if bytes.len() == 0 {
        return None;
    }
    let first = bytes[0];
    let extra: usize = if first < 0x80 {
        0
    } else if first < 0xC0 {
        1
    } else if first < 0xE0 {
        2
    } else if first < 0xF0 {
        3
    } else if first < 0xF8 {
        4
    } else if first < 0xFC {
        5
    } else if first < 0xFE {
        6
    } else if first < 0xFF {
        7
    } else {
        8
    };
    if bytes.len() < 1 + extra {
        return None;
    }
    let low: u64 = match extra {
        0 => u64::from(first & 0x7F),
        1 => u64::from(first & 0x3F),
        2 => u64::from(first & 0x1F),
        3 => u64::from(first & 0x0F),
        4 => u64::from(first & 0x07),
        5 => u64::from(first & 0x03),
        6 => u64::from(first & 0x01),
        _ => 0,
    };
    proof {
        vstd::arithmetic::power2::lemma2_to64();
        assert(first & 0x7F == first % 128) by (bit_vector);
        assert(first & 0x3F == first % 64) by (bit_vector);
        assert(first & 0x1F == first % 32) by (bit_vector);
        assert(first & 0x0F == first % 16) by (bit_vector);
        assert(first & 0x07 == first % 8) by (bit_vector);
        assert(first & 0x03 == first % 4) by (bit_vector);
        assert(first & 0x01 == first % 2) by (bit_vector);
        assert(bytes@.subrange(1, 1) =~= Seq::<u8>::empty());
        vstd::arithmetic::power::lemma_pow0(256);
    }
    let mut value = low;
    let mut at: usize = 1;
    while at <= extra
        invariant
            1 <= at <= extra + 1,
            extra <= 8,
            bytes.len() >= 1 + extra,
            first == bytes[0],
            extra == spec_ltf8_extra(first),
            low == spec_ltf8_low(first),
            low < vstd::arithmetic::power2::pow2((if extra >= 7 { 0 } else { 7 - extra }) as nat),
            value == low * vstd::arithmetic::power::pow(256, (at - 1) as nat) + spec_big(
                bytes@.subrange(1, at as int),
            ),
            value < vstd::arithmetic::power2::pow2(
                (8 * (at - 1) + (if extra >= 7 { 0 } else { 7 - extra })) as nat,
            ),
        decreases extra + 1 - at,
    {
        let byte = bytes[at];
        let ghost bits = (if extra >= 7 { 0 } else { 7 - extra }) as nat;
        let ghost exponent = (8 * (at - 1) + bits) as nat;
        proof {
            vstd::arithmetic::power2::lemma2_to64();
            vstd::arithmetic::power2::lemma2_to64_rest();
            if exponent < 56 {
                vstd::arithmetic::power2::lemma_pow2_strictly_increases(exponent, 56);
            }
        }
        let next = value << 8 | u64::from(byte);
        proof {
            assert(next == value * 256 + byte) by (bit_vector)
                requires
                    value < 0x100_0000_0000_0000,
                    next == value << 8 | byte as u64,
            ;
            assert(bytes@.subrange(1, at + 1).drop_last() =~= bytes@.subrange(1, at as int));
            assert(bytes@.subrange(1, at + 1).last() == byte);
            vstd::arithmetic::power::lemma_pow_adds(256, (at - 1) as nat, 1);
            vstd::arithmetic::power::lemma_pow1(256);
            vstd::arithmetic::power2::lemma_pow2_adds(exponent, 8);
            let previous = vstd::arithmetic::power::pow(256, (at - 1) as nat);
            assert(low * previous * 256 == low * (previous * 256)) by (nonlinear_arith);
        }
        value = next;
        at += 1;
    }
    Some((value, 1 + extra))
}

} // verus!

#[derive(Debug, Clone)]
pub(crate) struct Bytes<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Bytes<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    pub fn at(&self) -> usize {
        self.at
    }

    pub fn rest(&self) -> &'a [u8] {
        &self.data[self.at..]
    }

    pub fn byte(&mut self) -> Result<u8, Error> {
        let byte = *self.data.get(self.at).ok_or_else(bad)?;
        self.at += 1;
        Ok(byte)
    }

    pub fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let end = self.at.checked_add(count).ok_or_else(bad)?;
        let part = self.data.get(self.at..end).ok_or_else(bad)?;
        self.at = end;
        Ok(part)
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn itf8(&mut self) -> Result<i32, Error> {
        let (value, length) = read_itf8(self.rest()).ok_or_else(bad)?;
        self.at += length;
        Ok(value.cast_signed())
    }

    pub fn ltf8(&mut self) -> Result<i64, Error> {
        let (value, length) = read_ltf8(self.rest()).ok_or_else(bad)?;
        self.at += length;
        Ok(value.cast_signed())
    }

    pub fn size(&mut self) -> Result<usize, Error> {
        usize::try_from(self.itf8()?).map_err(|_| bad())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    left: u32,
}

impl<'a> Bits<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            at: 0,
            left: 8,
        }
    }

    pub fn bit(&mut self) -> Result<u32, Error> {
        let byte = *self.data.get(self.at).ok_or_else(bad)?;
        self.left -= 1;
        let bit = u32::from(byte >> self.left) & 1;
        if self.left == 0 {
            self.at += 1;
            self.left = 8;
        }
        Ok(bit)
    }

    pub fn bits(&mut self, count: u32) -> Result<u32, Error> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | self.bit()?;
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{Bits, Bytes};

    #[test]
    fn itf8_numbers_take_one_to_five_bytes() {
        let data = [
            0x05, 0x80, 0xFF, 0xC1, 0x02, 0x03, 0xE1, 0x02, 0x03, 0x04, 0xFF, 0xFF, 0xFF, 0xFF,
            0x0F, 0xF1, 0x02, 0x03, 0x04, 0x05,
        ];
        let mut bytes = Bytes::new(&data);
        assert_eq!(bytes.itf8().unwrap(), 5);
        assert_eq!(bytes.itf8().unwrap(), 0xFF);
        assert_eq!(bytes.itf8().unwrap(), 0x01_0203);
        assert_eq!(bytes.itf8().unwrap(), 0x0102_0304);
        assert_eq!(bytes.itf8().unwrap(), -1);
        assert_eq!(bytes.itf8().unwrap(), 0x1020_3045);
        assert_eq!(bytes.at(), data.len());
        assert!(bytes.itf8().is_err());
    }

    #[test]
    fn ltf8_numbers_take_one_to_nine_bytes() {
        let data = [
            0x7F, 0x81, 0x02, 0xC1, 0x02, 0x03, 0xFE, 1, 2, 3, 4, 5, 6, 7, 0xFF, 1, 2, 3, 4, 5, 6,
            7, 8,
        ];
        let mut bytes = Bytes::new(&data);
        assert_eq!(bytes.ltf8().unwrap(), 0x7F);
        assert_eq!(bytes.ltf8().unwrap(), 0x0102);
        assert_eq!(bytes.ltf8().unwrap(), 0x01_0203);
        assert_eq!(bytes.ltf8().unwrap(), 0x0001_0203_0405_0607);
        assert_eq!(bytes.ltf8().unwrap(), 0x0102_0304_0506_0708);
        assert!(bytes.ltf8().is_err());
    }

    #[test]
    fn bytes_and_words_come_in_order() {
        let mut bytes = Bytes::new(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(bytes.byte().unwrap(), 1);
        assert_eq!(bytes.u32().unwrap(), 0x0504_0302);
        assert_eq!(bytes.rest(), [6]);
        assert!(bytes.take(2).is_err());
        assert_eq!(bytes.take(1).unwrap(), [6]);
    }

    #[test]
    fn bits_come_from_the_high_end_first() {
        let mut bits = Bits::new(&[0b1011_0001, 0b1100_0000]);
        assert_eq!(bits.bit().unwrap(), 1);
        assert_eq!(bits.bits(3).unwrap(), 0b011);
        assert_eq!(bits.bits(6).unwrap(), 0b00_0111);
        assert_eq!(bits.bits(6).unwrap(), 0);
        assert!(bits.bit().is_err());
    }
}
