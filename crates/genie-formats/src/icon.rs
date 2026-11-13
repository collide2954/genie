const ICO_HEADER: usize = 6;
const ICO_ENTRY: usize = 16;
const ICNS_HEADER: usize = 8;
const RES_EMPTY: [u8; 32] = [
    0, 0, 0, 0, 32, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0,
];
const RES_HEADER: u32 = 32;
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const ICON_FLAGS: u16 = 0x1010;
const GROUP_FLAGS: u16 = 0x1030;
const ENGLISH: u16 = 0x0409;

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

pub fn res(ico: &[u8]) -> Option<Vec<u8>> {
    let word = |at: usize| Some(u16::from_le_bytes(ico.get(at..at + 2)?.try_into().ok()?));
    let long = |at: usize| Some(u32::from_le_bytes(ico.get(at..at + 4)?.try_into().ok()?));
    if word(0)? != 0 || word(2)? != 1 {
        return None;
    }
    let count = word(4)?;
    let mut file = RES_EMPTY.to_vec();
    let mut group = vec![0, 0, 1, 0];
    group.extend(count.to_le_bytes());
    for index in 0..count {
        let entry = ICO_HEADER + ICO_ENTRY * usize::from(index);
        let size = usize::try_from(long(entry + 8)?).ok()?;
        let offset = usize::try_from(long(entry + 12)?).ok()?;
        let image = ico.get(offset..offset.checked_add(size)?)?;
        let id = index + 1;
        resource(&mut file, (RT_ICON, id, ICON_FLAGS), image);
        group.extend_from_slice(ico.get(entry..entry + 12)?);
        group.extend(id.to_le_bytes());
    }
    resource(&mut file, (RT_GROUP_ICON, 1, GROUP_FLAGS), &group);
    Some(file)
}

fn resource(file: &mut Vec<u8>, (kind, name, flags): (u16, u16, u16), data: &[u8]) {
    file.extend(u32::try_from(data.len()).unwrap_or(u32::MAX).to_le_bytes());
    file.extend(RES_HEADER.to_le_bytes());
    file.extend([0xFF, 0xFF]);
    file.extend(kind.to_le_bytes());
    file.extend([0xFF, 0xFF]);
    file.extend(name.to_le_bytes());
    file.extend([0; 4]);
    file.extend(flags.to_le_bytes());
    file.extend(ENGLISH.to_le_bytes());
    file.extend([0; 8]);
    file.extend_from_slice(data);
    file.resize(file.len().next_multiple_of(4), 0);
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
    use super::{icns, ico, res};

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

    #[test]
    fn res_files_hold_each_ico_image_and_an_icon_group() {
        let file = res(&ico(&[(16, &[1, 2, 3]), (256, &[4, 5])])).unwrap();
        let mut expected = vec![0, 0, 0, 0, 32, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0];
        expected.extend([0; 16]);
        expected.extend([3, 0, 0, 0, 32, 0, 0, 0, 0xFF, 0xFF, 3, 0, 0xFF, 0xFF, 1, 0]);
        expected.extend([0, 0, 0, 0, 0x10, 0x10, 0x09, 0x04, 0, 0, 0, 0, 0, 0, 0, 0]);
        expected.extend([1, 2, 3, 0]);
        expected.extend([2, 0, 0, 0, 32, 0, 0, 0, 0xFF, 0xFF, 3, 0, 0xFF, 0xFF, 2, 0]);
        expected.extend([0, 0, 0, 0, 0x10, 0x10, 0x09, 0x04, 0, 0, 0, 0, 0, 0, 0, 0]);
        expected.extend([4, 5, 0, 0]);
        expected.extend([
            34, 0, 0, 0, 32, 0, 0, 0, 0xFF, 0xFF, 14, 0, 0xFF, 0xFF, 1, 0,
        ]);
        expected.extend([0, 0, 0, 0, 0x30, 0x10, 0x09, 0x04, 0, 0, 0, 0, 0, 0, 0, 0]);
        expected.extend([0, 0, 1, 0, 2, 0]);
        expected.extend([16, 16, 0, 0, 1, 0, 32, 0, 3, 0, 0, 0, 1, 0]);
        expected.extend([0, 0, 0, 0, 1, 0, 32, 0, 2, 0, 0, 0, 2, 0]);
        expected.extend([0, 0]);
        assert_eq!(file, expected);
    }

    #[test]
    fn res_files_need_a_whole_ico_file() {
        assert_eq!(res(&[0, 0, 2, 0, 0, 0]), None);
        let mut cut = ico(&[(16, &[1, 2, 3])]);
        cut.pop();
        assert_eq!(res(&cut), None);
        assert_eq!(res(&[0, 0, 1]), None);
    }
}
