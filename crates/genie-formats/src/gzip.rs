use std::io::{self, BufRead, ErrorKind, Read};

use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress};
use miniz_oxide::inflate::stream::{self, InflateState};
use miniz_oxide::{DataFormat, MZFlush, MZStatus};

use crate::png::{crc, crc_continue};
use crate::{Error, Reason};

const MAGIC: [u8; 3] = [31, 139, 8];
const HEADER: usize = 10;
const TRAILER: usize = 8;
const FHCRC: u8 = 2;
const FEXTRA: u8 = 4;
const FNAME: u8 = 8;
const FCOMMENT: u8 = 16;
const LEVEL: u8 = 6;
const BUFFER: usize = 1 << 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Start,
    Body,
    Done,
}

pub struct Reader<R> {
    inner: R,
    state: Box<InflateState>,
    output: Vec<u8>,
    filled: usize,
    position: usize,
    check: u32,
    size: u32,
    phase: Phase,
}

impl<R: BufRead> Reader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            state: InflateState::new_boxed(DataFormat::Raw),
            output: vec![0; BUFFER],
            filled: 0,
            position: 0,
            check: 0,
            size: 0,
            phase: Phase::Start,
        }
    }

    fn refill(&mut self) -> io::Result<()> {
        self.position = 0;
        self.filled = 0;
        while self.filled == 0 {
            match self.phase {
                Phase::Done => break,
                Phase::Start => self.start()?,
                Phase::Body => self.inflate()?,
            }
        }
        Ok(())
    }

    fn start(&mut self) -> io::Result<()> {
        let input = self.inner.fill_buf()?;
        if input.is_empty() {
            self.phase = Phase::Done;
            return Ok(());
        }
        if input.iter().all(|byte| *byte == 0) {
            let count = input.len();
            self.inner.consume(count);
            return Ok(());
        }
        read_header(&mut self.inner)?;
        self.state.reset(DataFormat::Raw);
        self.check = 0;
        self.size = 0;
        self.phase = Phase::Body;
        Ok(())
    }

    fn inflate(&mut self) -> io::Result<()> {
        let input = self.inner.fill_buf()?;
        if input.is_empty() {
            return Err(io::Error::from(ErrorKind::UnexpectedEof));
        }
        let result = stream::inflate(&mut self.state, input, &mut self.output, MZFlush::None);
        self.inner.consume(result.bytes_consumed);
        let written = result.bytes_written;
        self.check = crc_continue(self.check, &self.output[..written]);
        self.size = self.size.wrapping_add(written as u32);
        self.filled = written;
        match result.status {
            Ok(MZStatus::StreamEnd) => {
                let mut trailer = [0; TRAILER];
                self.inner.read_exact(&mut trailer)?;
                let [check, size] = [&trailer[..4], &trailer[4..]]
                    .map(|part| u32::from_le_bytes([part[0], part[1], part[2], part[3]]));
                if check != self.check || size != self.size {
                    return Err(broken(damaged()));
                }
                self.phase = Phase::Start;
                Ok(())
            }
            Ok(_) => Ok(()),
            Err(_) => Err(broken(damaged())),
        }
    }
}

impl<R: BufRead> Read for Reader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let count = available.len().min(buffer.len());
        buffer[..count].copy_from_slice(&available[..count]);
        self.consume(count);
        Ok(count)
    }
}

impl<R: BufRead> BufRead for Reader<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.position == self.filled {
            self.refill()?;
        }
        Ok(&self.output[self.position..self.filled])
    }

    fn consume(&mut self, amount: usize) {
        self.position = (self.position + amount).min(self.filled);
    }
}

fn read_header(reader: &mut impl BufRead) -> io::Result<()> {
    let mut fixed = [0; HEADER];
    reader.read_exact(&mut fixed)?;
    if !fixed.starts_with(&MAGIC) {
        return Err(broken(Error::Invalid(Reason::NotGzip)));
    }
    let flags = fixed[3];
    if flags & FEXTRA != 0 {
        let mut length = [0; 2];
        reader.read_exact(&mut length)?;
        let length = u64::from(u16::from_le_bytes(length));
        if io::copy(&mut reader.take(length), &mut io::sink())? != length {
            return Err(io::Error::from(ErrorKind::UnexpectedEof));
        }
    }
    for flag in [FNAME, FCOMMENT] {
        if flags & flag != 0 {
            let mut text = Vec::new();
            reader.read_until(0, &mut text)?;
            if text.last() != Some(&0) {
                return Err(io::Error::from(ErrorKind::UnexpectedEof));
            }
        }
    }
    if flags & FHCRC != 0 {
        reader.read_exact(&mut [0; 2])?;
    }
    Ok(())
}

fn broken(error: Error) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, error)
}

pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend([0, 0, 0, 0, 0, 0, 255]);
    bytes.extend(compress_to_vec(data, LEVEL));
    bytes.extend(crc(&[data]).to_le_bytes());
    bytes.extend((data.len() as u32).to_le_bytes());
    bytes
}

pub fn inflate(mut bytes: &[u8]) -> Result<Vec<u8>, Error> {
    if !bytes.starts_with(&MAGIC) {
        return Err(Error::Invalid(Reason::NotGzip));
    }
    let mut text = Vec::new();
    while bytes.iter().any(|byte| *byte != 0) {
        let body = header(bytes)?;
        let (data, used) = member(body)?;
        let trailer = body.get(used..used + TRAILER).ok_or_else(damaged)?;
        let check = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
        let size = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
        if check != crc(&[&data]) || size != data.len() as u32 {
            return Err(damaged());
        }
        text.extend(data);
        bytes = &body[used + TRAILER..];
    }
    Ok(text)
}

fn header(bytes: &[u8]) -> Result<&[u8], Error> {
    if bytes.len() < HEADER || !bytes.starts_with(&MAGIC) {
        return Err(Error::Invalid(Reason::NotGzip));
    }
    let flags = bytes[3];
    let mut at = HEADER;
    if flags & FEXTRA != 0 {
        let length = bytes.get(at..at + 2).ok_or_else(damaged)?;
        at += 2 + usize::from(u16::from_le_bytes([length[0], length[1]]));
    }
    for flag in [FNAME, FCOMMENT] {
        if flags & flag != 0 {
            let end = bytes
                .get(at..)
                .and_then(|rest| rest.iter().position(|byte| *byte == 0))
                .ok_or_else(damaged)?;
            at += end + 1;
        }
    }
    if flags & FHCRC != 0 {
        at += 2;
    }
    bytes.get(at..).ok_or_else(damaged)
}

fn member(input: &[u8]) -> Result<(Vec<u8>, usize), Error> {
    let mut state = Box::<DecompressorOxide>::default();
    let mut output = vec![0; input.len().saturating_mul(4).max(1 << 10)];
    let (mut read, mut written) = (0, 0);
    loop {
        let (status, used, made) = decompress(
            &mut state,
            &input[read..],
            &mut output,
            written,
            TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
        );
        read += used;
        written += made;
        match status {
            TINFLStatus::Done => {
                output.truncate(written);
                return Ok((output, read));
            }
            TINFLStatus::HasMoreOutput => output.resize(output.len() * 2, 0),
            _ => return Err(damaged()),
        }
    }
}

fn damaged() -> Error {
    Error::Invalid(Reason::BadBlock)
}

#[cfg(test)]
mod tests {
    use miniz_oxide::deflate::compress_to_vec;

    use std::io::{Cursor, ErrorKind, Read, Write};

    use super::{Reader, compress, inflate};
    use crate::bgzf::Writer;
    use crate::png::crc;
    use crate::{Error, Reason};

    fn stream(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
        let mut text = Vec::new();
        Reader::new(Cursor::new(bytes)).read_to_end(&mut text)?;
        Ok(text)
    }

    fn member(data: &[u8], name: Option<&str>, extra: bool) -> Vec<u8> {
        let flags = if name.is_some() { 8 } else { 0 } | if extra { 4 } else { 0 };
        let mut bytes = vec![31, 139, 8, flags, 0, 0, 0, 0, 0, 255];
        if extra {
            bytes.extend([6, 0, b'B', b'C', 2, 0, 0, 0]);
        }
        if let Some(name) = name {
            bytes.extend(name.as_bytes());
            bytes.push(0);
        }
        bytes.extend(compress_to_vec(data, 6));
        bytes.extend(crc(&[data]).to_le_bytes());
        bytes.extend(u32::try_from(data.len()).unwrap().to_le_bytes());
        bytes
    }

    #[test]
    fn inflates_one_member_with_a_name() {
        let bytes = member(
            b"chr1\t0\t2300000\tp36.33\tgneg\n",
            Some("cytoBandIdeo.txt"),
            false,
        );
        assert_eq!(
            inflate(&bytes).unwrap(),
            b"chr1\t0\t2300000\tp36.33\tgneg\n"
        );
    }

    #[test]
    fn compressed_text_inflates_back() {
        let text = b"chr1\t0\t10\tp1\tgneg\nchr1\t10\t20\tq1\tgpos50\n".repeat(50);
        let packed = compress(&text);
        assert!(packed.len() < text.len());
        assert_eq!(inflate(&packed).unwrap(), text);
        assert_eq!(inflate(&compress(b"")).unwrap(), b"");
    }

    #[test]
    fn inflates_members_one_after_another() {
        let mut bytes = member(b"hello ", None, true);
        bytes.extend(member(b"world\n", None, false));
        bytes.extend(member(b"", None, true));
        assert_eq!(inflate(&bytes).unwrap(), b"hello world\n");
    }

    #[test]
    fn rejects_other_and_damaged_data() {
        let not_gzip = inflate(b"chr1\t0\t10\n").unwrap_err();
        assert!(matches!(not_gzip, Error::Invalid(Reason::NotGzip)));
        let good = member(b"some text that compresses", None, false);
        let cut = inflate(&good[..good.len() - 12]).unwrap_err();
        assert!(matches!(cut, Error::Invalid(Reason::BadBlock)));
        let mut wrong = good.clone();
        let at = wrong.len() - 8;
        wrong[at] ^= 1;
        assert!(matches!(
            inflate(&wrong).unwrap_err(),
            Error::Invalid(Reason::BadBlock)
        ));
    }

    #[test]
    fn streams_members_one_after_another() {
        let mut bytes = member(b"hello ", Some("a.txt"), true);
        bytes.extend(member(b"", None, false));
        bytes.extend(member(b"world\n", None, false));
        assert_eq!(stream(&bytes).unwrap(), b"hello world\n");
        bytes.extend([0; 16]);
        assert_eq!(stream(&bytes).unwrap(), b"hello world\n");
    }

    #[test]
    fn streams_text_larger_than_one_buffer_and_bgzip_files() {
        let text: Vec<u8> = (0..60_000_u32)
            .flat_map(|line| format!("chr1\t{line}\t{}\n", line * 7 % 1_000).into_bytes())
            .collect();
        assert_eq!(stream(&compress(&text)).unwrap(), text);
        let mut writer = Writer::new(Vec::new());
        writer.write_all(&text).unwrap();
        assert_eq!(stream(&writer.finish().unwrap()).unwrap(), text);
    }

    #[test]
    fn streaming_rejects_other_and_damaged_data() {
        let error = stream(b"chr1\t0\t10\n").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        let good = member(b"some text that compresses", None, false);
        assert!(stream(&good[..good.len() - 12]).is_err());
        let mut wrong = good.clone();
        let at = wrong.len() - 8;
        wrong[at] ^= 1;
        assert_eq!(stream(&wrong).unwrap_err().kind(), ErrorKind::InvalidData);
    }
}
