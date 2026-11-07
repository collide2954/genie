use std::io::{self, BufRead, ErrorKind, Read, Seek, SeekFrom};

use miniz_oxide::inflate::{
    TINFLStatus,
    core::{
        DecompressorOxide, decompress, inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
    },
};

use crate::{Error, Reason};

const MAGIC: [u8; 4] = [31, 139, 8, 4];
const HEADER: usize = 12;
const FOOTER: usize = 8;
const MAX_LENGTH: usize = 1 << 16;

pub struct Bgzf<R> {
    inner: R,
    inflater: Box<DecompressorOxide>,
    compressed: Vec<u8>,
    block: Vec<u8>,
    position: usize,
    offset: u64,
    next: u64,
}

impl<R> Bgzf<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            inflater: Box::default(),
            compressed: Vec::new(),
            block: Vec::with_capacity(MAX_LENGTH),
            position: 0,
            offset: 0,
            next: 0,
        }
    }

    pub fn virtual_offset(&self) -> u64 {
        if self.position < self.block.len() {
            self.offset << 16 | self.position as u64
        } else {
            self.next << 16
        }
    }
}

impl<R: Read + Seek> Bgzf<R> {
    pub fn seek(&mut self, virtual_offset: u64) -> io::Result<()> {
        let offset = virtual_offset >> 16;
        let position = (virtual_offset & 0xFFFF) as usize;
        if offset != self.offset || self.block.is_empty() {
            self.inner.seek(SeekFrom::Start(offset))?;
            self.next = offset;
            self.block.clear();
            self.position = 0;
            self.load()?;
        }
        if position > self.block.len() {
            return Err(invalid(Reason::BadOffset));
        }
        self.position = position;
        Ok(())
    }
}

impl<R: Read> Bgzf<R> {
    fn load(&mut self) -> io::Result<bool> {
        let Some((size, length)) = read_block(&mut self.inner, &mut self.compressed)? else {
            return Ok(false);
        };
        let data = &self.compressed[..self.compressed.len() - FOOTER];
        self.block.resize(length, 0);
        self.inflater.init();
        let (status, _, written) = decompress(
            &mut self.inflater,
            data,
            &mut self.block,
            0,
            TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
        );
        if status != TINFLStatus::Done || written != length {
            return Err(invalid(Reason::BadBlock));
        }
        self.offset = self.next;
        self.next += size as u64;
        self.position = 0;
        Ok(true)
    }
}

impl<R: Read> Read for Bgzf<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let count = available.len().min(buffer.len());
        buffer[..count].copy_from_slice(&available[..count]);
        self.consume(count);
        Ok(count)
    }
}

impl<R: Read> BufRead for Bgzf<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        while self.position == self.block.len() && self.load()? {}
        Ok(&self.block[self.position..])
    }

    fn consume(&mut self, amount: usize) {
        self.position = (self.position + amount).min(self.block.len());
    }
}

pub(crate) fn read_block(
    reader: &mut impl Read,
    compressed: &mut Vec<u8>,
) -> io::Result<Option<(usize, usize)>> {
    let mut header = [0; HEADER];
    match fill(reader, &mut header)? {
        0 => return Ok(None),
        HEADER if header[..4] == MAGIC => {}
        _ => return Err(invalid(Reason::NotBgzf)),
    }
    let extra = usize::from(u16::from_le_bytes([header[10], header[11]]));
    compressed.resize(extra, 0);
    reader.read_exact(compressed)?;
    let size = block_size(compressed).ok_or_else(|| invalid(Reason::NotBgzf))?;
    let rest = size
        .checked_sub(HEADER + extra)
        .filter(|rest| *rest >= FOOTER)
        .ok_or_else(|| invalid(Reason::BadBlock))?;
    compressed.resize(rest, 0);
    reader.read_exact(compressed)?;
    let footer = &compressed[rest - 4..];
    let length = u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]) as usize;
    if length > MAX_LENGTH {
        return Err(invalid(Reason::BadBlock));
    }
    Ok(Some((size, length)))
}

fn block_size(mut extra: &[u8]) -> Option<usize> {
    while let [first, second, low, high, rest @ ..] = extra {
        let length = usize::from(u16::from_le_bytes([*low, *high]));
        let (data, tail) = rest.split_at_checked(length)?;
        if let ([b'B', b'C'], [low, high]) = ([*first, *second], data) {
            return Some(usize::from(u16::from_le_bytes([*low, *high])) + 1);
        }
        extra = tail;
    }
    None
}

fn fill(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut count = 0;
    while count < buffer.len() {
        match reader.read(&mut buffer[count..]) {
            Ok(0) => break,
            Ok(read) => count += read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(count)
}

fn invalid(reason: Reason) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, Error::Invalid(reason))
}

#[cfg(test)]
pub mod tests {
    use std::io::{BufRead, Cursor, ErrorKind, Read};

    use miniz_oxide::deflate::compress_to_vec;

    use super::Bgzf;

    pub fn block(data: &[u8]) -> Vec<u8> {
        let compressed = compress_to_vec(data, 6);
        let size = u16::try_from(compressed.len() + 25).unwrap();
        let mut block = vec![31, 139, 8, 4, 0, 0, 0, 0, 0, 255, 6, 0, b'B', b'C', 2, 0];
        block.extend(size.to_le_bytes());
        block.extend(compressed);
        block.extend([0; 4]);
        block.extend(u32::try_from(data.len()).unwrap().to_le_bytes());
        block
    }

    pub fn file(parts: &[&[u8]]) -> Vec<u8> {
        let mut bytes: Vec<u8> = parts.iter().flat_map(|part| block(part)).collect();
        bytes.extend(block(b""));
        bytes
    }

    #[test]
    fn reads_across_blocks() {
        let mut text = String::new();
        Bgzf::new(Cursor::new(file(&[b"hello ", b"world\n"])))
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "hello world\n");
    }

    #[test]
    fn seeks_to_virtual_offsets() {
        let first = block(b"hello ").len() as u64;
        let mut reader = Bgzf::new(Cursor::new(file(&[b"hello ", b"world\n"])));
        reader.seek(first << 16 | 1).unwrap();
        let mut text = String::new();
        reader.read_to_string(&mut text).unwrap();
        assert_eq!(text, "orld\n");
    }

    #[test]
    fn reports_virtual_offsets_while_reading_lines() {
        let first = block(b"a\nb\n").len() as u64;
        let mut reader = Bgzf::new(Cursor::new(file(&[b"a\nb\n", b"c\n"])));
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(reader.virtual_offset(), 2);
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(reader.virtual_offset(), first << 16);
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "c\n");
    }

    #[test]
    fn rejects_gzip_without_block_sizes() {
        let mut plain = block(b"x");
        plain[12] = b'Z';
        let error = Bgzf::new(Cursor::new(plain))
            .read_to_end(&mut Vec::new())
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }
}
