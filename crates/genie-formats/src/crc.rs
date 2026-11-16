//! CRC-32 as gzip, BGZF and PNG use it, eight bytes at a time.

const TABLES: [[u32; 256]; 8] = tables();

const fn tables() -> [[u32; 256]; 8] {
    let mut tables = [[0; 256]; 8];
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
        tables[0][index] = value;
        index += 1;
    }
    let mut slice = 1;
    while slice < 8 {
        let mut index = 0;
        while index < 256 {
            let previous = tables[slice - 1][index];
            tables[slice][index] = (previous >> 8) ^ tables[0][(previous & 0xFF) as usize];
            index += 1;
        }
        slice += 1;
    }
    tables
}

/// The CRC-32 of data after data with the check `previous`, 0 at the start.
pub fn update(previous: u32, data: &[u8]) -> u32 {
    let mut crc = !previous;
    let (chunks, rest) = data.as_chunks::<8>();
    for chunk in chunks {
        let low = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) ^ crc;
        let high = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        crc = TABLES[7][(low & 0xFF) as usize]
            ^ TABLES[6][((low >> 8) & 0xFF) as usize]
            ^ TABLES[5][((low >> 16) & 0xFF) as usize]
            ^ TABLES[4][(low >> 24) as usize]
            ^ TABLES[3][(high & 0xFF) as usize]
            ^ TABLES[2][((high >> 8) & 0xFF) as usize]
            ^ TABLES[1][((high >> 16) & 0xFF) as usize]
            ^ TABLES[0][(high >> 24) as usize];
    }
    for byte in rest {
        crc = TABLES[0][((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::{TABLES, update};

    fn bytewise(data: &[u8]) -> u32 {
        let mut crc = !0;
        for byte in data {
            crc = TABLES[0][((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
        }
        !crc
    }

    #[test]
    fn checks_match_the_standard_value_and_the_bytewise_way() {
        assert_eq!(update(0, b"123456789"), 0xCBF4_3926);
        assert_eq!(update(update(0, b"1234"), b"56789"), 0xCBF4_3926);
        assert_eq!(update(0, b""), 0);
        let data: Vec<u8> = (0..1_000_u32)
            .map(|value| (value * 37 % 251) as u8)
            .collect();
        for length in [0, 1, 7, 8, 9, 63, 64, 999, 1_000] {
            assert_eq!(
                update(0, &data[..length]),
                bytewise(&data[..length]),
                "{length}"
            );
        }
    }
}
