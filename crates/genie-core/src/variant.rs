use std::cmp::Ordering;

use crate::Region;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    Missing,
    Reference,
    Het,
    Alternate,
}

impl Call {
    pub fn parse(genotype: &str) -> Self {
        let genotype = genotype.split(':').next().unwrap_or_default();
        let mut called = genotype
            .split(['/', '|'])
            .filter(|allele| !matches!(*allele, "." | ""));
        let Some(first) = called.next() else {
            return Self::Missing;
        };
        if called.any(|allele| allele != first) {
            Self::Het
        } else if first == "0" {
            Self::Reference
        } else {
            Self::Alternate
        }
    }
}

/// The fewest reads of a site that give a B-allele frequency.
pub const LEAST_DEPTH: u32 = 10;

/// What the FORMAT fields of a sample tell about the reads of a site: AD, DP, and a field of
/// fractions such as VAF.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reads {
    /// The reads of each allele (AD), the reference allele first.
    pub alleles: Vec<Option<u32>>,
    /// The depth of the site (DP).
    pub depth: Option<u32>,
    /// The fractions of the reads of the other alleles (VAF, AF or BAF).
    pub fractions: Vec<Option<f32>>,
}

/// The B-allele frequency of a sample at a site: the fraction of its reads that carry an allele
/// that is not the reference allele.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fraction {
    /// The start of the site, from 0.
    pub position: u64,
    /// From 0 to 1.
    pub value: f32,
    /// The reads of the site: all the reads of AD, else DP. Nothing when the file does not give
    /// them.
    pub depth: Option<u32>,
    pub call: Call,
}

impl Fraction {
    /// The fraction of a called site: from the reads of each allele, else from a field of
    /// fractions. Nothing for a site without a call or values, or with fewer than `LEAST_DEPTH`
    /// reads.
    pub fn new(position: u64, call: Call, reads: &Reads) -> Option<Self> {
        if call == Call::Missing {
            return None;
        }
        let (value, depth) = if let [Some(reference), ..] = reads.alleles[..] {
            let total = reads
                .alleles
                .iter()
                .flatten()
                .fold(0_u32, |sum, count| sum.saturating_add(*count));
            if total == 0 {
                return None;
            }
            ((total - reference) as f32 / total as f32, Some(total))
        } else {
            let mut known = reads
                .fractions
                .iter()
                .flatten()
                .filter(|value| value.is_finite())
                .peekable();
            known.peek()?;
            (known.sum::<f32>().clamp(0.0, 1.0), reads.depth)
        };
        if depth.is_some_and(|depth| depth < LEAST_DEPTH) {
            return None;
        }
        Some(Self {
            position,
            value,
            depth,
            call,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Snv(u8),
    Insertion,
    Deletion,
    Complex,
}

/// How much a change matters to a transcript, as Ensembl VEP and `SnpEff` rank it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Impact {
    #[default]
    Modifier,
    Low,
    Moderate,
    High,
}

impl Impact {
    pub fn word(self) -> &'static str {
        match self {
            Self::High => "HIGH",
            Self::Moderate => "MODERATE",
            Self::Low => "LOW",
            Self::Modifier => "MODIFIER",
        }
    }
}

/// What a tool such as VEP, `SnpEff` or bcftools csq says that an allele does to a transcript.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Annotation {
    pub allele: String,
    pub consequences: Vec<String>,
    pub impact: Impact,
    pub gene: String,
    pub transcript: String,
    pub biotype: String,
    /// The change on the transcript, for example c.5095C>T.
    pub coding: String,
    /// The change on the protein, for example p.Arg1699Trp.
    pub protein: String,
}

/// The annotations of a variant in its file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Annotations {
    /// The most severe annotations, the most severe first.
    pub first: Vec<Annotation>,
    /// The number of the other annotations.
    pub others: usize,
    /// Fields that list the values of all annotations, then the other columns of the first.
    pub fields: Vec<(String, String)>,
}

/// The type of a structural variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sv {
    Deletion,
    Duplication,
    Inversion,
    Insertion,
    CopyNumber,
    Breakend,
    /// A rearrangement of several parts, as the CPX records of gnomAD-SV.
    Complex,
}

/// What a VCF record says about a structural variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Structure {
    pub kind: Sv,
    /// The number of bases that the variant changes, from SVLEN or from its span.
    pub length: Option<u64>,
    /// The contig and the position (from 0) of the other end of a breakend.
    pub mate: Option<(String, u64)>,
    /// The confidence intervals of the start and the end, in bases around them.
    pub around: [Option<(i64, i64)>; 2],
    /// False when the caller marks the variant as IMPRECISE.
    pub precise: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub id: String,
    pub span: Region,
    pub reference: String,
    pub alternates: Vec<String>,
    pub quality: Option<f32>,
    pub filter: String,
    pub info: String,
    pub annotations: Annotations,
    pub structure: Option<Structure>,
    pub calls: Vec<Call>,
}

impl Variant {
    pub fn kind(&self) -> Kind {
        let Some(alternate) = self.alternates.first() else {
            return Kind::Complex;
        };
        if let Some(symbol) = alternate.strip_prefix('<') {
            return if symbol.starts_with("DEL") {
                Kind::Deletion
            } else if symbol.starts_with("INS") {
                Kind::Insertion
            } else {
                Kind::Complex
            };
        }
        if matches!(alternate.as_str(), "." | "*") || alternate.contains(['[', ']']) {
            return Kind::Complex;
        }
        match self.reference.len().cmp(&alternate.len()) {
            Ordering::Equal if alternate.len() == 1 => {
                Kind::Snv(alternate.as_bytes()[0].to_ascii_uppercase())
            }
            Ordering::Equal => Kind::Complex,
            Ordering::Less => Kind::Insertion,
            Ordering::Greater => Kind::Deletion,
        }
    }

    pub fn passes(&self) -> bool {
        matches!(self.filter.as_str(), "PASS" | "." | "")
    }
}

#[cfg(test)]
mod tests {
    use super::{Annotations, Call, Fraction, Kind, Reads, Variant};
    use crate::Region;

    fn reads(alleles: &[Option<u32>], depth: Option<u32>, fractions: &[Option<f32>]) -> Reads {
        Reads {
            alleles: alleles.to_vec(),
            depth,
            fractions: fractions.to_vec(),
        }
    }

    #[test]
    fn fractions_come_from_the_reads_of_each_allele() {
        let het = Fraction::new(99, Call::Het, &reads(&[Some(12), Some(10)], Some(30), &[]));
        let het = het.unwrap();
        assert_eq!(
            (het.position, het.depth, het.call),
            (99, Some(22), Call::Het)
        );
        assert!((het.value - 10.0 / 22.0).abs() < 1e-6);
        let both = reads(&[Some(2), Some(9), None, Some(9)], None, &[Some(0.1)]);
        let both = Fraction::new(5, Call::Het, &both).unwrap();
        assert!((both.value - 0.9).abs() < 1e-6);
        assert_eq!(both.depth, Some(20));
        let reference = Fraction::new(5, Call::Reference, &reads(&[Some(31)], None, &[]));
        assert_eq!(reference.map(|found| found.value), Some(0.0));
        let few = reads(&[Some(4), Some(5)], Some(40), &[]);
        assert_eq!(Fraction::new(5, Call::Het, &few), None);
        let deep = reads(&[Some(20), Some(20)], None, &[]);
        assert_eq!(Fraction::new(5, Call::Missing, &deep), None);
        let none = reads(&[Some(0), Some(0)], None, &[]);
        assert_eq!(Fraction::new(5, Call::Het, &none), None);
    }

    #[test]
    fn fractions_without_allele_reads_come_from_a_fraction_field() {
        let vaf = Fraction::new(7, Call::Het, &reads(&[], Some(25), &[Some(0.48)])).unwrap();
        assert_eq!((vaf.value, vaf.depth), (0.48, Some(25)));
        let array = reads(&[], None, &[Some(0.97)]);
        let array = Fraction::new(7, Call::Alternate, &array).unwrap();
        assert_eq!((array.value, array.depth), (0.97, None));
        let two = reads(&[], Some(30), &[Some(0.6), Some(0.5)]);
        assert_eq!(
            Fraction::new(7, Call::Het, &two).map(|found| found.value),
            Some(1.0)
        );
        let unknown = reads(&[None, Some(3)], Some(30), &[Some(0.25), Some(f32::NAN)]);
        let unknown = Fraction::new(7, Call::Het, &unknown).unwrap();
        assert_eq!((unknown.value, unknown.depth), (0.25, Some(30)));
        let shallow = reads(&[], Some(9), &[Some(0.5)]);
        assert_eq!(Fraction::new(7, Call::Het, &shallow), None);
        assert_eq!(
            Fraction::new(7, Call::Het, &reads(&[], Some(30), &[None])),
            None
        );
        assert_eq!(Fraction::new(7, Call::Het, &Reads::default()), None);
    }

    fn variant(reference: &str, alternates: &[&str]) -> Variant {
        Variant {
            id: String::new(),
            span: Region::between(100, 100 + reference.len() as u64),
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

    #[test]
    fn kinds_follow_the_first_alternate() {
        assert_eq!(variant("A", &["G"]).kind(), Kind::Snv(b'G'));
        assert_eq!(variant("a", &["t", "C"]).kind(), Kind::Snv(b'T'));
        assert_eq!(variant("A", &["ACGT"]).kind(), Kind::Insertion);
        assert_eq!(variant("ACGT", &["A"]).kind(), Kind::Deletion);
        assert_eq!(variant("AC", &["GT"]).kind(), Kind::Complex);
        assert_eq!(variant("A", &["<DEL>"]).kind(), Kind::Deletion);
        assert_eq!(variant("A", &["<INS:ME>"]).kind(), Kind::Insertion);
        assert_eq!(variant("A", &["<DUP>"]).kind(), Kind::Complex);
        assert_eq!(variant("A", &["."]).kind(), Kind::Complex);
    }

    #[test]
    fn pass_and_missing_filters_pass() {
        let mut site = variant("A", &["G"]);
        assert!(site.passes());
        site.filter = ".".to_owned();
        assert!(site.passes());
        site.filter = "LowQual".to_owned();
        assert!(!site.passes());
    }

    #[test]
    fn calls_read_genotypes() {
        let calls: Vec<Call> = [
            "0/0",
            "0|1",
            "1/0",
            "1/1",
            "1|2",
            "./.",
            ".",
            "1",
            "0",
            "./1",
            "0/0:30:12",
            "2/2",
            "1/1/2",
        ]
        .iter()
        .map(|genotype| Call::parse(genotype))
        .collect();
        assert_eq!(
            calls,
            [
                Call::Reference,
                Call::Het,
                Call::Het,
                Call::Alternate,
                Call::Het,
                Call::Missing,
                Call::Missing,
                Call::Alternate,
                Call::Reference,
                Call::Alternate,
                Call::Reference,
                Call::Alternate,
                Call::Het,
            ]
        );
    }
}
