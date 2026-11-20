//! The review of the variants of a file: a mark for each variant that somebody looked at, true,
//! false or not sure, with a note.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::variant::Variant;

/// What somebody says about a variant after a look at its reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verdict {
    True,
    False,
    Unsure,
}

impl Verdict {
    pub const ALL: [Self; 3] = [Self::True, Self::False, Self::Unsure];

    /// The verdict as a word of a file: true, false or unsure.
    pub fn word(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::Unsure => "unsure",
        }
    }

    /// The verdict as people read it: true, false or not sure.
    pub fn label(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::Unsure => "not sure",
        }
    }

    /// A word or a label of a verdict, in any letter case.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "true" => Some(Self::True),
            "false" => Some(Self::False),
            "unsure" | "not sure" => Some(Self::Unsure),
            _ => None,
        }
    }
}

/// The mark of a variant: the verdict, the ID of the variant when somebody marked it, and a
/// note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub verdict: Verdict,
    pub id: String,
    pub note: String,
}

/// The place and the alleles of a variant, which name it in the reviews of a file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    pub contig: String,
    /// The start of the variant, from 0.
    pub position: u64,
    pub reference: String,
    /// The alternate alleles, with commas between them.
    pub alternates: String,
}

impl Key {
    pub fn of(contig: &str, variant: &Variant) -> Self {
        Self {
            contig: contig.to_owned(),
            position: variant.span.start(),
            reference: variant.reference.clone(),
            alternates: variant.alternates.join(","),
        }
    }
}

/// The marks of the variants of a file, in the order of their places.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reviews {
    marks: BTreeMap<Key, Review>,
}

impl Reviews {
    pub fn get(&self, key: &Key) -> Option<&Review> {
        self.marks.get(key)
    }

    /// The mark of a variant of a contig.
    pub fn of(&self, contig: &str, variant: &Variant) -> Option<&Review> {
        if self.marks.is_empty() {
            return None;
        }
        self.marks.get(&Key::of(contig, variant))
    }

    /// Marks a variant, or removes its mark.
    pub fn set(&mut self, key: Key, review: Option<Review>) {
        match review {
            Some(review) => self.marks.insert(key, review),
            None => self.marks.remove(&key),
        };
    }

    pub fn len(&self) -> usize {
        self.marks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// The number of the marks with a verdict.
    pub fn count(&self, verdict: Verdict) -> usize {
        let marks = self.marks.values();
        marks.filter(|review| review.verdict == verdict).count()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Key, &Review)> {
        self.marks.iter()
    }

    /// The marks as tab-separated text: a line of names, then a line for each mark with the
    /// position from 1.
    pub fn tsv(&self) -> String {
        let mut text = String::from("chrom\tpos\tid\tref\talt\treview\tnote\n");
        for (key, review) in &self.marks {
            let note = review.note.replace(['\t', '\n', '\r'], " ");
            let _ = writeln!(
                text,
                "{}\t{}\t{}\t{}\t{}\t{}\t{note}",
                key.contig,
                key.position + 1,
                id_word(&review.id),
                key.reference,
                key.alternates,
                review.verdict.word(),
            );
        }
        text
    }

    /// The marks as lines of a session: the verdict, the contig, the position from 1, the
    /// alleles, the ID (. for none) and the note, with spaces between them.
    pub fn lines(&self) -> Vec<String> {
        self.marks
            .iter()
            .map(|(key, review)| {
                let mut line = format!(
                    "{} {} {} {} {} {}",
                    review.verdict.word(),
                    key.contig,
                    key.position + 1,
                    key.reference,
                    key.alternates,
                    id_word(&review.id),
                );
                let note = review.note.replace(['\n', '\r'], " ");
                if !note.trim().is_empty() {
                    line.push(' ');
                    line.push_str(note.trim());
                }
                line
            })
            .collect()
    }

    /// The marks of the lines of a session. Bad lines give no mark.
    pub fn from_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            marks: lines.into_iter().filter_map(line_mark).collect(),
        }
    }
}

fn id_word(id: &str) -> &str {
    if id.is_empty() { "." } else { id }
}

/// The first word of a text, and the text after it.
fn word(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start();
    if text.is_empty() {
        return None;
    }
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    Some((&text[..end], &text[end..]))
}

/// The mark of a line of a session.
fn line_mark(line: &str) -> Option<(Key, Review)> {
    let (verdict, rest) = word(line)?;
    let verdict = Verdict::parse(verdict)?;
    let (contig, rest) = word(rest)?;
    let (position, rest) = word(rest)?;
    let position = position
        .parse::<u64>()
        .ok()
        .filter(|position| *position > 0)?;
    let (reference, rest) = word(rest)?;
    let (alternates, rest) = word(rest)?;
    let (id, rest) = word(rest)?;
    let key = Key {
        contig: contig.to_owned(),
        position: position - 1,
        reference: reference.to_owned(),
        alternates: alternates.to_owned(),
    };
    let review = Review {
        verdict,
        id: if id == "." {
            String::new()
        } else {
            id.to_owned()
        },
        note: rest.trim().to_owned(),
    };
    Some((key, review))
}

#[cfg(test)]
mod tests {
    use super::{Key, Review, Reviews, Verdict};
    use crate::Region;
    use crate::variant::{Annotations, Variant};

    fn variant(start: u64, reference: &str, alternates: &[&str]) -> Variant {
        Variant {
            id: String::new(),
            span: Region::between(start, start + reference.len() as u64),
            reference: reference.to_owned(),
            alternates: alternates.iter().map(|alt| (*alt).to_owned()).collect(),
            quality: None,
            filter: "PASS".to_owned(),
            info: String::new(),
            annotations: Annotations::default(),
            structure: None,
            calls: Vec::new(),
        }
    }

    fn review(verdict: Verdict, id: &str, note: &str) -> Review {
        Review {
            verdict,
            id: id.to_owned(),
            note: note.to_owned(),
        }
    }

    fn marked() -> Reviews {
        let mut reviews = Reviews::default();
        let snv = variant(99, "A", &["G"]);
        let both = variant(499, "C", &["T", "CA"]);
        reviews.set(
            Key::of("chr2", &snv),
            Some(review(Verdict::False, "rs2", "")),
        );
        reviews.set(
            Key::of("chr1", &snv),
            Some(review(Verdict::True, "", "seen in IGV; 30 reads")),
        );
        reviews.set(
            Key::of("chr1", &both),
            Some(review(Verdict::Unsure, "", "low MAPQ")),
        );
        reviews
    }

    #[test]
    fn marks_belong_to_the_place_and_the_alleles_of_a_variant() {
        let mut reviews = marked();
        let snv = variant(99, "A", &["G"]);
        assert_eq!(
            reviews.of("chr1", &snv).map(|found| found.verdict),
            Some(Verdict::True)
        );
        assert_eq!(reviews.of("chr1", &variant(99, "A", &["T"])), None);
        assert_eq!(reviews.of("chr3", &snv), None);
        assert_eq!((reviews.len(), reviews.count(Verdict::True)), (3, 1));
        reviews.set(Key::of("chr2", &snv), None);
        assert_eq!(reviews.len(), 2);
        assert!(Reviews::default().is_empty());
    }

    #[test]
    fn marks_give_tab_separated_text_in_the_order_of_their_places() {
        let text = marked().tsv();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines,
            [
                "chrom\tpos\tid\tref\talt\treview\tnote",
                "chr1\t100\t.\tA\tG\ttrue\tseen in IGV; 30 reads",
                "chr1\t500\t.\tC\tT,CA\tunsure\tlow MAPQ",
                "chr2\t100\trs2\tA\tG\tfalse\t",
            ]
        );
    }

    #[test]
    fn sessions_keep_marks_as_lines() {
        let reviews = marked();
        let lines = reviews.lines();
        assert_eq!(lines[0], "true chr1 100 A G . seen in IGV; 30 reads");
        assert_eq!(lines[2], "false chr2 100 A G rs2");
        let read = Reviews::from_lines(lines.iter().map(String::as_str));
        assert_eq!(read, reviews);
        let bad = [
            "maybe chr1 100 A G .",
            "true chr1 0 A G .",
            "true chr1 100 A",
            "",
        ];
        assert!(Reviews::from_lines(bad).is_empty());
        let spaced = Reviews::from_lines(["  unsure   chrX  7  T  C   rs7   two  words "]);
        let (key, found) = spaced.iter().next().unwrap();
        assert_eq!((key.contig.as_str(), key.position), ("chrX", 6));
        assert_eq!(
            (found.id.as_str(), found.note.as_str()),
            ("rs7", "two  words")
        );
    }

    #[test]
    fn verdicts_have_words_for_files_and_labels_for_people() {
        let words: Vec<&str> = Verdict::ALL.iter().map(|verdict| verdict.word()).collect();
        assert_eq!(words, ["true", "false", "unsure"]);
        assert_eq!(Verdict::Unsure.label(), "not sure");
        assert_eq!(Verdict::parse("TRUE"), Some(Verdict::True));
        assert_eq!(Verdict::parse("not sure"), Some(Verdict::Unsure));
        assert_eq!(Verdict::parse("maybe"), None);
    }
}
