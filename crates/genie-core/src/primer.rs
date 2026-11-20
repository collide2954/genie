//! Primer design as Primer3 does it with its default settings: primers by melting temperature,
//! length, GC content, runs of one base, the bases at the 3′ end and complementarity, and pairs
//! around a target by product size, ranked by how far they are from the best Tm and length.

use crate::Region;

/// The salts and the DNA of a PCR, as the defaults of Primer3: millimolar of monovalent ions,
/// of divalent ions and of dNTPs, and nanomolar of each primer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conditions {
    pub monovalent: f64,
    pub divalent: f64,
    pub dntp: f64,
    pub dna: f64,
}

impl Default for Conditions {
    fn default() -> Self {
        Self {
            monovalent: 50.0,
            divalent: 1.5,
            dntp: 0.6,
            dna: 50.0,
        }
    }
}

/// The product sizes of PCR, of Sanger sequencing and of qPCR.
pub const PCR: (u64, u64) = (150, 500);
pub const SANGER: (u64, u64) = (400, 800);
pub const QPCR: (u64, u64) = (70, 150);

/// The limits of primers and pairs, as the defaults of Primer3.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// The shortest, the best and the longest length.
    pub lengths: [usize; 3],
    /// The lowest, the best and the highest melting temperature in °C.
    pub tms: [f64; 3],
    /// The lowest and the highest GC content in percent.
    pub gc: [f64; 2],
    /// The shortest and the longest product.
    pub product: (u64, u64),
    /// The largest difference of the melting temperatures of the two primers of a pair.
    pub tm_difference: f64,
    /// The longest run of one base.
    pub run: usize,
    /// The highest scores of complementarity: of any stretch, and of a stretch at the 3′ end.
    pub complementarity: [i32; 2],
    pub conditions: Conditions,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lengths: [18, 20, 25],
            tms: [57.0, 60.0, 63.0],
            gc: [20.0, 80.0],
            product: PCR,
            tm_difference: 5.0,
            run: 4,
            complementarity: [8, 3],
            conditions: Conditions::default(),
        }
    }
}

/// A primer: its bases on the reference, its bases from its 5′ end, its melting temperature,
/// its GC content and its penalty.
#[derive(Debug, Clone, PartialEq)]
pub struct Primer {
    pub span: Region,
    pub bases: String,
    pub tm: f64,
    pub gc: f64,
    pub penalty: f64,
}

/// A pair of primers around a target: the left primer, the right primer, the product and the
/// penalty of the pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub left: Primer,
    pub right: Primer,
    pub product: Region,
    pub penalty: f64,
}

/// The melting temperature of a primer in °C, by the nearest-neighbor values of SantaLucia
/// (1998) with his correction for salt, as Primer3. Divalent ions count as monovalent ions, as
/// von Ahsen and others (2001) show. A primer with a base that is not A, C, G or T has none.
pub fn tm(bases: &[u8], conditions: Conditions) -> Option<f64> {
    let bases: Vec<u8> = bases.iter().map(u8::to_ascii_uppercase).collect();
    let (first, last) = (*bases.first()?, *bases.last()?);
    if bases.len() < 2 {
        return None;
    }
    let (mut enthalpy, mut entropy) = (0.0, 0.0);
    for pair in bases.windows(2) {
        let (h, s) = nearest([pair[0], pair[1]])?;
        enthalpy += h;
        entropy += s;
    }
    for end in [first, last] {
        let (h, s) = if matches!(end, b'G' | b'C') {
            (0.1, -2.8)
        } else {
            (2.3, 4.1)
        };
        enthalpy += h;
        entropy += s;
    }
    let mut salt = conditions.monovalent;
    if conditions.divalent > conditions.dntp {
        salt += 120.0 * (conditions.divalent - conditions.dntp).sqrt();
    }
    entropy += 0.368 * (bases.len() - 1) as f64 * (salt / 1_000.0).ln();
    let share = if bases == reverse_complement(&bases) {
        entropy -= 1.4;
        conditions.dna / 1e9
    } else {
        conditions.dna / 4e9
    };
    Some(enthalpy * 1_000.0 / (entropy + 1.987 * share.ln()) - 273.15)
}

/// The enthalpy (kcal/mol) and the entropy (cal/(K·mol)) of a pair of neighbor bases on the
/// top strand, as SantaLucia (1998).
fn nearest(pair: [u8; 2]) -> Option<(f64, f64)> {
    Some(match &pair {
        b"AA" | b"TT" => (-7.9, -22.2),
        b"AC" | b"GT" => (-8.4, -22.4),
        b"AG" | b"CT" => (-7.8, -21.0),
        b"AT" => (-7.2, -20.4),
        b"CA" | b"TG" => (-8.5, -22.7),
        b"CC" | b"GG" => (-8.0, -19.9),
        b"CG" => (-10.6, -27.2),
        b"GA" | b"TC" => (-8.2, -22.2),
        b"GC" => (-9.8, -24.4),
        b"TA" => (-7.2, -21.3),
        _ => return None,
    })
}

fn pairs_with(one: u8, other: u8) -> bool {
    matches!(
        (one, other),
        (b'A', b'T') | (b'T', b'A') | (b'C', b'G') | (b'G', b'C')
    )
}

/// The best scores of the antiparallel pairing of two primers: +1 for each pair of
/// complementary bases and −1 for each other pair, over any stretch, and over a stretch that
/// ends at the 3′ end of `one`.
fn complementarity(one: &[u8], other: &[u8]) -> (i32, i32) {
    let reversed: Vec<u8> = other.iter().rev().copied().collect();
    let (length, others) = (one.len().cast_signed(), reversed.len().cast_signed());
    let (mut any, mut end) = (0, 0);
    for shift in -(length - 1)..others {
        let mut run = i32::MIN / 2;
        for at in 0..length {
            let place = at + shift;
            if place < 0 || place >= others {
                continue;
            }
            let score = if pairs_with(one[at as usize], reversed[place as usize]) {
                1
            } else {
                -1
            };
            run = score.max(run + score);
            any = any.max(run);
            if at == length - 1 {
                end = end.max(run);
            }
        }
    }
    (any, end)
}

/// The melting temperature and the GC content of a primer that keeps to the settings.
fn check(bases: &[u8], settings: &Settings) -> Option<(f64, f64)> {
    if bases.iter().any(|base| !b"ACGT".contains(base)) {
        return None;
    }
    let strong = |part: &[u8]| {
        part.iter()
            .filter(|base| matches!(base, b'G' | b'C'))
            .count()
    };
    let gc = strong(bases) as f64 / bases.len() as f64 * 100.0;
    if gc < settings.gc[0] || gc > settings.gc[1] {
        return None;
    }
    let longest = bases
        .chunk_by(|one, other| one == other)
        .map(<[u8]>::len)
        .max()
        .unwrap_or(0);
    let end = strong(&bases[bases.len().saturating_sub(5)..]);
    if longest > settings.run || !(1..=3).contains(&end) {
        return None;
    }
    let melting = tm(bases, settings.conditions)?;
    if melting < settings.tms[0] || melting > settings.tms[2] {
        return None;
    }
    let (any, end) = complementarity(bases, bases);
    if any > settings.complementarity[0] || end > settings.complementarity[1] {
        return None;
    }
    Some((melting, gc))
}

/// The primers that keep to the settings, from the places that `spans` gives, each with the
/// bases that the primer has on its strand. The best ones first, at most `CANDIDATES`.
fn candidates(
    spans: impl Iterator<Item = (Region, Vec<u8>)>,
    masked: &[u64],
    settings: &Settings,
) -> Vec<Primer> {
    let mut found: Vec<Primer> = spans
        .filter(|(span, _)| {
            let first = masked.partition_point(|&place| place < span.start());
            masked.get(first).is_none_or(|&place| place >= span.end())
        })
        .filter_map(|(span, bases)| {
            let (melting, gc) = check(&bases, settings)?;
            let penalty = (melting - settings.tms[1]).abs()
                + (bases.len() as f64 - settings.lengths[1] as f64).abs();
            Some(Primer {
                span,
                bases: String::from_utf8_lossy(&bases).into_owned(),
                tm: melting,
                gc,
                penalty,
            })
        })
        .collect();
    found.sort_by(|one, other| one.penalty.total_cmp(&other.penalty));
    found.truncate(CANDIDATES);
    found
}

/// The most primers on each side that genie pairs.
const CANDIDATES: usize = 250;

/// The pairs of primers around a target in a template that starts at `start`, the best first,
/// at most `count`. Primers stay off the masked places, for example of known variants.
pub fn design(
    template: &[u8],
    start: u64,
    target: Region,
    masked: &[u64],
    settings: &Settings,
    count: usize,
) -> Vec<Pair> {
    let bases: Vec<u8> = template.iter().map(u8::to_ascii_uppercase).collect();
    let end = start + bases.len() as u64;
    let (shortest, longest) = settings.product;
    let mut masked = masked.to_vec();
    masked.sort_unstable();
    let [fewest, _, most] = settings.lengths;
    let slice =
        |span: Region| &bases[(span.start() - start) as usize..(span.end() - start) as usize];
    let lefts = candidates(
        (fewest..=most)
            .flat_map(|length| {
                let first = start.max(target.end().saturating_sub(longest));
                let last = target.start().saturating_sub(length as u64);
                (first..=last)
                    .filter(move |_| target.start() >= length as u64)
                    .map(move |from| Region::between(from, from + length as u64))
            })
            .filter(|span| span.start() >= start && span.end() <= end)
            .map(|span| (span, slice(span).to_vec())),
        &masked,
        settings,
    );
    let rights = candidates(
        (fewest..=most)
            .flat_map(|length| {
                let first = target.end() + length as u64;
                let last = end.min(target.start() + longest);
                (first..=last).map(move |to| Region::between(to - length as u64, to))
            })
            .filter(|span| span.start() >= start && span.end() <= end)
            .map(|span| (span, reverse_complement(slice(span)))),
        &masked,
        settings,
    );
    let mut options: Vec<(f64, usize, usize)> = Vec::new();
    for (one, left) in lefts.iter().enumerate() {
        for (other, right) in rights.iter().enumerate() {
            let size = right.span.end().saturating_sub(left.span.start());
            if size < shortest
                || size > longest
                || (left.tm - right.tm).abs() > settings.tm_difference
            {
                continue;
            }
            options.push((left.penalty + right.penalty, one, other));
        }
    }
    options.sort_by(|one, other| one.0.total_cmp(&other.0));
    let mut pairs = Vec::new();
    for (penalty, one, other) in options {
        if pairs.len() == count {
            break;
        }
        let (left, right) = (&lefts[one], &rights[other]);
        let (one_any, one_end) = complementarity(left.bases.as_bytes(), right.bases.as_bytes());
        let (_, other_end) = complementarity(right.bases.as_bytes(), left.bases.as_bytes());
        if one_any > settings.complementarity[0]
            || one_end.max(other_end) > settings.complementarity[1]
        {
            continue;
        }
        pairs.push(Pair {
            left: left.clone(),
            right: right.clone(),
            product: Region::between(left.span.start(), right.span.end()),
            penalty,
        });
    }
    pairs
}

/// The reverse complement of bases.
pub fn reverse_complement(bases: &[u8]) -> Vec<u8> {
    bases
        .iter()
        .rev()
        .map(|base| match base.to_ascii_uppercase() {
            b'A' => b'T',
            b'C' => b'G',
            b'G' => b'C',
            b'T' => b'A',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Conditions, Settings, complementarity, design, reverse_complement, tm};
    use crate::Region;

    /// Bases that look random, with half G and C.
    fn template(length: usize, seed: u64) -> Vec<u8> {
        let mut state = seed;
        (0..length)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                b"ACGT"[((state >> 33) % 4) as usize]
            })
            .collect()
    }

    #[test]
    fn melting_follows_the_nearest_neighbors_and_the_salt() {
        let melting = tm(b"ACGTCGATCG", Conditions::default()).unwrap();
        assert!((melting - 33.63).abs() < 0.05, "{melting}");
        let low = tm(b"ATATATTATAATTTAATATA", Conditions::default()).unwrap();
        let high = tm(b"GCGGCGCCGCGGCCGCGGCG", Conditions::default()).unwrap();
        assert!(low < 50.0 && high > 75.0, "{low} {high}");
        let less_salt = Conditions {
            monovalent: 10.0,
            divalent: 0.0,
            ..Conditions::default()
        };
        let primer = b"AGCTGACTGACGTAGCTAGC";
        assert!(tm(primer, less_salt).unwrap() < tm(primer, Conditions::default()).unwrap());
        assert_eq!(tm(b"ACGNT", Conditions::default()), None);
        assert_eq!(reverse_complement(b"AACGT"), b"ACGTT");
    }

    #[test]
    fn complementarity_scores_stretches_and_ends() {
        assert_eq!(complementarity(b"GAATTC", b"GAATTC"), (6, 6));
        assert_eq!(complementarity(b"AAAAAA", b"AAAAAA"), (0, 0));
        assert_eq!(complementarity(b"AAAAAA", b"TTTTTT"), (6, 6));
        let (any, end) = complementarity(b"GGGGGGAAAA", b"CCCCCCAAAA");
        assert_eq!(any, 6);
        assert!(end < 3, "{end}");
    }

    #[test]
    fn pairs_flank_the_target_and_come_best_first() {
        let bases = template(2_000, 7);
        let target = Region::between(10_900, 11_000);
        let settings = Settings::default();
        let pairs = design(&bases, 10_000, target, &[], &settings, 5);
        assert_eq!(pairs.len(), 5);
        for pair in &pairs {
            assert!(pair.left.span.end() <= target.start(), "{pair:?}");
            assert!(pair.right.span.start() >= target.end(), "{pair:?}");
            assert_eq!(
                pair.product,
                Region::between(pair.left.span.start(), pair.right.span.end())
            );
            assert!((150..=500).contains(&pair.product.len()), "{pair:?}");
            for primer in [&pair.left, &pair.right] {
                assert!((57.0..=63.0).contains(&primer.tm), "{primer:?}");
                assert!((18..=25).contains(&primer.bases.len()), "{primer:?}");
                assert!((20.0..=80.0).contains(&primer.gc), "{primer:?}");
            }
            let at = (pair.right.span.start() - 10_000) as usize
                ..(pair.right.span.end() - 10_000) as usize;
            assert_eq!(pair.right.bases.as_bytes(), reverse_complement(&bases[at]));
            assert!((pair.left.tm - pair.right.tm).abs() <= 5.0);
        }
        assert!(pairs.windows(2).all(|two| two[0].penalty <= two[1].penalty));
    }

    #[test]
    fn primers_stay_off_masked_places() {
        let bases = template(2_000, 11);
        let target = Region::between(10_900, 11_000);
        let settings = Settings::default();
        let best = design(&bases, 10_000, target, &[], &settings, 1);
        let left = best[0].left.span;
        let masked: Vec<u64> = (left.start()..left.end()).collect();
        let pairs = design(&bases, 10_000, target, &masked, &settings, 20);
        assert_ne!(pairs.len(), 0);
        for pair in &pairs {
            assert!(masked.iter().all(|place| !pair.left.span.contains(*place)));
        }
    }

    #[test]
    fn templates_without_good_primers_give_no_pairs() {
        let bases = vec![b'A'; 2_000];
        let target = Region::between(10_900, 11_000);
        assert_eq!(
            design(&bases, 10_000, target, &[], &Settings::default(), 5),
            []
        );
    }
}
