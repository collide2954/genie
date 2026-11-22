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
    /// An insertion of the same bases as on its 5′ side, in transcript order: a dup.
    duplicate: bool,
}

/// How far an insertion or a deletion moves at most to the 3′ end of the transcript.
const SHIFT: u64 = 1_000;

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
            duplicate: false,
        }
    }

    fn end(&self) -> u64 {
        self.start + self.deleted.len() as u64
    }

    /// The insertion or the deletion at the place nearest the 3′ end of the transcript that
    /// gives the same sequence, where HGVS names it. It stays in the span of the transcript.
    fn shifted(
        mut self,
        forward: bool,
        span: Region,
        bases: &mut dyn FnMut(Region) -> Option<Vec<u8>>,
    ) -> Self {
        let deletion = self.inserted.is_empty();
        let mut unit = match (self.deleted.is_empty(), deletion) {
            (false, true) => self.deleted.clone(),
            (true, false) => self.inserted.clone(),
            _ => return self,
        };
        let length = unit.len() as u64;
        if forward {
            let (from, to) = (self.end(), (self.end() + SHIFT).min(span.end()));
            if let Some(after) = (from < to)
                .then(|| bases(Region::between(from, to)))
                .flatten()
            {
                for base in after.iter().map(u8::to_ascii_uppercase) {
                    if base != unit[0] {
                        break;
                    }
                    unit.rotate_left(1);
                    self.start += 1;
                }
            }
        } else {
            let (from, to) = (
                self.start.saturating_sub(SHIFT).max(span.start()),
                self.start,
            );
            if let Some(before) = (from < to)
                .then(|| bases(Region::between(from, to)))
                .flatten()
            {
                for base in before.iter().rev().map(u8::to_ascii_uppercase) {
                    if Some(&base) != unit.last() {
                        break;
                    }
                    unit.rotate_right(1);
                    self.start -= 1;
                }
            }
        }
        if deletion {
            self.deleted = unit;
            return self;
        }
        let side = if forward {
            Region::between(self.start.saturating_sub(length), self.start)
        } else {
            Region::between(self.start, self.start + length)
        };
        self.duplicate = side.len() == length
            && bases(side).is_some_and(|found| found.eq_ignore_ascii_case(&unit));
        self.inserted = unit;
        self
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
        return format!("p.Ter{}{}extTer?", first + 1, amino(next));
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
        (0, 1) if first >= 1 && reference[first - 1..first] == *gained => {
            format!("p.{}{}dup", name(first - 1), first)
        }
        (0, size) if first >= size && reference[first - size..first] == *gained => format!(
            "p.{}{}_{}{}dup",
            name(first - size),
            first - size + 1,
            name(first - 1),
            first
        ),
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

/// Whether a feature is a transcript with a coding sequence: it has a strand, exons or a
/// coding part that is not all of it, and whole codons. The coding part of a plain interval,
/// for example from a BED file with 6 columns, is all of the interval.
pub fn readable(transcript: &Feature) -> bool {
    let Some(coding) = transcript.coding() else {
        return false;
    };
    let shaped = transcript.has_blocks() || coding != transcript.span();
    let length = Coding::new(transcript, coding).length;
    transcript.strand() != Strand::Unknown && shaped && length >= 3 && length.is_multiple_of(3)
}

/// The effect of a variant on a transcript with a coding part. `bases` gives the reference
/// bases of a region of the contig.
pub fn effect(
    transcript: &Feature,
    (position, reference, alternate): (u64, &[u8], &[u8]),
    bases: &mut dyn FnMut(Region) -> Option<Vec<u8>>,
) -> Option<Effect> {
    if !readable(transcript) {
        return None;
    }
    let coding = transcript.coding()?;
    let forward = transcript.strand() != Strand::Reverse;
    let edit = Edit::new(position, reference, alternate);
    if edit.deleted.is_empty() && edit.inserted.is_empty() {
        return None;
    }
    let edit = edit.shifted(forward, transcript.span(), bases);
    let exons = transcript.blocks();
    let exon = |position: u64| exons.iter().position(|exon| exon.contains(position));
    let (first, last) = if edit.deleted.is_empty() {
        (edit.start.checked_sub(1)?, edit.start)
    } else {
        (edit.start, edit.end() - 1)
    };
    let places = Places::new(transcript, coding);
    let name = || places.as_ref().and_then(|places| places.change(&edit));
    let unknown = |place| Effect {
        coding: name(),
        protein: Some("p.?".to_owned()),
        place: Some(place),
    };
    if let Some(found) = removed(exons, &edit, (first, last)) {
        return Some(Effect {
            coding: name(),
            ..found
        });
    }
    match (exon(first), exon(last)) {
        (Some(a), Some(b)) if a == b => {
            if coding.contains(first) && coding.contains(last) {
                coded(transcript, coding, &edit, bases, places.as_ref()?)
            } else if coding.contains(first) || coding.contains(last) {
                Some(unknown("start or stop codon"))
            } else {
                let upstream = (last < coding.start()) == forward;
                Some(Effect {
                    coding: name(),
                    ..Effect::place(if upstream { "5′ UTR" } else { "3′ UTR" })
                })
            }
        }
        (None, None) => match intronic(transcript, (first, last))? {
            "intron" => Some(Effect {
                coding: name(),
                ..Effect::place("intron")
            }),
            place => Some(unknown(place)),
        },
        _ => Some(unknown("splice site")),
    }
}

/// The effect of a deletion that takes whole exons away: all of them, or some of them.
fn removed(exons: &[Region], edit: &Edit, (first, last): (u64, u64)) -> Option<Effect> {
    if edit.deleted.is_empty() {
        return None;
    }
    let whole = exons
        .iter()
        .filter(|exon| first <= exon.start() && exon.end() <= last + 1)
        .count();
    let (place, protein) = match whole {
        0 => return None,
        all if all == exons.len() => ("transcript deletion", "p.0?"),
        _ => ("exon deletion", "p.?"),
    };
    Some(Effect {
        protein: Some(protein.to_owned()),
        ..Effect::place(place)
    })
}

/// The place of a variant in an intron: a splice site within 2 bases of an exon, else the
/// intron. Variants outside the transcript have no place.
fn intronic(transcript: &Feature, (first, last): (u64, u64)) -> Option<&'static str> {
    let span = transcript.span();
    if last < span.start() || first >= span.end() {
        return None;
    }
    let exons = transcript.blocks();
    let after = exons
        .iter()
        .filter(|exon| exon.end() <= first)
        .map(|exon| first - (exon.end() - 1))
        .min();
    let before = exons
        .iter()
        .filter(|exon| exon.start() > last)
        .map(|exon| exon.start() - last)
        .min();
    let near = [after, before].into_iter().flatten().min()?;
    Some(if near > SPLICE {
        "intron"
    } else {
        "splice site"
    })
}

/// A variant in the coding sequence.
fn coded(
    transcript: &Feature,
    coding: Region,
    edit: &Edit,
    bases: &mut dyn FnMut(Region) -> Option<Vec<u8>>,
    places: &Places,
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
    let coding = places.change(edit)?;
    if edit.inserted.iter().any(|base| !b"ACGT".contains(base)) {
        return Some(Effect {
            coding: Some(coding),
            protein: Some("p.?".to_owned()),
            place: None,
        });
    }
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

/// The c. names of the positions of a transcript: a number in the coding sequence, -N before
/// it and *N after it in the exons, and in an intron the nearest exon base with the distance
/// from it.
struct Places {
    /// The exons in genome order.
    exons: Vec<Region>,
    forward: bool,
    /// The number of exon bases.
    total: u64,
    /// The place of the first coding base among the exon bases, in transcript order.
    first: u64,
    /// The number of coding bases.
    coding: u64,
}

impl Places {
    fn new(transcript: &Feature, coding: Region) -> Option<Self> {
        let mut exons = transcript.blocks().to_vec();
        exons.sort_by_key(|exon| exon.start());
        let forward = transcript.strand() != Strand::Reverse;
        let mut places = Self {
            total: exons.iter().map(|exon| exon.len()).sum(),
            exons,
            forward,
            first: 0,
            coding: Coding::new(transcript, coding).length,
        };
        let start = if forward {
            coding.start()
        } else {
            coding.end().checked_sub(1)?
        };
        places.first = places.order(start)?;
        Some(places)
    }

    /// The place of an exon base among the exon bases, in transcript order.
    fn order(&self, position: u64) -> Option<u64> {
        let mut before = 0;
        for exon in &self.exons {
            if exon.contains(position) {
                let at = before + position - exon.start();
                return Some(if self.forward {
                    at
                } else {
                    self.total - 1 - at
                });
            }
            before += exon.len();
        }
        None
    }

    fn exonic(&self, position: u64) -> Option<String> {
        let at = self.order(position)?;
        Some(if at < self.first {
            format!("-{}", self.first - at)
        } else if at - self.first < self.coding {
            (at - self.first + 1).to_string()
        } else {
            format!("*{}", at - self.first - self.coding + 1)
        })
    }

    /// The name of a position of the transcript. A base in the middle of an intron takes the
    /// exon before it, as HGVS asks.
    fn name(&self, position: u64) -> Option<String> {
        if let Some(name) = self.exonic(position) {
            return Some(name);
        }
        let left = self
            .exons
            .iter()
            .filter(|exon| exon.end() <= position)
            .map(|exon| exon.end() - 1)
            .max()?;
        let right = self
            .exons
            .iter()
            .filter(|exon| exon.start() > position)
            .map(|exon| exon.start())
            .min()?;
        let (up, down) = if self.forward {
            (left, right)
        } else {
            (right, left)
        };
        let (to_up, to_down) = (up.abs_diff(position), down.abs_diff(position));
        Some(if to_up <= to_down {
            format!("{}+{to_up}", self.exonic(up)?)
        } else {
            format!("{}-{to_down}", self.exonic(down)?)
        })
    }

    /// The HGVS change of an edit, with the bases on the strand of the transcript.
    fn change(&self, edit: &Edit) -> Option<String> {
        let strand = |bases: &[u8]| -> String {
            let bases = if self.forward {
                bases.to_vec()
            } else {
                reverse_complement(bases)
            };
            String::from_utf8_lossy(&bases).into_owned()
        };
        let ordered = |a: String, b: String| if self.forward { (a, b) } else { (b, a) };
        if edit.duplicate {
            let length = edit.inserted.len() as u64;
            let (first, last) = if self.forward {
                (edit.start.checked_sub(length)?, edit.start.checked_sub(1)?)
            } else {
                (edit.start, edit.start + length - 1)
            };
            let (low, high) = ordered(self.name(first)?, self.name(last)?);
            return Some(if length == 1 {
                format!("c.{low}dup")
            } else {
                format!("c.{low}_{high}dup")
            });
        }
        if edit.deleted.is_empty() {
            let left = self.name(edit.start.checked_sub(1)?)?;
            let (low, high) = ordered(left, self.name(edit.start)?);
            return Some(format!("c.{low}_{high}ins{}", strand(&edit.inserted)));
        }
        let (low, high) = ordered(self.name(edit.start)?, self.name(edit.end() - 1)?);
        let place = if edit.deleted.len() == 1 {
            low
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
}

/// The bases of the coding part of a transcript on its exons, or nothing for a feature that is
/// not a transcript with a coding part.
pub fn coding_length(transcript: &Feature) -> Option<u64> {
    if !readable(transcript) {
        return None;
    }
    Some(Coding::new(transcript, transcript.coding()?).length)
}

/// The bases on the genome of the amino acids `first` to `last`, from 1, of the protein of a
/// transcript with a coding part, as blocks in genome order. Nothing when they are not all in
/// the protein.
pub fn protein_blocks(transcript: &Feature, (first, last): (u64, u64)) -> Option<Vec<Region>> {
    if !readable(transcript) || first == 0 || last < first {
        return None;
    }
    let coding = Coding::new(transcript, transcript.coding()?);
    let (from, to) = ((first - 1) * 3, last.checked_mul(3)?);
    if to > coding.length {
        return None;
    }
    let (from, to) = if coding.forward {
        (from, to)
    } else {
        (coding.length - to, coding.length - from)
    };
    let mut found = Vec::new();
    let mut before = 0;
    for block in &coding.blocks {
        let (start, end) = (before, before + block.len());
        let (low, high) = (from.max(start), to.min(end));
        if low < high {
            let offset = block.start() - start;
            found.push(Region::between(offset + low, offset + high));
        }
        before = end;
    }
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::{Effect, coding_length, effect, protein_blocks, readable};
    use crate::codon::complement;
    use crate::{Feature, Region, Strand};

    /// A transcript with exons at 100-130 and 200-260 and a coding part from 112 to 230: 16
    /// codons, 6 in the first exon and 10 in the second. On the reverse strand the protein
    /// starts at 230.
    #[test]
    fn amino_acids_lie_on_the_coding_exons() {
        let transcript = |strand| {
            Feature::new("T", Region::between(100, 260), strand)
                .with_blocks(vec![Region::between(100, 130), Region::between(200, 260)])
                .with_coding(Some(Region::between(112, 230)))
        };
        let forward = transcript(Strand::Forward);
        let blocks = |first, last| protein_blocks(&forward, (first, last));
        assert_eq!(blocks(1, 6), Some(vec![Region::between(112, 130)]));
        assert_eq!(
            blocks(6, 7),
            Some(vec![Region::between(127, 130), Region::between(200, 203)])
        );
        assert_eq!(blocks(16, 16), Some(vec![Region::between(227, 230)]));
        assert_eq!(blocks(16, 17), None);
        assert_eq!(blocks(0, 3), None);
        assert_eq!(blocks(5, 4), None);
        let reverse = transcript(Strand::Reverse);
        assert_eq!(
            protein_blocks(&reverse, (1, 6)),
            Some(vec![Region::between(212, 230)])
        );
        assert_eq!(
            protein_blocks(&reverse, (16, 16)),
            Some(vec![Region::between(112, 115)])
        );
        let plain = Feature::new("P", Region::between(0, 30), Strand::Unknown);
        assert_eq!(protein_blocks(&plain, (1, 2)), None);
        assert_eq!(coding_length(&forward), Some(48));
        assert_eq!(coding_length(&reverse), Some(48));
        assert_eq!(coding_length(&plain), None);
    }

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
        change_in(&genome(strand), strand, position, reference, alternate)
    }

    fn change_in(
        genome: &[u8],
        strand: Strand,
        position: u64,
        reference: &str,
        alternate: &str,
    ) -> Effect {
        let mut bases =
            |region: Region| Some(genome[region.start() as usize..region.end() as usize].to_vec());
        let variant = (position, reference.as_bytes(), alternate.as_bytes());
        effect(&transcript(strand), variant, &mut bases).unwrap()
    }

    /// The genome with A at some places, so that deletions there cannot move in the C bases
    /// around them.
    fn genome_with_a(strand: Strand, places: &[usize]) -> Vec<u8> {
        let mut genome = genome(strand);
        for place in places {
            genome[*place] = b'A';
        }
        genome
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
        assert_eq!(forward(222, "T", "C"), pair("c.58T>C", "p.Ter20GlnextTer?"));
        assert_eq!(forward(112, "G", "N"), pair("c.8G>N", "p.?"));
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
        assert_eq!(forward(110, "A", "AAAA"), pair("c.4_6dup", "p.Lys2dup"));
        assert_eq!(forward(110, "A", "AT"), pair("c.6_7insT", "p.Arg3fs"));
    }

    /// In a repeat, the same change can sit at several places. HGVS names the place nearest
    /// the 3′ end of the transcript, and an insertion of the bases before it is a dup.
    #[test]
    fn insertions_and_deletions_move_to_the_three_prime_end() {
        let on = |strand, position, reference, alternate| {
            let found = change(strand, position, reference, alternate);
            (found.coding.unwrap(), found.protein.unwrap())
        };
        let pair = |coding: &str, protein: &str| (coding.to_owned(), protein.to_owned());
        // c.4_6 is AAA.
        assert_eq!(
            on(Strand::Forward, 107, "G", "GA"),
            pair("c.6dup", "p.Arg3fs")
        );
        assert_eq!(
            on(Strand::Forward, 107, "GA", "G"),
            pair("c.6del", "p.Lys2fs")
        );
        assert_eq!(
            on(Strand::Reverse, 221, "T", "TT"),
            pair("c.6dup", "p.Arg3fs")
        );
        assert_eq!(
            on(Strand::Reverse, 220, "TT", "T"),
            pair("c.6del", "p.Lys2fs")
        );
        // When the intron starts with the same bases, a deletion at the end of an exon moves
        // into the intron, as ClinVar names SOD1 c.357_357+2del.
        let mut genome = genome(Strand::Forward);
        genome[120] = b'T';
        genome[121] = b'T';
        let mut bases =
            |region: Region| Some(genome[region.start() as usize..region.end() as usize].to_vec());
        let variant = (116, b"CTTT".as_slice(), b"C".as_slice());
        let found = effect(&transcript(Strand::Forward), variant, &mut bases).unwrap();
        assert_eq!(
            written(&found),
            (Some("c.15_15+2del"), Some("p.?"), Some("splice site"))
        );
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
            (Some("c.16-15C>A"), None, Some("intron"))
        );
        let lone = genome_with_a(Strand::Forward, &[134]);
        assert_eq!(
            written(&change_in(&lone, Strand::Forward, 133, "CA", "C")),
            (Some("c.15+15del"), None, Some("intron"))
        );
        assert_eq!(
            written(&forward(101, "C", "A")),
            (Some("c.-4C>A"), None, Some("5′ UTR"))
        );
        assert_eq!(
            written(&forward(228, "C", "A")),
            (Some("c.*4C>A"), None, Some("3′ UTR"))
        );
    }

    #[test]
    fn deletions_of_whole_exons_name_the_exons_or_the_transcript() {
        for strand in [Strand::Forward, Strand::Reverse] {
            let genome = genome_with_a(strand, &[90, 134, 140, 179, 239]);
            // A deletion as VCF gives it: the base before, then the deleted bases.
            let deleted = |start: usize, end: usize| {
                let reference = String::from_utf8(genome[start - 1..end].to_vec()).unwrap();
                change_in(
                    &genome,
                    strand,
                    start as u64 - 1,
                    &reference,
                    &reference[..1],
                )
            };
            let inside = if strand == Strand::Forward {
                "c.16-10_35+10del"
            } else {
                "c.26-10_45+10del"
            };
            assert_eq!(
                written(&deleted(140, 180)),
                (Some(inside), Some("p.?"), Some("exon deletion")),
                "{strand:?}"
            );
            assert_eq!(
                written(&deleted(90, 135)),
                (None, Some("p.?"), Some("exon deletion")),
                "{strand:?}"
            );
            assert_eq!(
                written(&deleted(90, 240)),
                (None, Some("p.0?"), Some("transcript deletion")),
                "{strand:?}"
            );
        }
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
        assert_eq!(
            written(&reverse(228, "C", "A")),
            (Some("c.-4G>T"), None, Some("5′ UTR"))
        );
        assert_eq!(
            written(&reverse(135, "C", "A")),
            (Some("c.45+15G>T"), None, Some("intron"))
        );
    }

    #[test]
    fn only_transcripts_with_a_strand_and_whole_codons_have_effects() {
        assert!(readable(&transcript(Strand::Forward)));
        assert!(readable(&transcript(Strand::Reverse)));
        assert!(!readable(&transcript(Strand::Unknown)));
        assert!(!readable(&transcript(Strand::Forward).with_coding(None)));
        let partial = transcript(Strand::Forward).with_coding(Some(Region::between(106, 225)));
        assert!(!readable(&partial));
        let interval = Feature::new("ENHANCER", Region::between(100, 160), Strand::Forward);
        assert!(!readable(&interval));
        let exon = interval.with_blocks(vec![Region::between(100, 160)]);
        assert!(readable(&exon));
        let genome = genome(Strand::Forward);
        let mut bases =
            |region: Region| Some(genome[region.start() as usize..region.end() as usize].to_vec());
        let unknown = transcript(Strand::Unknown);
        assert_eq!(effect(&unknown, (111, b"C", b"T"), &mut bases), None);
    }
}
