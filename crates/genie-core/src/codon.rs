const CODE: &[u8; 64] = b"FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG";

pub fn complement(base: u8) -> u8 {
    match base {
        b'A' => b'T',
        b'C' => b'G',
        b'G' => b'C',
        b'T' => b'A',
        b'a' => b't',
        b'c' => b'g',
        b'g' => b'c',
        b't' => b'a',
        other => other,
    }
}

pub fn translate(codon: [u8; 3]) -> u8 {
    let mut index = 0;
    for base in codon {
        let value = match base.to_ascii_uppercase() {
            b'T' => 0,
            b'C' => 1,
            b'A' => 2,
            b'G' => 3,
            _ => return b'X',
        };
        index = index * 4 + value;
    }
    CODE[index]
}

#[cfg(test)]
mod tests {
    use super::{complement, translate};

    #[test]
    fn complements_bases_and_keeps_case() {
        let bases: Vec<u8> = b"ACGTNacgtn-"
            .iter()
            .map(|&base| complement(base))
            .collect();
        assert_eq!(bases, b"TGCANtgcan-");
    }

    #[test]
    fn translates_with_the_standard_code() {
        let protein: Vec<u8> = [
            b"ATG", b"TTT", b"TGG", b"GGC", b"TAA", b"TAG", b"TGA", b"CGA", b"AAG", b"GAT",
        ]
        .iter()
        .map(|codon| translate(**codon))
        .collect();
        assert_eq!(protein, b"MFWG***RKD");
    }

    #[test]
    fn translation_ignores_case_and_marks_unknown_codons() {
        assert_eq!(translate(*b"atg"), b'M');
        assert_eq!(translate(*b"ANG"), b'X');
    }
}
