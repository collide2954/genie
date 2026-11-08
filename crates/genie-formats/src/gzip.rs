use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress};

use crate::png::crc;
use crate::{Error, Reason};

const MAGIC: [u8; 3] = [31, 139, 8];
const HEADER: usize = 10;
const TRAILER: usize = 8;
const FHCRC: u8 = 2;
const FEXTRA: u8 = 4;
const FNAME: u8 = 8;
const FCOMMENT: u8 = 16;
const LEVEL: u8 = 6;

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

    use super::{compress, inflate};
    use crate::png::crc;
    use crate::{Error, Reason};

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
}
