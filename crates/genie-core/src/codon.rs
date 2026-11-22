use vstd::prelude::*;

verus! {

/// The amino acids of the standard genetic code, in the order of the codons with T, C, A and G
/// at each base: TTT, TTC, TTA, TTG, TCT and on to GGG. A stop codon is `*`.
pub const CODE: [u8; 64] = *b"FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG";

/// The value of a base in the order of the code: T, C, A, G, in upper or lower case.
pub open spec fn spec_value(base: u8) -> Option<int> {
    if base == b'T' || base == b't' {
        Some(0)
    } else if base == b'C' || base == b'c' {
        Some(1)
    } else if base == b'A' || base == b'a' {
        Some(2)
    } else if base == b'G' || base == b'g' {
        Some(3)
    } else {
        None
    }
}

/// The complement of a base: A and T, C and G, in the same case. Other bytes stay.
pub open spec fn spec_complement(base: u8) -> u8 {
    if base == b'A' {
        b'T'
    } else if base == b'C' {
        b'G'
    } else if base == b'G' {
        b'C'
    } else if base == b'T' {
        b'A'
    } else if base == b'a' {
        b't'
    } else if base == b'c' {
        b'g'
    } else if base == b'g' {
        b'c'
    } else if base == b't' {
        b'a'
    } else {
        base
    }
}

pub fn complement(base: u8) -> (other: u8)
    ensures
        other == spec_complement(base),
        spec_complement(other) == base,
{
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

fn value(base: u8) -> (value: Option<usize>)
    ensures
        match spec_value(base) {
            Some(known) => value == Some(known as usize),
            None => value is None,
        },
{
    match base {
        b'T' | b't' => Some(0),
        b'C' | b'c' => Some(1),
        b'A' | b'a' => Some(2),
        b'G' | b'g' => Some(3),
        _ => None,
    }
}

/// The amino acid of a codon by the standard genetic code, or X when a base is not A, C, G or
/// T.
pub fn translate(codon: [u8; 3]) -> (amino: u8)
    ensures
        match (spec_value(codon[0]), spec_value(codon[1]), spec_value(codon[2])) {
            (Some(first), Some(second), Some(third)) => amino == CODE[16 * first + 4 * second
                + third],
            _ => amino == b'X',
        },
{
    let (Some(first), Some(second), Some(third)) = (
        value(codon[0]),
        value(codon[1]),
        value(codon[2]),
    ) else {
        return b'X';
    };
    CODE[16 * first + 4 * second + third]
}

} // verus!

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
