use std::io::{self, ErrorKind, Write};

use miniz_oxide::deflate::compress_to_vec_zlib;

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
const LEVEL: u8 = 6;
const TABLE: [u32; 256] = table();

pub fn write(mut writer: impl Write, width: u32, height: u32, rgba: &[u8]) -> io::Result<()> {
    let stride = width as usize * 4;
    if stride == 0 || rgba.len() != stride * height as usize {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "the pixels do not fill the image",
        ));
    }
    let mut scanlines = Vec::with_capacity(rgba.len() + height as usize);
    for row in rgba.chunks_exact(stride) {
        scanlines.push(0);
        scanlines.extend_from_slice(row);
    }
    let mut header = Vec::with_capacity(13);
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    header.extend([8, 6, 0, 0, 0]);
    writer.write_all(&SIGNATURE)?;
    chunk(&mut writer, *b"IHDR", &header)?;
    let compressed = compress_to_vec_zlib(&scanlines, LEVEL);
    chunk(&mut writer, *b"IDAT", &compressed)?;
    chunk(&mut writer, *b"IEND", &[])?;
    writer.flush()
}

fn chunk(writer: &mut impl Write, kind: [u8; 4], data: &[u8]) -> io::Result<()> {
    let length = u32::try_from(data.len())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "the image is too large"))?;
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(&kind)?;
    writer.write_all(data)?;
    writer.write_all(&crc(&[&kind, data]).to_be_bytes())
}

fn crc(parts: &[&[u8]]) -> u32 {
    let mut crc = !0;
    for byte in parts.iter().flat_map(|part| part.iter()) {
        crc = TABLE[((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

const fn table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                0xEDB8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use miniz_oxide::inflate::decompress_to_vec_zlib;

    use super::{crc, write};

    fn chunks(png: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut chunks = Vec::new();
        let mut rest = &png[8..];
        while !rest.is_empty() {
            let length = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
            let kind = String::from_utf8(rest[4..8].to_vec()).unwrap();
            let data = rest[8..8 + length].to_vec();
            let stored = u32::from_be_bytes(rest[8 + length..12 + length].try_into().unwrap());
            assert_eq!(stored, crc(&[kind.as_bytes(), &data]), "{kind}");
            chunks.push((kind, data));
            rest = &rest[12 + length..];
        }
        chunks
    }

    #[test]
    fn crc_matches_the_standard_check_value() {
        assert_eq!(crc(&[b"1234", b"56789"]), 0xCBF4_3926);
    }

    #[test]
    fn writes_rgba_scanlines() {
        let rgba = [1, 2, 3, 255, 4, 5, 6, 128, 7, 8, 9, 0, 10, 11, 12, 64];
        let mut png = Vec::new();
        write(&mut png, 2, 2, &rgba).unwrap();
        assert_eq!(
            png[..8],
            [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n']
        );
        assert_eq!(png[png.len() - 4..], [0xAE, 0x42, 0x60, 0x82]);
        let chunks = chunks(&png);
        let kinds: Vec<&str> = chunks.iter().map(|(kind, _)| kind.as_str()).collect();
        assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);
        assert_eq!(chunks[0].1, [0, 0, 0, 2, 0, 0, 0, 2, 8, 6, 0, 0, 0]);
        let mut scanlines = vec![0];
        scanlines.extend(&rgba[..8]);
        scanlines.push(0);
        scanlines.extend(&rgba[8..]);
        assert_eq!(decompress_to_vec_zlib(&chunks[1].1).unwrap(), scanlines);
    }

    #[test]
    fn rejects_pixels_that_do_not_fill_the_image() {
        let error = write(Vec::new(), 2, 2, &[0; 12]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
    }
}
