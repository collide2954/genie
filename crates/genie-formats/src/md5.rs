//! MD5 (RFC 1321), for the reference checks of CRAM slices.

use std::fmt::Write;

const SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

const SINES: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

fn round(state: &mut [u32; 4], block: &[u8; 64]) {
    let words: Vec<u32> = block
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes(*word))
        .collect();
    let [mut a, mut b, mut c, mut d] = *state;
    for step in 0..64 {
        let (mix, index) = match step / 16 {
            0 => ((b & c) | (!b & d), step),
            1 => ((d & b) | (!d & c), (5 * step + 1) % 16),
            2 => (b ^ c ^ d, (3 * step + 5) % 16),
            _ => (c ^ (b | !d), (7 * step) % 16),
        };
        let sum = a
            .wrapping_add(mix)
            .wrapping_add(SINES[step])
            .wrapping_add(words[index]);
        (a, d, c) = (d, c, b);
        b = b.wrapping_add(sum.rotate_left(SHIFTS[step]));
    }
    for (value, added) in state.iter_mut().zip([a, b, c, d]) {
        *value = value.wrapping_add(added);
    }
}

pub fn digest(data: &[u8]) -> [u8; 16] {
    let mut state = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    let (blocks, rest) = data.as_chunks::<64>();
    for block in blocks {
        round(&mut state, block);
    }
    let mut tail = rest.to_vec();
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend(((data.len() as u64).wrapping_mul(8)).to_le_bytes());
    for block in tail.as_chunks::<64>().0 {
        round(&mut state, block);
    }
    let mut out = [0; 16];
    for (place, value) in out.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *place = value.to_le_bytes();
    }
    out
}

pub fn hex(digest: &[u8; 16]) -> String {
    digest.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[cfg(test)]
mod tests {
    use super::{digest, hex};

    #[test]
    fn digests_match_the_rfc_examples() {
        let cases = [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(hex(&digest(text.as_bytes())), expected, "{text}");
        }
    }

    #[test]
    fn digests_cross_block_boundaries() {
        let data: Vec<u8> = (0..1_000u32).map(|value| (value % 251) as u8).collect();
        assert_eq!(hex(&digest(&data)), "a24f1e3ef66950e1327f210e3997ba2c");
        assert_eq!(
            hex(&digest(&[b'A'; 56])),
            "a2f3e2024931bd470555002aa5ccc010"
        );
        assert_eq!(
            hex(&digest(&[b'B'; 64])),
            "8e558437ffca99de04c18a63b55a4fad"
        );
    }
}
