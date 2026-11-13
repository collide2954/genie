const ICO_HEADER: usize = 6;
const ICO_ENTRY: usize = 16;
const ICNS_HEADER: usize = 8;

pub fn ico(images: &[(u32, &[u8])]) -> Vec<u8> {
    let count = u16::try_from(images.len()).unwrap_or(u16::MAX);
    let mut file = vec![0, 0, 1, 0];
    file.extend(count.to_le_bytes());
    let mut offset = ICO_HEADER + ICO_ENTRY * images.len();
    for (size, png) in images {
        let side = u8::try_from(*size).unwrap_or(0);
        file.extend([side, side, 0, 0, 1, 0, 32, 0]);
        file.extend(u32::try_from(png.len()).unwrap_or(u32::MAX).to_le_bytes());
        file.extend(u32::try_from(offset).unwrap_or(u32::MAX).to_le_bytes());
        offset += png.len();
    }
    for (_, png) in images {
        file.extend_from_slice(png);
    }
    file
}

pub fn icns(images: &[([u8; 4], &[u8])]) -> Vec<u8> {
    let length = |size: usize| u32::try_from(size).unwrap_or(u32::MAX).to_be_bytes();
    let total = ICNS_HEADER
        + images
            .iter()
            .map(|(_, png)| ICNS_HEADER + png.len())
            .sum::<usize>();
    let mut file = b"icns".to_vec();
    file.extend(length(total));
    for (kind, png) in images {
        file.extend(kind);
        file.extend(length(ICNS_HEADER + png.len()));
        file.extend_from_slice(png);
    }
    file
}

#[cfg(test)]
mod tests {
    use super::{icns, ico};

    #[test]
    fn ico_files_list_png_images_after_a_directory() {
        let small = [1_u8, 2, 3];
        let large = [4_u8, 5];
        let file = ico(&[(16, &small), (256, &large)]);
        let mut expected = vec![0, 0, 1, 0, 2, 0];
        expected.extend([16, 16, 0, 0, 1, 0, 32, 0]);
        expected.extend(3_u32.to_le_bytes());
        expected.extend(38_u32.to_le_bytes());
        expected.extend([0, 0, 0, 0, 1, 0, 32, 0]);
        expected.extend(2_u32.to_le_bytes());
        expected.extend(41_u32.to_le_bytes());
        expected.extend(small);
        expected.extend(large);
        assert_eq!(file, expected);
    }

    #[test]
    fn icns_files_hold_typed_entries_with_big_endian_lengths() {
        let file = icns(&[(*b"ic07", &[9, 9]), (*b"ic08", &[7])]);
        let mut expected = b"icns".to_vec();
        expected.extend(27_u32.to_be_bytes());
        expected.extend(b"ic07");
        expected.extend(10_u32.to_be_bytes());
        expected.extend([9, 9]);
        expected.extend(b"ic08");
        expected.extend(9_u32.to_be_bytes());
        expected.push(7);
        assert_eq!(file, expected);
    }
}
