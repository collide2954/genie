//! The effect of a variant on a coding transcript in HGVS notation: the change of the coding
//! sequence (c.) and of the protein (p.).

use crate::codon::{complement, translate};
use crate::{Feature, Region, Strand};

/// How far from an exon edge a variant in an intron counts as a splice site.
const SPLICE: u64 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    /// The change of the coding sequence, for example `c.5095C>T`.
    pub coding: Option<String>,
    /// The change of the protein, for example `p.Arg1699Trp`.
    pub protein: Option<String>,
    /// The part of the transcript outside the coding sequence, for example "intron".
    pub place: Option<&'static str>,
}

impl Effect {
    fn place(place: &'static str) -> Self {
        Self {
            coding: None,
            protein: None,
            place: Some(place),
        }
    }
}

/// One change of the genome: the deleted bases from `start` become the inserted bases.
struct Edit {
    start: u64,
    deleted: Vec<u8>,
    inserted: Vec<u8>,
}

impl Edit {
    /// The change without the bases that the alleles share at their start and end.
    fn new(position: u64, reference: &[u8], alternate: &[u8]) -> Self {
        let same = |a: &u8, b: &u8| a.eq_ignore_ascii_case(b);
        let prefix = reference
            .iter()
            .zip(alternate)
            .take_while(|(a, b)| same(a, b))
            .count();
        let (reference, alternate) = (&reference[prefix..], &alternate[prefix..]);
        let suffix = reference
            .iter()
            .rev()
            .zip(alternate.iter().rev())
            .take_while(|(a, b)| same(a, b))
            .count();
        Self {
            start: position + prefix as u64,
            deleted: reference[..reference.len() - suffix].to_ascii_uppercase(),
            inserted: alternate[..alternate.len() - suffix].to_ascii_uppercase(),
        }
    }

    fn end(&self) -> u64 {
        self.start + self.deleted.len() as u64
    }
}

fn reverse_complement(bases: &[u8]) -> Vec<u8> {
    bases.iter().rev().map(|base| complement(*base)).collect()
}

fn amino(letter: u8) -> &'static str {
    const NAMES: [(u8, &str); 22] = [
        (b'A', "Ala"),
        (b'R', "Arg"),
        (b'N', "Asn"),
        (b'D', "Asp"),
        (b'C', "Cys"),
        (b'Q', "Gln"),
        (b'E', "Glu"),
        (b'G', "Gly"),
        (b'H', "His"),
        (b'I', "Ile"),
        (b'L', "Leu"),
        (b'K', "Lys"),
        (b'M', "Met"),
        (b'F', "Phe"),
        (b'P', "Pro"),
        (b'S', "Ser"),
        (b'T', "Thr"),
        (b'W', "Trp"),
        (b'Y', "Tyr"),
        (b'V', "Val"),
        (b'*', "Ter"),
        (b'U', "Sec"),
    ];
    NAMES
        .iter()
        .find(|(known, _)| *known == letter)
        .map_or("Xaa", |(_, name)| name)
}

fn aminos(letters: &[u8]) -> String {
    letters.iter().map(|letter| amino(*letter)).collect()
}

/// The protein of a coding sequence, up to and with the first stop.
fn translated(cds: &[u8]) -> Vec<u8> {
    let mut protein = Vec::with_capacity(cds.len() / 3);
    for codon in cds.as_chunks::<3>().0 {
        let letter = translate(*codon);
        protein.push(letter);
        if letter == b'*' {
            break;
        }
    }
    protein
}

/// The HGVS protein change between two proteins. `at` is the amino acid at the edit, for a
/// change that keeps the protein.
fn protein_change(reference: &[u8], alternate: &[u8], frameshift: bool, at: usize) -> String {
    let first = reference
        .iter()
        .zip(alternate)
        .take_while(|(a, b)| a == b)
        .count();
    if first == reference.len().min(alternate.len()) && reference.len() == alternate.len() {
        let at = at.min(reference.len().saturating_sub(1));
        return format!(
            "p.{}{}=",
            amino(reference.get(at).copied().unwrap_or(b'X')),
            at + 1
        );
    }
    let name = |index: usize| amino(reference.get(index).copied().unwrap_or(b'X'));
    if first == 0 && reference.first() == Some(&b'M') {
        return "p.Met1?".to_owned();
    }
    if alternate.get(first) == Some(&b'*') {
        return format!("p.{}{}Ter", name(first), first + 1);
    }
    if reference.get(first) == Some(&b'*') {
        let next = alternate.get(first).copied().unwrap_or(b'X');
        return format!("p.Ter{}{}ext*?", first + 1, amino(next));
    }
    if frameshift {
        return format!("p.{}{}fs", name(first), first + 1);
    }
    let shared = reference[first..]
        .iter()
        .rev()
        .zip(alternate[first..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let lost = &reference[first..reference.len() - shared];
    let gained = &alternate[first..alternate.len() - shared];
    let end = first + lost.len().saturating_sub(1);
    match (lost.len(), gained.len()) {
        (0, _) if first > 0 => format!(
            "p.{}{}_{}{}ins{}",
            name(first - 1),
            first,
            name(first),
            first + 1,
            aminos(gained)
        ),
        (1, 1) => format!("p.{}{}{}", name(first), first + 1, aminos(gained)),
        (1, 0) => format!("p.{}{}del", name(first), first + 1),
        (_, 0) => format!("p.{}{}_{}{}del", name(first), first + 1, name(end), end + 1),
        (1, _) => format!("p.{}{}delins{}", name(first), first + 1, aminos(gained)),
        _ => format!(
            "p.{}{}_{}{}delins{}",
            name(first),
            first + 1,
            name(end),
            end + 1,
            aminos(gained)
        ),
    }
}

/// The coding parts of a transcript in genome order, and where the coding sequence of each
/// genome position is.
struct Coding {
    blocks: Vec<Region>,
    length: u64,
    forward: bool,
}

impl Coding {
    fn new(transcript: &Feature, coding: Region) -> Self {
        let mut blocks: Vec<Region> = transcript
            .blocks()
            .iter()
            .filter_map(|block| block.intersection(coding))
            .filter(|block| !block.is_empty())
            .collect();
        blocks.sort_by_key(|block| block.start());
        let length = blocks.iter().map(|block| block.len()).sum();
        Self {
            blocks,
            length,
            forward: transcript.strand() != Strand::Reverse,
        }
    }

    /// The place of a genome position in the coding bases in genome order.
    fn index(&self, position: u64) -> Option<u64> {
        let mut before = 0;
        for block in &self.blocks {
            if block.contains(position) {
                return Some(before + position - block.start());
            }
            before += block.len();
        }
        None
    }

    /// The c. number of a genome position in the coding sequence, from 1.
    fn number(&self, position: u64) -> Option<u64> {
        let index = self.index(position)?;
        Some(if self.forward {
            index + 1
        } else {
            self.length - index
        })
    }
}

/// The effect of a variant on a transcript with a coding part. `bases` gives the reference
/// bases of a region of the contig.
pub fn effect(
    transcript: &Feature,
    (position, reference, alternate): (u64, &[u8], &[u8]),
    bases: &mut dyn FnMut(Region) -> Option<Vec<u8>>,
) -> Option<Effect> {
    let coding = transcript.coding()?;
    let edit = Edit::new(position, reference, alternate);
    if edit.deleted.is_empty() && edit.inserted.is_empty() {
        return None;
    }
    let exons = transcript.blocks();
    let exon = |position: u64| exons.iter().position(|exon| exon.contains(position));
    let forward = transcript.strand() != Strand::Reverse;
    let (first, last) = if edit.deleted.is_empty() {
        (edit.start.checked_sub(1)?, edit.start)
    } else {
        (edit.start, edit.end() - 1)
    };
    match (exon(first), exon(last)) {
        (Some(a), Some(b)) if a == b => {
            if coding.contains(first) && coding.contains(last) {
                coded(transcript, coding, &edit, bases)
            } else if coding.contains(first) || coding.contains(last) {
                Some(Effect {
                    protein: Some("p.?".to_owned()),
                    ..Effect::place("start or stop codon")
                })
            } else {
                let upstream = (last < coding.start()) == forward;
                Some(Effect::place(if upstream {
                    "5′ UTR"
                } else {
                    "3′ UTR"
                }))
            }
        }
        (None, None) => intronic(transcript, coding, &edit, (first, last)),
        _ => Some(Effect {
            protein: Some("p.?".to_owned()),
            ..Effect::place("splice site")
        }),
    }
}

/// A variant in an intron: a splice site within 2 bases of an exon, else the intron.
fn intronic(
    transcript: &Feature,
    coding: Region,
    edit: &Edit,
    (first, last): (u64, u64),
) -> Option<Effect> {
    let span = transcript.span();
    if last < span.start() || first >= span.end() {
        return None;
    }
    let exons = transcript.blocks();
    let after = exons
        .iter()
        .filter(|exon| exon.end() <= first)
        .map(|exon| (first - (exon.end() - 1), exon.end() - 1))
        .min();
    let before = exons
        .iter()
        .filter(|exon| exon.start() > last)
        .map(|exon| (exon.start() - last, exon.start()))
        .min();
    let near = [after, before]
        .into_iter()
        .flatten()
        .min_by_key(|(distance, _)| *distance)?;
    if near.0 > SPLICE {
        return Some(Effect::place("intron"));
    }
    let numbers = Coding::new(transcript, coding);
    let forward = numbers.forward;
    let snv = edit.deleted.len() == 1 && edit.inserted.len() == 1;
    let coding = numbers.number(near.1).filter(|_| snv).map(|number| {
        let downstream = (near.1 < edit.start) == forward;
        let sign = if downstream { '+' } else { '-' };
        let (from, to) = if forward {
            (edit.deleted[0], edit.inserted[0])
        } else {
            (complement(edit.deleted[0]), complement(edit.inserted[0]))
        };
        format!(
            "c.{number}{sign}{}{}>{}",
            near.0,
            char::from(from),
            char::from(to)
        )
    });
    Some(Effect {
        coding,
        protein: Some("p.?".to_owned()),
        place: Some("splice site"),
    })
}

/// A variant in the coding sequence.
fn coded(
    transcript: &Feature,
    coding: Region,
    edit: &Edit,
    bases: &mut dyn FnMut(Region) -> Option<Vec<u8>>,
) -> Option<Effect> {
    let numbers = Coding::new(transcript, coding);
    let mut genomic = Vec::with_capacity(usize::try_from(numbers.length).ok()?);
    for block in &numbers.blocks {
        genomic.extend(bases(*block)?.to_ascii_uppercase());
    }
    let at = if edit.deleted.is_empty() {
        usize::try_from(numbers.index(edit.start - 1)? + 1).ok()?
    } else {
        usize::try_from(numbers.index(edit.start)?).ok()?
    };
    let mut changed = genomic[..at].to_vec();
    changed.extend(&edit.inserted);
    changed.extend(genomic.get(at + edit.deleted.len()..)?);
    let forward = numbers.forward;
    let (reference, alternate) = if forward {
        (genomic, changed)
    } else {
        (reverse_complement(&genomic), reverse_complement(&changed))
    };
    let start = numbers.number(edit.start)?;
    let coding = coding_change(&numbers, edit)?;
    let frameshift = !edit
        .deleted
        .len()
        .abs_diff(edit.inserted.len())
        .is_multiple_of(3);
    let around = usize::try_from(start.saturating_sub(1) / 3).ok()?;
    let protein = protein_change(
        &translated(&reference),
        &translated(&alternate),
        frameshift,
        around,
    );
    Some(Effect {
        coding: Some(coding),
        protein: Some(protein),
        place: None,
    })
}

/// The HGVS change of the coding sequence.
fn coding_change(numbers: &Coding, edit: &Edit) -> Option<String> {
    let strand = |bases: &[u8]| -> String {
        let bases = if numbers.forward {
            bases.to_vec()
        } else {
            reverse_complement(bases)
        };
        String::from_utf8_lossy(&bases).into_owned()
    };
    if edit.deleted.is_empty() {
        let (left, right) = (numbers.number(edit.start - 1)?, numbers.number(edit.start)?);
        let (low, high) = (left.min(right), left.max(right));
        return Some(format!("c.{low}_{high}ins{}", strand(&edit.inserted)));
    }
    let (a, b) = (numbers.number(edit.start)?, numbers.number(edit.end() - 1)?);
    let (low, high) = (a.min(b), a.max(b));
    let place = if low == high {
        low.to_string()
    } else {
        format!("{low}_{high}")
    };
    Some(match (edit.deleted.len(), edit.inserted.len()) {
        (1, 1) => format!(
            "c.{place}{}>{}",
            strand(&edit.deleted),
            strand(&edit.inserted)
        ),
        (_, 0) => format!("c.{place}del"),
        _ => format!("c.{place}delins{}", strand(&edit.inserted)),
    })
}

#[cfg(test)]
mod tests {
    use super::{Effect, effect};
    use crate::codon::complement;
    use crate::{Feature, Region, Strand};

    /// 20 codons: M K R G F P D W N Q R I V T Y K E L S *.
    const CDS: &[u8; 60] = b"ATGAAACGTGGCTTTCCCGATTGGAACCAGCGCATTGTTACCTACAAGGAACTGTCTTAA";
    const EXONS: [(u64, u64); 3] = [(100, 120), (150, 170), (200, 230)];
    const CODING: (u64, u64) = (105, 225);

    fn transcript(strand: Strand) -> Feature {
        Feature::new("GENE", Region::between(100, 230), strand)
            .with_blocks(
                EXONS
                    .iter()
                    .map(|(start, end)| Region::between(*start, *end))
                    .collect(),
            )
            .with_coding(Some(Region::between(CODING.0, CODING.1)))
    }

    /// A genome of C bases with the coding sequence in the coding parts of the exons.
    fn genome(strand: Strand) -> Vec<u8> {
        let mut genome = vec![b'C'; 300];
        let positions: Vec<usize> = EXONS
            .iter()
            .flat_map(|(start, end)| *start..*end)
            .filter(|position| (CODING.0..CODING.1).contains(position))
            .map(|position| position as usize)
            .collect();
        let cds: Vec<u8> = if strand == Strand::Reverse {
            CDS.iter().rev().map(|base| complement(*base)).collect()
        } else {
            CDS.to_vec()
        };
        for (position, base) in positions.iter().zip(cds) {
            genome[*position] = base;
        }
        genome
    }

    fn change(strand: Strand, position: u64, reference: &str, alternate: &str) -> Effect {
        let genome = genome(strand);
        let mut bases =
            |region: Region| Some(genome[region.start() as usize..region.end() as usize].to_vec());
        let variant = (position, reference.as_bytes(), alternate.as_bytes());
        effect(&transcript(strand), variant, &mut bases).unwrap()
    }

    fn written(effect: &Effect) -> (Option<&str>, Option<&str>, Option<&str>) {
        (
            effect.coding.as_deref(),
            effect.protein.as_deref(),
            effect.place,
        )
    }

    #[test]
    fn substitutions_name_the_codon_change() {
        let forward = |position, reference, alternate| {
            let found = change(Strand::Forward, position, reference, alternate);
            (found.coding.unwrap(), found.protein.unwrap())
        };
        let pair = |coding: &str, protein: &str| (coding.to_owned(), protein.to_owned());
        assert_eq!(forward(112, "G", "A"), pair("c.8G>A", "p.Arg3His"));
        assert_eq!(forward(113, "T", "C"), pair("c.9T>C", "p.Arg3="));
        assert_eq!(forward(158, "G", "A"), pair("c.24G>A", "p.Trp8Ter"));
        assert_eq!(forward(105, "A", "G"), pair("c.1A>G", "p.Met1?"));
        assert_eq!(forward(222, "T", "C"), pair("c.58T>C", "p.Ter20Glnext*?"));
    }

    #[test]
    fn insertions_and_deletions_shift_or_keep_the_frame() {
        let forward = |position, reference, alternate| {
            let found = change(Strand::Forward, position, reference, alternate);
            (found.coding.unwrap(), found.protein.unwrap())
        };
        let pair = |coding: &str, protein: &str| (coding.to_owned(), protein.to_owned());
        assert_eq!(forward(114, "GG", "G"), pair("c.11del", "p.Gly4fs"));
        assert_eq!(forward(116, "CTTT", "C"), pair("c.13_15del", "p.Phe5del"));
        assert_eq!(
            forward(110, "A", "AAAA"),
            pair("c.6_7insAAA", "p.Lys2_Arg3insLys")
        );
        assert_eq!(forward(110, "A", "AT"), pair("c.6_7insT", "p.Arg3fs"));
    }

    #[test]
    fn variants_outside_the_coding_sequence_name_their_place() {
        let forward = |position, reference: &str, alternate: &str| {
            change(Strand::Forward, position, reference, alternate)
        };
        assert_eq!(
            written(&forward(120, "C", "T")),
            (Some("c.15+1C>T"), Some("p.?"), Some("splice site"))
        );
        assert_eq!(
            written(&forward(148, "C", "A")),
            (Some("c.16-2C>A"), Some("p.?"), Some("splice site"))
        );
        assert_eq!(
            written(&forward(135, "C", "A")),
            (None, None, Some("intron"))
        );
        assert_eq!(forward(101, "C", "A").place, Some("5′ UTR"));
        assert_eq!(forward(228, "C", "A").place, Some("3′ UTR"));
    }

    #[test]
    fn reverse_strand_transcripts_read_the_other_strand() {
        let reverse = |position, reference: &str, alternate: &str| {
            change(Strand::Reverse, position, reference, alternate)
        };
        assert_eq!(
            written(&reverse(217, "C", "T")),
            (Some("c.8G>A"), Some("p.Arg3His"), None)
        );
        assert_eq!(
            written(&reverse(199, "A", "G")),
            (Some("c.25+1T>C"), Some("p.?"), Some("splice site"))
        );
        assert_eq!(reverse(228, "C", "A").place, Some("5′ UTR"));
    }
}
