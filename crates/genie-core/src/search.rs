//! Finds a short DNA sequence (IUPAC codes allowed) on both strands, with up to a few mismatches.

use std::fmt;

use crate::{Region, Strand};

pub const SHORTEST: usize = 2;
pub const LONGEST: usize = 200;
pub const MOST_MISMATCHES: u8 = 3;
const BASES_PER_MISMATCH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternError {
    Empty,
    Letter(char),
    Short,
    Long,
    Mismatches(u8),
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Type the bases that you want to find."),
            Self::Letter(letter) => write!(
                f,
                "{letter} is not a base. Use A, C, G, T or IUPAC codes such as N, R and Y."
            ),
            Self::Short => write!(f, "Type {SHORTEST} bases or more."),
            Self::Long => write!(f, "genie finds up to {LONGEST} bases."),
            Self::Mismatches(count) => {
                let least = usize::from(*count) * BASES_PER_MISMATCH;
                let noun = if *count == 1 {
                    "mismatch"
                } else {
                    "mismatches"
                };
                write!(f, "Type {least} bases or more to allow {count} {noun}.")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    text: String,
    forward: Vec<u8>,
    reverse: Vec<u8>,
}

const A: u8 = 1;
const C: u8 = 2;
const G: u8 = 4;
const T: u8 = 8;

fn code(letter: char) -> Option<u8> {
    Some(match letter.to_ascii_uppercase() {
        'A' => A,
        'C' => C,
        'G' => G,
        'T' | 'U' => T,
        'R' => A | G,
        'Y' => C | T,
        'S' => C | G,
        'W' => A | T,
        'K' => G | T,
        'M' => A | C,
        'B' => C | G | T,
        'D' => A | G | T,
        'H' => A | C | T,
        'V' => A | C | G,
        'N' => A | C | G | T,
        _ => return None,
    })
}

fn complement(mask: u8) -> u8 {
    ((mask & A) << 3) | ((mask & T) >> 3) | ((mask & C) << 1) | ((mask & G) >> 1)
}

fn base(byte: u8) -> u8 {
    match byte.to_ascii_uppercase() {
        b'A' => A,
        b'C' => C,
        b'G' => G,
        b'T' => T,
        _ => 0,
    }
}

fn ignored(letter: char) -> bool {
    letter.is_whitespace() || letter.is_ascii_digit() || matches!(letter, '-' | '\'' | '′' | '’')
}

impl Pattern {
    pub fn parse(text: &str) -> Result<Self, PatternError> {
        let mut letters = String::new();
        let mut forward = Vec::new();
        for letter in text.chars().filter(|letter| !ignored(*letter)) {
            let mask = code(letter).ok_or(PatternError::Letter(letter))?;
            let upper = letter.to_ascii_uppercase();
            letters.push(if upper == 'U' { 'T' } else { upper });
            forward.push(mask);
        }
        match forward.len() {
            0 => return Err(PatternError::Empty),
            length if length < SHORTEST => return Err(PatternError::Short),
            length if length > LONGEST => return Err(PatternError::Long),
            _ => {}
        }
        let reverse = forward.iter().rev().map(|mask| complement(*mask)).collect();
        Ok(Self {
            text: letters,
            forward,
            reverse,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.forward.len()
    }

    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }

    pub fn is_palindrome(&self) -> bool {
        self.forward == self.reverse
    }

    pub fn check(&self, mismatches: u8) -> Result<(), PatternError> {
        if self.len() < usize::from(mismatches) * BASES_PER_MISMATCH {
            return Err(PatternError::Mismatches(mismatches));
        }
        Ok(())
    }
}

fn wrong(masks: &[u8], window: &[u8], most: usize) -> Option<usize> {
    let mut count = 0;
    for (mask, base) in masks.iter().zip(window) {
        if mask & base == 0 {
            count += 1;
            if count > most {
                return None;
            }
        }
    }
    Some(count)
}

fn wrong_offsets(masks: &[u8], window: &[u8]) -> Vec<u32> {
    masks
        .iter()
        .zip(window)
        .enumerate()
        .filter(|(_, (mask, base))| *mask & *base == 0)
        .map(|(offset, _)| offset as u32)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub span: Region,
    pub strand: Strand,
    pub mismatches: Vec<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub hits: Vec<Hit>,
    pub complete: bool,
}

pub fn find(pattern: &Pattern, bases: &[u8], start: u64, mismatches: u8, limit: usize) -> Found {
    let length = pattern.len();
    let reference: Vec<u8> = bases.iter().map(|byte| base(*byte)).collect();
    let strands: &[(&[u8], Strand)] = if pattern.is_palindrome() {
        &[(&pattern.forward, Strand::Unknown)]
    } else {
        &[
            (&pattern.forward, Strand::Forward),
            (&pattern.reverse, Strand::Reverse),
        ]
    };
    let mut hits = Vec::new();
    if length == 0 || reference.len() < length {
        return Found {
            hits,
            complete: true,
        };
    }
    let most = usize::from(mismatches);
    for (at, window) in reference.windows(length).enumerate() {
        for (masks, strand) in strands {
            let Some(count) = wrong(masks, window, most) else {
                continue;
            };
            if hits.len() == limit {
                return Found {
                    hits,
                    complete: false,
                };
            }
            let from = start + at as u64;
            hits.push(Hit {
                span: Region::between(from, from + length as u64),
                strand: *strand,
                mismatches: if count == 0 {
                    Vec::new()
                } else {
                    wrong_offsets(masks, window)
                },
            });
        }
    }
    Found {
        hits,
        complete: true,
    }
}

#[cfg(test)]
mod tests {
    use super::{Found, Hit, Pattern, PatternError, find};
    use crate::{Region, Strand};

    fn hit(start: u64, end: u64, strand: Strand, mismatches: &[u32]) -> Hit {
        Hit {
            span: Region::between(start, end),
            strand,
            mismatches: mismatches.to_vec(),
        }
    }

    fn all(pattern: &str, bases: &str, start: u64, mismatches: u8) -> Vec<Hit> {
        let pattern = Pattern::parse(pattern).unwrap();
        let found = find(&pattern, bases.as_bytes(), start, mismatches, 1_000);
        assert!(found.complete);
        found.hits
    }

    #[test]
    fn patterns_keep_bases_and_iupac_codes_only() {
        let pattern = Pattern::parse(" gga tcc\n12 Nu ").unwrap();
        assert_eq!(pattern.text(), "GGATCCNT");
        assert_eq!(pattern.len(), 8);
        assert_eq!(Pattern::parse("5'-GGATCC-3'").unwrap().text(), "GGATCC");
        assert_eq!(Pattern::parse(" 1 2 "), Err(PatternError::Empty));
        assert_eq!(Pattern::parse("ACXT"), Err(PatternError::Letter('X')));
        assert_eq!(Pattern::parse("A"), Err(PatternError::Short));
        assert_eq!(Pattern::parse(&"A".repeat(201)), Err(PatternError::Long));
        assert!(Pattern::parse(&"ACGT".repeat(50)).is_ok());
        assert!(Pattern::parse("RYSWKMBDHVN").is_ok());
    }

    #[test]
    fn errors_say_what_to_type() {
        let texts: Vec<String> = [
            PatternError::Empty,
            PatternError::Letter('X'),
            PatternError::Short,
            PatternError::Long,
            PatternError::Mismatches(1),
            PatternError::Mismatches(3),
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(
            texts,
            [
                "Type the bases that you want to find.",
                "X is not a base. Use A, C, G, T or IUPAC codes such as N, R and Y.",
                "Type 2 bases or more.",
                "genie finds up to 200 bases.",
                "Type 4 bases or more to allow 1 mismatch.",
                "Type 12 bases or more to allow 3 mismatches.",
            ]
        );
    }

    #[test]
    fn palindromes_read_the_same_on_both_strands() {
        for text in ["GAATTC", "GGATCC", "ACGT", "RY", "NN"] {
            assert!(Pattern::parse(text).unwrap().is_palindrome(), "{text}");
        }
        for text in ["ACGTT", "GACC", "RR"] {
            assert!(!Pattern::parse(text).unwrap().is_palindrome(), "{text}");
        }
    }

    #[test]
    fn mismatches_need_four_bases_each() {
        let six = Pattern::parse("GACCTA").unwrap();
        assert_eq!(six.check(0), Ok(()));
        assert_eq!(six.check(1), Ok(()));
        assert_eq!(six.check(2), Err(PatternError::Mismatches(2)));
        let twelve = Pattern::parse("GACCTAGACCTA").unwrap();
        assert_eq!(twelve.check(3), Ok(()));
        assert_eq!(twelve.check(4), Err(PatternError::Mismatches(4)));
    }

    #[test]
    fn matches_come_from_both_strands_in_order() {
        assert_eq!(
            all("GACC", "AAGACCTTGGTCAA", 100, 0),
            [
                hit(102, 106, Strand::Forward, &[]),
                hit(108, 112, Strand::Reverse, &[]),
            ]
        );
        assert_eq!(
            all("gacc", "aagaccttggtcaa", 0, 0),
            [
                hit(2, 6, Strand::Forward, &[]),
                hit(8, 12, Strand::Reverse, &[]),
            ]
        );
        assert_eq!(
            all("ACG", "ACGT", 0, 0),
            [
                hit(0, 3, Strand::Forward, &[]),
                hit(1, 4, Strand::Reverse, &[])
            ]
        );
    }

    #[test]
    fn iupac_codes_match_their_bases() {
        assert_eq!(
            all("GRCC", "GACCGGCCGTCC", 0, 0),
            [
                hit(0, 4, Strand::Forward, &[]),
                hit(4, 8, Strand::Forward, &[]),
                hit(4, 8, Strand::Reverse, &[]),
            ]
        );
    }

    #[test]
    fn a_palindrome_shows_once_for_both_strands() {
        assert_eq!(
            all("GAATTC", "CCGAATTCGG", 0, 0),
            [hit(2, 8, Strand::Unknown, &[])]
        );
    }

    #[test]
    fn near_matches_list_their_wrong_bases() {
        let bases = "GGAACCGCTAGG";
        assert_eq!(all("AACCGGTA", bases, 50, 0), []);
        assert_eq!(
            all("AACCGGTA", bases, 50, 1),
            [hit(52, 60, Strand::Forward, &[5])]
        );
        assert_eq!(
            all("CGGTTCC", "AAGGAACCTTT", 0, 1),
            [hit(2, 9, Strand::Reverse, &[6])]
        );
    }

    #[test]
    fn unknown_reference_bases_never_match() {
        assert_eq!(all("ACGA", "ACNA", 0, 0), []);
        assert_eq!(all("NNNN", "ACNA", 0, 0), []);
        assert_eq!(
            all("ACGA", "ACNA", 0, 1),
            [hit(0, 4, Strand::Forward, &[2])]
        );
    }

    #[test]
    fn the_search_stops_at_the_limit() {
        let pattern = Pattern::parse("AA").unwrap();
        let found = find(&pattern, b"AAAAAA", 0, 0, 3);
        assert_eq!(found.hits.len(), 3);
        assert!(!found.complete);
        assert_eq!(
            find(&pattern, b"A", 0, 0, 3),
            Found {
                hits: Vec::new(),
                complete: true
            }
        );
    }
}
