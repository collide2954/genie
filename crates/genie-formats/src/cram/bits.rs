//! Byte and bit readers for CRAM: ITF8 and LTF8 numbers, and the MSB-first core bit stream.

use crate::{Error, Reason};

pub(crate) fn bad() -> Error {
    Error::Invalid(Reason::BadCram)
}

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
        let first = self.byte()?;
        let value = match first.leading_ones() {
            0 => u32::from(first),
            1 => (u32::from(first & 0x3F) << 8) | u32::from(self.byte()?),
            2 => {
                let rest = self.take(2)?;
                (u32::from(first & 0x1F) << 16) | (u32::from(rest[0]) << 8) | u32::from(rest[1])
            }
            3 => {
                let rest = self.take(3)?;
                (u32::from(first & 0x0F) << 24)
                    | (u32::from(rest[0]) << 16)
                    | (u32::from(rest[1]) << 8)
                    | u32::from(rest[2])
            }
            _ => {
                let rest = self.take(4)?;
                (u32::from(first & 0x0F) << 28)
                    | (u32::from(rest[0]) << 20)
                    | (u32::from(rest[1]) << 12)
                    | (u32::from(rest[2]) << 4)
                    | u32::from(rest[3] & 0x0F)
            }
        };
        Ok(value.cast_signed())
    }

    pub fn ltf8(&mut self) -> Result<i64, Error> {
        let first = self.byte()?;
        let extra = first.leading_ones();
        let mut value = if extra >= 7 {
            0
        } else {
            u64::from(first & (0xFF >> (extra + 1)))
        };
        for _ in 0..extra {
            value = (value << 8) | u64::from(self.byte()?);
        }
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
